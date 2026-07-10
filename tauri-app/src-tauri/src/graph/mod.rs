// Graph API client module
// Handles all Microsoft Graph API communication with environment-aware endpoints

pub mod commands;
pub mod validators;

use std::time::Duration;

use reqwest::{Client, RequestBuilder, Response};

use crate::auth::cloud_config::CloudEnvironment;
use crate::errors::AppError;

/// Microsoft Graph API client with environment-aware endpoints and retry logic.
pub struct GraphClient {
    client: Client,
    cloud_env: CloudEnvironment,
}

/// Configuration for retry behavior on transient failures.
struct RetryConfig {
    max_attempts: u32,
    initial_delay: Duration,
    backoff_multiplier: u64,
    max_delay: Duration,
}

impl Default for RetryConfig {
    fn default() -> Self {
        Self {
            max_attempts: 3,
            initial_delay: Duration::from_secs(1),
            backoff_multiplier: 2,
            max_delay: Duration::from_secs(30),
        }
    }
}

impl GraphClient {
    /// Creates a new `GraphClient` for the given cloud environment.
    pub fn new(cloud_env: CloudEnvironment) -> Self {
        let client = Client::builder()
            .timeout(Duration::from_secs(30))
            .build()
            .expect("failed to build reqwest client");

        Self { client, cloud_env }
    }

    /// Returns the Graph API base URL for the configured cloud environment.
    pub fn base_url(&self) -> &str {
        match self.cloud_env {
            CloudEnvironment::Global => "https://graph.microsoft.com/v1.0",
            CloudEnvironment::China => "https://microsoftgraph.chinacloudapi.cn/v1.0",
        }
    }

    /// Returns a reference to the underlying `reqwest::Client`.
    pub fn http_client(&self) -> &Client {
        &self.client
    }

    /// Returns a reference to the cloud environment.
    pub fn cloud_env(&self) -> &CloudEnvironment {
        &self.cloud_env
    }

    /// Executes a request with retry logic for transient failures.
    ///
    /// Retries on:
    /// - HTTP 5xx server errors
    /// - Network connection errors
    /// - Request timeouts
    ///
    /// Does NOT retry on:
    /// - HTTP 4xx client errors (returned immediately as errors)
    /// - Successful responses (returned immediately)
    ///
    /// Uses exponential backoff: 1s, 2s, 4s (capped at 3 attempts, max delay 30s).
    pub async fn request_with_retry<F>(&self, token: &str, build_request: F) -> Result<Response, AppError>
    where
        F: Fn(&Client, &str) -> RequestBuilder,
    {
        let config = RetryConfig::default();
        let mut last_error: Option<AppError> = None;

        for attempt in 0..config.max_attempts {
            let request_builder = build_request(&self.client, token);

            match request_builder.send().await {
                Ok(response) => {
                    let status = response.status();

                    if status.is_success() {
                        return Ok(response);
                    }

                    if status.is_client_error() {
                        // 4xx errors are not retryable
                        let status_code = status.as_u16();
                        let message = response
                            .text()
                            .await
                            .unwrap_or_else(|_| format!("HTTP {}", status_code));
                        return Err(AppError::GraphApi {
                            message,
                            status_code,
                        });
                    }

                    if status.is_server_error() {
                        // 5xx errors are retryable
                        let status_code = status.as_u16();
                        let message = response
                            .text()
                            .await
                            .unwrap_or_else(|_| format!("HTTP {}", status_code));
                        last_error = Some(AppError::GraphApi {
                            message,
                            status_code,
                        });
                    }
                }
                Err(err) => {
                    if is_retryable_error(&err) {
                        last_error = Some(AppError::Network {
                            message: err.to_string(),
                            retryable: true,
                        });
                    } else {
                        // Non-retryable network error, fail immediately
                        return Err(AppError::Network {
                            message: err.to_string(),
                            retryable: false,
                        });
                    }
                }
            }

            // Wait before next attempt (skip sleep after the last attempt)
            if attempt < config.max_attempts - 1 {
                let delay = calculate_delay(&config, attempt);
                tokio::time::sleep(delay).await;
            }
        }

        // All retries exhausted
        Err(last_error.unwrap_or_else(|| AppError::Network {
            message: "request failed after all retry attempts".into(),
            retryable: false,
        }))
    }
}

/// Determines whether a reqwest error is retryable (connection or timeout errors).
fn is_retryable_error(err: &reqwest::Error) -> bool {
    err.is_timeout() || err.is_connect()
}

/// Calculates the delay for a given attempt using exponential backoff.
fn calculate_delay(config: &RetryConfig, attempt: u32) -> Duration {
    let multiplier = config.backoff_multiplier.pow(attempt) as u32;
    let delay = config.initial_delay.saturating_mul(multiplier);
    std::cmp::min(delay, config.max_delay)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_base_url_global() {
        let client = GraphClient::new(CloudEnvironment::Global);
        assert_eq!(client.base_url(), "https://graph.microsoft.com/v1.0");
    }

    #[test]
    fn test_base_url_china() {
        let client = GraphClient::new(CloudEnvironment::China);
        assert_eq!(
            client.base_url(),
            "https://microsoftgraph.chinacloudapi.cn/v1.0"
        );
    }

    #[test]
    fn test_calculate_delay_attempt_0() {
        let config = RetryConfig::default();
        let delay = calculate_delay(&config, 0);
        // 1s * 2^0 = 1s
        assert_eq!(delay, Duration::from_secs(1));
    }

    #[test]
    fn test_calculate_delay_attempt_1() {
        let config = RetryConfig::default();
        let delay = calculate_delay(&config, 1);
        // 1s * 2^1 = 2s
        assert_eq!(delay, Duration::from_secs(2));
    }

    #[test]
    fn test_calculate_delay_attempt_2() {
        let config = RetryConfig::default();
        let delay = calculate_delay(&config, 2);
        // 1s * 2^2 = 4s
        assert_eq!(delay, Duration::from_secs(4));
    }

    #[test]
    fn test_calculate_delay_capped_at_max() {
        let config = RetryConfig::default();
        // Very high attempt number should be capped at max_delay (30s)
        let delay = calculate_delay(&config, 10);
        assert_eq!(delay, Duration::from_secs(30));
    }

    #[test]
    fn test_retry_config_defaults() {
        let config = RetryConfig::default();
        assert_eq!(config.max_attempts, 3);
        assert_eq!(config.initial_delay, Duration::from_secs(1));
        assert_eq!(config.backoff_multiplier, 2);
        assert_eq!(config.max_delay, Duration::from_secs(30));
    }

    #[test]
    fn test_graph_client_cloud_env() {
        let client = GraphClient::new(CloudEnvironment::Global);
        assert_eq!(client.cloud_env(), &CloudEnvironment::Global);

        let client = GraphClient::new(CloudEnvironment::China);
        assert_eq!(client.cloud_env(), &CloudEnvironment::China);
    }
}
