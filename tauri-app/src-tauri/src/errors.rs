use serde::Serialize;

use crate::auth::cloud_config::CloudEnvironment;

/// Unified application error type returned from Tauri commands.
#[derive(Debug, Serialize)]
#[serde(tag = "type")]
pub enum AppError {
    Network {
        message: String,
        retryable: bool,
    },
    Auth {
        message: String,
        cloud_env: CloudEnvironment,
    },
    GraphApi {
        message: String,
        status_code: u16,
    },
    FileSystem {
        message: String,
        path: String,
    },
    Config {
        message: String,
    },
    Transfer {
        message: String,
        task_id: String,
    },
    Validation {
        message: String,
        field: String,
    },
}

impl std::fmt::Display for AppError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AppError::Network { message, .. } => write!(f, "Network error: {}", message),
            AppError::Auth { message, .. } => write!(f, "Auth error: {}", message),
            AppError::GraphApi { message, status_code } => {
                write!(f, "Graph API error ({}): {}", status_code, message)
            }
            AppError::FileSystem { message, path } => {
                write!(f, "File system error at '{}': {}", path, message)
            }
            AppError::Config { message } => write!(f, "Config error: {}", message),
            AppError::Transfer { message, task_id } => {
                write!(f, "Transfer error (task {}): {}", task_id, message)
            }
            AppError::Validation { message, field } => {
                write!(f, "Validation error on '{}': {}", field, message)
            }
        }
    }
}

impl std::error::Error for AppError {}

// AppError already implements Serialize, which satisfies Tauri's Into<InvokeError>
// via the blanket impl `impl<T: Serialize> From<T> for InvokeError`.
// No manual From impl needed.
