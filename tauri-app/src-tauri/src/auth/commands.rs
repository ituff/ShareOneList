use serde::{Deserialize, Serialize};
use tauri::State;
use tokio::sync::Mutex;

use crate::auth::cloud_config::CloudEnvironment;
use crate::auth::AuthModule;
use crate::config::ConfigManager;
use crate::errors::AppError;
use crate::graph::GraphClient;
use crate::models::{AccountCategory, AccountEntry};

/// Account information returned to the frontend after a successful login.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AccountInfo {
    pub home_account_id: String,
    pub display_name: String,
    pub drive_id: String,
    pub cloud_env: String,
    pub account_type: Option<AccountCategory>,
}

/// Parse a cloud environment string ("global" or "china") into the enum.
fn parse_cloud_env(cloud_env: &str) -> Result<CloudEnvironment, AppError> {
    match cloud_env.to_lowercase().as_str() {
        "global" => Ok(CloudEnvironment::Global),
        "china" => Ok(CloudEnvironment::China),
        _ => Err(AppError::Validation {
            message: format!(
                "Invalid cloud environment '{}'. Expected 'global' or 'china'.",
                cloud_env
            ),
            field: "cloud_env".to_string(),
        }),
    }
}

/// Minimal /me/drive response used to resolve the user's OneDrive ID.
#[derive(Debug, Deserialize)]
struct MeDriveResponse {
    id: Option<String>,
    #[serde(rename = "driveType")]
    drive_type: Option<String>,
}

/// Minimal /me response used to resolve the account display name.
#[derive(Debug, Deserialize)]
struct MeResponse {
    #[serde(rename = "displayName")]
    display_name: Option<String>,
}

/// Resolve the user's personal OneDrive drive ID after authentication.
async fn fetch_me_drive(
    env: &CloudEnvironment,
    token: &str,
) -> Result<MeDriveResponse, AppError> {
    let client = GraphClient::new(env.clone());
    let url = format!("{}/me/drive?$select=id,driveType", client.base_url());
    let response = client
        .request_with_retry(token, |http, tkn| http.get(&url).bearer_auth(tkn))
        .await?;
    let drive: MeDriveResponse = response.json().await.map_err(|e| AppError::GraphApi {
        message: format!("Failed to parse /me/drive response: {}", e),
        status_code: 0,
    })?;
    let id = drive
        .id
        .filter(|id| !id.is_empty())
        .ok_or_else(|| AppError::GraphApi {
            message: "The account does not have a personal OneDrive drive".to_string(),
            status_code: 0,
        })?;
    Ok(MeDriveResponse {
        id: Some(id),
        drive_type: drive.drive_type,
    })
}

/// Final classification of an account kind, combining the ID token `tid`
/// heuristic with the authoritative OneDrive `driveType` of the signed-in
/// account. The drive type wins because it reflects the actual service the
/// account uses: `personal` is a consumer OneDrive (MSA identity) and
/// `business` is OneDrive for Business (Entra identity). This corrects the
/// known misdetection where a personal account signing in through an Entra
/// context gets an organizational `tid`.
fn reconcile_account_category(
    env: &CloudEnvironment,
    tid_category: Option<AccountCategory>,
    drive_type: Option<&str>,
) -> Option<AccountCategory> {
    match env {
        CloudEnvironment::China => Some(AccountCategory::Organization),
        CloudEnvironment::Global => match drive_type.map(str::trim) {
            Some("personal") => Some(AccountCategory::Personal),
            Some("business") => Some(AccountCategory::Organization),
            _ => tid_category,
        },
    }
}

/// Fetch the signed-in user profile so account entries have a stable ID and real name.
async fn fetch_me_info(env: &CloudEnvironment, token: &str) -> Result<MeResponse, AppError> {
    let client = GraphClient::new(env.clone());
    let url = format!("{}/me?$select=displayName", client.base_url());
    let response = client
        .request_with_retry(token, |http, tkn| http.get(&url).bearer_auth(tkn))
        .await?;
    response
        .json::<MeResponse>()
        .await
        .map_err(|e| AppError::GraphApi {
            message: format!("Failed to parse /me response: {}", e),
            status_code: 0,
        })
}

