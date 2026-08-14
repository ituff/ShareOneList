use serde::{Deserialize, Serialize};
use tauri::State;
use tokio::sync::Mutex;

use crate::auth::cloud_config::CloudEnvironment;
use crate::auth::AuthModule;
use crate::config::ConfigManager;
use crate::errors::AppError;
use crate::graph::GraphClient;
use crate::models::AccountEntry;

/// Account information returned to the frontend after a successful login.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AccountInfo {
    pub home_account_id: String,
    pub display_name: String,
    pub drive_id: String,
    pub cloud_env: String,
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
}

/// Minimal /me response used to resolve a stable account ID and display name.
#[derive(Debug, Deserialize)]
struct MeResponse {
    id: Option<String>,
    #[serde(rename = "displayName")]
    display_name: Option<String>,
}

/// Resolve the user's personal OneDrive drive ID after authentication.
async fn fetch_me_drive_id(env: &CloudEnvironment, token: &str) -> Result<String, AppError> {
    let client = GraphClient::new(env.clone());
    let url = format!("{}/me/drive", client.base_url());
    let response = client
        .request_with_retry(token, |http, tkn| http.get(&url).bearer_auth(tkn))
        .await?;
    let drive: MeDriveResponse = response.json().await.map_err(|e| AppError::GraphApi {
        message: format!("Failed to parse /me/drive response: {}", e),
        status_code: 0,
    })?;
    drive
        .id
        .filter(|id| !id.is_empty())
        .ok_or_else(|| AppError::GraphApi {
            message: "The account does not have a personal OneDrive drive".to_string(),
            status_code: 0,
        })
}

/// Fetch the signed-in user profile so account entries have a stable ID and real name.
async fn fetch_me_info(env: &CloudEnvironment, token: &str) -> Result<MeResponse, AppError> {
    let client = GraphClient::new(env.clone());
    let url = format!("{}/me?$select=id,displayName", client.base_url());
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

    let drive_id = fetch_me_drive_id(&env, &session.access_token).await?;
    let me_info = fetch_me_info(&env, &session.access_token).await.ok();
    let account_id = me_info
        .as_ref()
        .and_then(|m| m.id.as_ref())
        .filter(|id| !id.is_empty())
        .cloned()
        .unwrap_or_else(|| session.home_account_id.clone());
    let display_name = me_info
        .as_ref()
        .and_then(|m| m.display_name.as_ref())
        .filter(|name| !name.is_empty())
        .cloned()
        .unwrap_or_else(|| session.display_name.clone());

    let account_info = AccountInfo {
        home_account_id: account_id.clone(),
        display_name: display_name.clone(),
        drive_id: drive_id.clone(),
        cloud_env: cloud_env.to_lowercase(),
    };

    let mut accounts = config_manager.load_accounts();
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
        display_name: display_name.clone(),
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
