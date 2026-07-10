use serde::Serialize;
use tauri::State;
use tokio::sync::Mutex;

use crate::auth::cloud_config::CloudEnvironment;
use crate::auth::AuthModule;
use crate::config::ConfigManager;
use crate::errors::AppError;
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

    // Build the account info to return
    let account_info = AccountInfo {
        home_account_id: session.home_account_id.clone(),
        display_name: session.display_name.clone(),
        // For now drive_id is derived from the home_account_id;
        // the actual drive_id is fetched via Graph API after login.
        // We use a placeholder that the frontend will replace after querying /me/drive.
        drive_id: String::new(),
        cloud_env: cloud_env.to_lowercase(),
    };

    // Persist account entry via ConfigManager
    let mut accounts = config_manager.load_accounts();
    let already_exists = accounts
        .iter()
        .any(|a| a.home_account_id == session.home_account_id);

    if !already_exists {
        accounts.push(AccountEntry {
            home_account_id: session.home_account_id.clone(),
            drive_id: account_info.drive_id.clone(),
            cloud_type: env,
            display_name: session.display_name.clone(),
        });
        config_manager.save_accounts(&accounts).map_err(|e| AppError::Config {
            message: e.to_string(),
        })?;
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
    auth_module: State<'_, Mutex<AuthModule>>,
    config_manager: State<'_, ConfigManager>,
) -> Result<(), AppError> {
    let env = parse_cloud_env(&cloud_env)?;

    // Perform logout (clears tokens from keyring)
    {
        let mut auth = auth_module.lock().await;
        auth.logout(env.clone()).await?;
    }

    // Remove the account entry from persisted accounts
    let mut accounts = config_manager.load_accounts();
    accounts.retain(|a| a.cloud_type != env);
    config_manager.save_accounts(&accounts).map_err(|e| AppError::Config {
        message: e.to_string(),
    })?;

    Ok(())
}