/// Initiates OAuth2 login for the specified cloud environment.
///
/// Opens the system browser, completes the PKCE authorization code flow,
/// persists the account entry via ConfigManager, and returns account info.
#[tauri::command]
pub async fn login(
    cloud_env: String,
    auth_module: State<'_, Mutex<AuthModule>>,
    config_manager: State<'_, ConfigManager>,
) -> Result<AccountInfo, AppError> {
    let env = parse_cloud_env(&cloud_env)?;

    // Perform login (requires mutable access to AuthModule)
    let session = {
        let mut auth = auth_module.lock().await;
        auth.login(env.clone()).await?
    };

    let drive = fetch_me_drive(&env, &session.access_token).await?;
    let drive_id = drive.id.clone().unwrap_or_default();
    let me_info = fetch_me_info(&env, &session.access_token).await.ok();
    // The session's home account ID (from the ID token `oid` claim) is the one
    // stable identity across logins. Graph `/me` id intentionally NOT used as
    // the key: for personal (MSA) accounts it is a different identifier (the
    // cid), which used to fork the same person into duplicate account entries
    // whenever a login resolved through a different path.
    let account_id = session.home_account_id.clone();
    let display_name = me_info
        .as_ref()
        .and_then(|m| m.display_name.as_ref())
        .filter(|name| !name.is_empty())
        .cloned()
        .unwrap_or_else(|| session.display_name.clone());
    // The OneDrive driveType is the authoritative personal/organization signal.
    let account_type = reconcile_account_category(
        &env,
        session.account_type,
        drive.drive_type.as_deref(),
    );

    let account_info = AccountInfo {
        home_account_id: account_id.clone(),
        display_name: display_name.clone(),
        drive_id: drive_id.clone(),
        cloud_env: cloud_env.to_lowercase(),
        account_type,
    };

    let mut accounts = config_manager.load_accounts();
    // Carry over the user-customized alias and icon from the replaced entry
    // so a re-login does not wipe personalization.
    let previous = accounts
        .iter()
        .find(|a| a.home_account_id == account_id)
        .cloned();
    // Replace any stale duplicate entry for the same user, drive, or legacy display name.
    accounts.retain(|a| {
        a.home_account_id != account_id
            && a.drive_id != drive_id
            && !(a.cloud_type == env && a.display_name == display_name)
    });
    accounts.push(AccountEntry {
        home_account_id: account_id.clone(),
        drive_id: drive_id.clone(),
        cloud_type: env.clone(),
        display_name,
        account_type,
        alias: previous.as_ref().and_then(|a| a.alias.clone()),
        icon: previous.as_ref().and_then(|a| a.icon.clone()),
    });
    config_manager
        .save_accounts(&accounts)
        .map_err(|e| AppError::Config {
            message: e.to_string(),
        })?;

    {
        let mut auth = auth_module.lock().await;
        auth.register_session(&env, session, &account_id, &drive_id);
    }

    Ok(account_info)
}

/// Logs out the session for the specified cloud environment.
///
/// Clears tokens from the keyring, removes the session from AuthModule,
/// and removes the corresponding account entry from ConfigManager.
#[tauri::command]
pub async fn logout(
    cloud_env: String,
    home_account_id: Option<String>,
    auth_module: State<'_, Mutex<AuthModule>>,
    config_manager: State<'_, ConfigManager>,
) -> Result<(), AppError> {
    let env = parse_cloud_env(&cloud_env)?;

    // Perform logout (clears tokens from keyring)
    {
        let mut auth = auth_module.lock().await;
        auth.logout(env.clone(), home_account_id.as_deref()).await?;
    }

    // Remove the account entry from persisted accounts
    let mut accounts = config_manager.load_accounts();
    accounts.retain(|a| match &home_account_id {
        Some(id) => a.home_account_id != *id,
        None => a.cloud_type != env,
    });
    config_manager
        .save_accounts(&accounts)
        .map_err(|e| AppError::Config {
            message: e.to_string(),
        })?;

    Ok(())
}

/// Update an account's user-set alias and/or icon.
///
/// `None` leaves the field unchanged; an empty string resets it to the default.
/// Returns the updated account entry.
#[tauri::command]
pub async fn update_account(
    cloud_env: String,
    home_account_id: String,
    alias: Option<String>,
    icon: Option<String>,
    config_manager: State<'_, ConfigManager>,
) -> Result<AccountEntry, AppError> {
    let env = parse_cloud_env(&cloud_env)?;

    let mut accounts = config_manager.load_accounts();
    let entry = accounts
        .iter_mut()
        .find(|a| a.home_account_id == home_account_id && a.cloud_type == env)
        .ok_or_else(|| AppError::Validation {
            message: format!("Account '{}' not found.", home_account_id),
            field: "home_account_id".to_string(),
        })?;

    if let Some(value) = alias {
        entry.alias = Some(value.trim().to_string()).filter(|s| !s.is_empty());
    }
    if let Some(value) = icon {
        entry.icon = Some(value.trim().to_string()).filter(|s| !s.is_empty());
    }
    let updated = entry.clone();

    config_manager
        .save_accounts(&accounts)
        .map_err(|e| AppError::Config {
            message: e.to_string(),
        })?;

    Ok(updated)
}

/// Re-derive an account's personal/organization classification from the live
/// OneDrive driveType and persist it. Used to heal entries saved before the
/// driveType-based detection (or misclassified by the old tid-only logic).
/// Returns the resolved category ("personal" / "organization") or null when
/// the drive type is unavailable.
#[tauri::command]
pub async fn refresh_account_type(
    cloud_env: String,
    home_account_id: String,
    auth_module: State<'_, Mutex<AuthModule>>,
    config_manager: State<'_, ConfigManager>,
) -> Result<Option<String>, AppError> {
    let env = parse_cloud_env(&cloud_env)?;
    let token = {
        let mut auth = auth_module.lock().await;
        auth.get_token_for_account(env.clone(), &home_account_id)
            .await?
    };

    let drive_type = match fetch_me_drive(&env, &token).await {
        Ok(drive) => drive.drive_type,
        // Accounts without any OneDrive drive keep their current classification.
        Err(_) => None,
    };
    let category = reconcile_account_category(&env, None, drive_type.as_deref());

    let mut accounts = config_manager.load_accounts();
    if let Some(entry) = accounts
        .iter_mut()
        .find(|a| a.home_account_id == home_account_id && a.cloud_type == env)
    {
        // A missing driveType must not erase a previously known category.
        if category.is_some() {
            entry.account_type = category;
        }
        let saved = entry.clone();
        config_manager
            .save_accounts(&accounts)
            .map_err(|e| AppError::Config {
                message: e.to_string(),
            })?;
        return Ok(saved.account_type.map(|c| c.to_string()));
    }

    Ok(category.map(|c| c.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reconcile_prefers_drive_type_over_tid() {
        let global = CloudEnvironment::Global;
        let china = CloudEnvironment::China;
        let org = Some(AccountCategory::Organization);
        let personal = Some(AccountCategory::Personal);

        // driveType wins over a (wrong) tid-derived classification.
        assert_eq!(reconcile_account_category(&global, org, Some("personal")), personal);
        assert_eq!(reconcile_account_category(&global, personal, Some("business")), org);
        // Unknown or missing driveType falls back to the tid classification.
        assert_eq!(reconcile_account_category(&global, org, Some("documentLibrary")), org);
        assert_eq!(reconcile_account_category(&global, personal, None), personal);
        assert_eq!(reconcile_account_category(&global, None, None), None);
        // 21Vianet is always organizational.
        assert_eq!(reconcile_account_category(&china, None, Some("personal")), org);
    }
}
