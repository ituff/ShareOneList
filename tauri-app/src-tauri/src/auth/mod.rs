pub mod cloud_config;
pub mod commands;

use std::collections::HashMap;
use std::io::{BufRead, BufReader, Write};

use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use chrono::{DateTime, Duration, Utc};
use rand::Rng;
use sha2::{Digest, Sha256};
use socket2::{Domain, Protocol, Socket, Type};
use tokio::net::TcpListener;
use url::Url;

use crate::auth::cloud_config::{CloudConfig, CloudEnvironment};
use crate::errors::AppError;
use crate::models::AccountEntry;

// Default client IDs – these should be overridden from a config file in production.
// For now they are compile-time constants matching the existing appsettings.json structure.
const GLOBAL_CLIENT_ID: &str = "9e5165d3-7c32-4cf6-bb54-b444bc429ba8";
const CHINA_CLIENT_ID: &str = "edbc6b7c-e49c-42bd-8761-c0bc2386856f";

const KEYRING_SERVICE: &str = "shareonelist";

/// An active authentication session for a single cloud environment.
#[derive(Debug, Clone)]
pub struct AuthSession {
    pub cloud_env: CloudEnvironment,
    pub client_id: String,
    pub access_token: String,
    pub refresh_token: String,
    pub expires_at: DateTime<Utc>,
    pub home_account_id: String,
    pub display_name: String,
}

/// Manages OAuth2 sessions for all cloud environments.
#[derive(Debug)]
pub struct AuthModule {
    sessions: HashMap<(CloudEnvironment, String), AuthSession>,
    drive_to_account: HashMap<(CloudEnvironment, String), String>,
}

/// Token response from the OAuth2 token endpoint.
#[derive(Debug, serde::Deserialize)]
struct TokenResponse {
    access_token: String,
    refresh_token: Option<String>,
    expires_in: i64,
    #[serde(default)]
    id_token: Option<String>,
}

/// Minimal ID token claims we decode (without signature verification – we trust the token endpoint).
#[derive(Debug, serde::Deserialize)]
struct IdTokenClaims {
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    preferred_username: Option<String>,
    #[serde(default)]
    oid: Option<String>,
    #[serde(default)]
    sub: Option<String>,
}

/// Binds an ephemeral localhost listener that accepts both IPv4 and IPv6.
async fn bind_localhost_listener() -> std::io::Result<TcpListener> {
    if let Ok(socket) = Socket::new(Domain::IPV6, Type::STREAM, Some(Protocol::TCP)) {
        let dual_stack = socket.set_only_v6(false).is_ok();
        if dual_stack {
            let addr: std::net::SocketAddr = (std::net::Ipv6Addr::UNSPECIFIED, 0).into();
            if socket.bind(&addr.into()).is_ok() && socket.listen(128).is_ok() {
                let std_listener: std::net::TcpListener = socket.into();
                return TcpListener::from_std(std_listener);
            }
        }
    }

    TcpListener::bind("127.0.0.1:0").await
}

impl AuthModule {
    /// Creates a new AuthModule and attempts to restore sessions from the keyring.
    pub fn new() -> Self {
        Self {
            sessions: HashMap::new(),
            drive_to_account: HashMap::new(),
        }
    }

    /// Restore saved sessions from the platform keyring on startup.
    pub async fn restore_sessions(&mut self, accounts: Vec<AccountEntry>) {
        for account in accounts {
            let env = account.cloud_type;
            let key = (env.clone(), account.home_account_id.clone());
            if self.sessions.contains_key(&key) {
                continue;
            }
            match self
                .refresh_token_if_needed(&env, &account.home_account_id)
                .await
            {
                Ok(()) => {
                    if !account.drive_id.is_empty() {
                        self.drive_to_account.insert(
                            (env.clone(), account.drive_id.clone()),
                            account.home_account_id.clone(),
                        );
                    }
                    eprintln!(
                        "[auth] Restored {} session",
                        keyring_key(&env, &account.home_account_id)
                    );
                }
                Err(AppError::Auth { message, .. }) if message.contains("No active session") => {}
                Err(e) => eprintln!(
                    "[auth] Failed to restore {} session: {}",
                    keyring_key(&env, &account.home_account_id),
                    e
                ),
            }
        }
    }

    /// Returns the client ID for the given cloud environment.
    fn client_id_for(cloud_env: &CloudEnvironment) -> String {
        match cloud_env {
            CloudEnvironment::Global => GLOBAL_CLIENT_ID.to_string(),
            CloudEnvironment::China => CHINA_CLIENT_ID.to_string(),
        }
    }

    /// Returns the cloud configuration for the given environment.
    fn cloud_config(cloud_env: &CloudEnvironment) -> CloudConfig {
        let client_id = Self::client_id_for(cloud_env);
        cloud_env.config(&client_id)
    }

    /// Initiates the OAuth2 authorization code flow with PKCE.
    ///
    /// Opens the system browser for user authentication, listens on a localhost
    /// redirect URI for the callback, and exchanges the authorization code for tokens.
    pub async fn login(&mut self, cloud_env: CloudEnvironment) -> Result<AuthSession, AppError> {
        let config = Self::cloud_config(&cloud_env);

        // Generate PKCE code verifier and challenge
        let code_verifier = generate_code_verifier();
        let code_challenge = generate_code_challenge(&code_verifier);

        // Bind to a random available port for the redirect URI
        let listener = bind_localhost_listener()
            .await
            .map_err(|e| AppError::Auth {
                message: format!("Failed to bind localhost listener: {}", e),
                cloud_env: cloud_env.clone(),
            })?;
        let port = listener
            .local_addr()
            .map_err(|e| AppError::Auth {
                message: format!("Failed to get local address: {}", e),
                cloud_env: cloud_env.clone(),
            })?
            .port();

        let redirect_uri = format!("http://localhost:{}", port);
        let state = uuid::Uuid::new_v4().to_string();

        // Build authorization URL
        let scopes = config.scopes.join(" ") + " offline_access";
        let auth_url = format!(
            "{}/oauth2/v2.0/authorize?client_id={}&response_type=code&redirect_uri={}&scope={}&state={}&code_challenge={}&code_challenge_method=S256",
            config.authority,
            urlencoding::encode(&config.client_id),
            urlencoding::encode(&redirect_uri),
            urlencoding::encode(&scopes),
            urlencoding::encode(&state),
            urlencoding::encode(&code_challenge),
        );

        // Open the system browser
        open::that(&auth_url).map_err(|e| AppError::Auth {
            message: format!("Failed to open browser: {}", e),
            cloud_env: cloud_env.clone(),
        })?;

        // Wait for the OAuth2 callback
        let auth_code = wait_for_callback(listener, &state)
            .await
            .map_err(|e| AppError::Auth {
                message: e,
                cloud_env: cloud_env.clone(),
            })?;

        // Exchange the authorization code for tokens
        let token_response = exchange_code(&config, &auth_code, &redirect_uri, &code_verifier)
            .await
            .map_err(|e| AppError::Auth {
                message: e,
                cloud_env: cloud_env.clone(),
            })?;

        let expires_at = Utc::now() + Duration::seconds(token_response.expires_in);

        // Decode the ID token to get user info (name, oid)
        let claims = token_response
            .id_token
            .as_deref()
            .and_then(decode_id_token_claims);

        let display_name = claims
            .as_ref()
            .and_then(|c| c.name.clone().or_else(|| c.preferred_username.clone()))
            .unwrap_or_else(|| "Unknown User".to_string());

        let home_account_id = claims
            .as_ref()
            .and_then(|c| c.oid.clone().or_else(|| c.sub.clone()))
            .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());

        let refresh_token = token_response.refresh_token.unwrap_or_default();

        let session = AuthSession {
            cloud_env: cloud_env.clone(),
            client_id: config.client_id.clone(),
            access_token: token_response.access_token,
            refresh_token,
            expires_at,
            home_account_id: home_account_id.clone(),
            display_name,
        };

        self.sessions.insert(
            (cloud_env.clone(), home_account_id.clone()),
            session.clone(),
        );
        Ok(session)
    }

    /// Registers a completed login under its final account ID and drive ID.
    pub fn register_session(
        &mut self,
        cloud_env: &CloudEnvironment,
        session: AuthSession,
        home_account_id: &str,
        drive_id: &str,
    ) {
        let old_key = (cloud_env.clone(), session.home_account_id.clone());
        self.sessions.remove(&old_key);

        let new_key = (cloud_env.clone(), home_account_id.to_string());
        self.sessions.insert(new_key.clone(), session);

        if let Some(session) = self.sessions.get(&new_key) {
            if !session.refresh_token.is_empty() {
                let _ = store_refresh_token(cloud_env, home_account_id, &session.refresh_token);
            }
        }

        if !drive_id.is_empty() {
            self.drive_to_account.insert(
                (cloud_env.clone(), drive_id.to_string()),
                home_account_id.to_string(),
            );
        }
    }

    /// Logs out one account by clearing its keyring token and session.
    pub async fn logout(
        &mut self,
        cloud_env: CloudEnvironment,
        home_account_id: Option<&str>,
    ) -> Result<(), AppError> {
        match home_account_id {
            Some(id) => {
                delete_refresh_token(&cloud_env, id).ok();
                self.drive_to_account.retain(|(env, _), account_id| {
                    env != &cloud_env || account_id != id
                });
                self.sessions
                    .remove(&(cloud_env.clone(), id.to_string()));
            }
            None => {
                self.sessions.retain(|(env, _), _| env != &cloud_env);
                self.drive_to_account
                    .retain(|(env, _), _| env != &cloud_env);
            }
        }
        Ok(())
    }

    /// Returns the access token for the account that owns a drive.
    pub async fn get_token_for_drive(
        &mut self,
        cloud_env: CloudEnvironment,
        drive_id: &str,
    ) -> Result<String, AppError> {
        let home_account_id = self
            .drive_to_account
            .get(&(cloud_env.clone(), drive_id.to_string()))
            .cloned()
            .ok_or_else(|| AppError::Auth {
                message: "No active session for this drive. Please login first.".to_string(),
                cloud_env: cloud_env.clone(),
            })?;
        self.get_token_for_account(cloud_env, &home_account_id).await
    }

    /// Returns the access token for a specific account.
    pub async fn get_token_for_account(
        &mut self,
        cloud_env: CloudEnvironment,
        home_account_id: &str,
    ) -> Result<String, AppError> {
        self.refresh_token_if_needed(&cloud_env, home_account_id)
            .await?;

        let key = (cloud_env.clone(), home_account_id.to_string());
        let session = self.sessions.get(&key).ok_or_else(|| AppError::Auth {
            message: "No active session for this account. Please login first.".to_string(),
            cloud_env: cloud_env.clone(),
        })?;

        Ok(session.access_token.clone())
    }

    /// Fallback token lookup for commands without a drive context.
    pub async fn get_token(&mut self, cloud_env: CloudEnvironment) -> Result<String, AppError> {
        let home_account_id = self
            .sessions
            .keys()
            .find(|(env, _)| env == &cloud_env)
            .map(|(_, id)| id.clone())
            .ok_or_else(|| AppError::Auth {
                message: "No active session. Please login first.".to_string(),
                cloud_env: cloud_env.clone(),
            })?;
        self.get_token_for_account(cloud_env, &home_account_id)
            .await
    }

    /// Checks if the access token is about to expire (≤5 min remaining)
    /// and refreshes it using the account-specific refresh token.
    pub async fn refresh_token_if_needed(
        &mut self,
        cloud_env: &CloudEnvironment,
        home_account_id: &str,
    ) -> Result<(), AppError> {
        let key = (cloud_env.clone(), home_account_id.to_string());
        let needs_refresh = self
            .sessions
            .get(&key)
            .map(|session| session.expires_at - Utc::now() <= Duration::minutes(5))
            .unwrap_or(true);

        if !needs_refresh {
            return Ok(());
        }

        let refresh_token = self
            .sessions
            .get(&key)
            .map(|s| s.refresh_token.clone())
            .or_else(|| load_refresh_token(cloud_env, home_account_id))
            .ok_or_else(|| AppError::Auth {
                message: "No active session for this account. Please login first.".to_string(),
                cloud_env: cloud_env.clone(),
            })?;

        let config = Self::cloud_config(cloud_env);
        match refresh_access_token(&config, &refresh_token).await {
            Ok(token_response) => {
                let expires_at = Utc::now() + Duration::seconds(token_response.expires_in);
                let new_refresh = token_response
                    .refresh_token
                    .unwrap_or_else(|| refresh_token.clone());

                if !new_refresh.is_empty() {
                    if let Err(e) = store_refresh_token(cloud_env, home_account_id, &new_refresh) {
                        eprintln!("[auth] Failed to store refreshed token: {}", e);
                    }
                }

                let session = self.sessions.entry(key).or_insert_with(|| AuthSession {
                    cloud_env: cloud_env.clone(),
                    client_id: config.client_id.clone(),
                    access_token: String::new(),
                    refresh_token: new_refresh.clone(),
                    expires_at,
                    home_account_id: home_account_id.to_string(),
                    display_name: "Unknown User".to_string(),
                });
                session.access_token = token_response.access_token;
                session.refresh_token = new_refresh;
                session.expires_at = expires_at;
                session.client_id = config.client_id.clone();
                Ok(())
            }
            Err(e) => {
                self.sessions.remove(&key);
                Err(AppError::Auth {
                    message: format!("Token expired. Please re-login. ({})", e),
                    cloud_env: cloud_env.clone(),
                })
            }
        }
    }

    /// Returns true if there is a session for the given account.
    pub fn has_session(&self, cloud_env: &CloudEnvironment, home_account_id: &str) -> bool {
        self.sessions
            .contains_key(&(cloud_env.clone(), home_account_id.to_string()))
    }

    /// Returns the session for the given account, if any.
    pub fn get_session(
        &self,
        cloud_env: &CloudEnvironment,
        home_account_id: &str,
    ) -> Option<&AuthSession> {
        self.sessions
            .get(&(cloud_env.clone(), home_account_id.to_string()))
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// PKCE helpers
// ─────────────────────────────────────────────────────────────────────────────

/// Generates a cryptographically random code verifier (43–128 chars, unreserved characters).
fn generate_code_verifier() -> String {
    let mut rng = rand::thread_rng();
    let length = rng.gen_range(43..=128);
    let charset: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-._~";
    (0..length)
        .map(|_| {
            let idx = rng.gen_range(0..charset.len());
            charset[idx] as char
        })
        .collect()
}

/// Computes the PKCE code challenge: base64url(sha256(code_verifier)).
fn generate_code_challenge(verifier: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(verifier.as_bytes());
    let hash = hasher.finalize();
    URL_SAFE_NO_PAD.encode(hash)
}

// ─────────────────────────────────────────────────────────────────────────────
// OAuth2 callback listener
// ─────────────────────────────────────────────────────────────────────────────

/// Waits for the OAuth2 redirect callback on the localhost TCP listener.
/// Returns the authorization code if the state matches.
async fn wait_for_callback(listener: TcpListener, expected_state: &str) -> Result<String, String> {
    // Accept a single connection with a timeout
    let accept_future = listener.accept();
    let result = tokio::time::timeout(std::time::Duration::from_secs(300), accept_future)
        .await
        .map_err(|_| "Login timed out (5 minutes). Please try again.".to_string())?
        .map_err(|e| format!("Failed to accept connection: {}", e))?;

    let (stream, _) = result;
    let std_stream = stream
        .into_std()
        .map_err(|e| format!("Stream conversion failed: {}", e))?;

    // Read the HTTP request
    let mut reader = BufReader::new(std_stream.try_clone().map_err(|e| e.to_string())?);
    let mut request_line = String::new();
    reader
        .read_line(&mut request_line)
        .map_err(|e| format!("Failed to read request: {}", e))?;

    // Parse the request to extract query parameters
    // Expected format: GET /?code=...&state=... HTTP/1.1
    let path = request_line
        .split_whitespace()
        .nth(1)
        .ok_or_else(|| "Invalid HTTP request".to_string())?;

    let full_url = format!("http://localhost{}", path);
    let parsed =
        Url::parse(&full_url).map_err(|e| format!("Failed to parse callback URL: {}", e))?;

    let params: HashMap<String, String> = parsed.query_pairs().into_owned().collect();

    // Check for error response
    if let Some(error) = params.get("error") {
        let description = params
            .get("error_description")
            .cloned()
            .unwrap_or_else(|| error.clone());

        // Send error response to browser
        let response_body = format!(
            "<html><body><h2>Authentication Failed</h2><p>{}</p><p>You can close this window.</p></body></html>",
            description
        );
        let response = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            response_body.len(),
            response_body
        );
        let mut writer = std_stream;
        let _ = writer.write_all(response.as_bytes());
        let _ = writer.flush();

        return Err(format!("Authorization failed: {}", description));
    }

    // Validate state
    let state = params
        .get("state")
        .ok_or_else(|| "Missing state parameter in callback".to_string())?;

    if state != expected_state {
        return Err("State mismatch in OAuth callback. Possible CSRF attack.".to_string());
    }

    // Extract the authorization code
    let code = params
        .get("code")
        .ok_or_else(|| "Missing authorization code in callback".to_string())?
        .clone();

    // Send success response to the browser
    let response_body =
        "<html><body><h2>Login Successful!</h2><p>You can close this window and return to ShareOneList.</p></body></html>";
    let response = format!(
        "HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        response_body.len(),
        response_body
    );
    let mut writer = std_stream.try_clone().map_err(|e| e.to_string())?;
    let _ = writer.write_all(response.as_bytes());
    let _ = writer.flush();

    Ok(code)
}

// ─────────────────────────────────────────────────────────────────────────────
// Token exchange
// ─────────────────────────────────────────────────────────────────────────────

/// Exchanges the authorization code for access and refresh tokens.
async fn exchange_code(
    config: &CloudConfig,
    code: &str,
    redirect_uri: &str,
    code_verifier: &str,
) -> Result<TokenResponse, String> {
    let token_url = format!("{}/oauth2/v2.0/token", config.authority);
    let scopes = config.scopes.join(" ") + " offline_access";

    let params = [
        ("client_id", config.client_id.as_str()),
        ("grant_type", "authorization_code"),
        ("code", code),
        ("redirect_uri", redirect_uri),
        ("code_verifier", code_verifier),
        ("scope", &scopes),
    ];

    let client = reqwest::Client::new();
    let response = client
        .post(&token_url)
        .form(&params)
        .send()
        .await
        .map_err(|e| format!("Token request failed: {}", e))?;

    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        return Err(format!("Token endpoint returned {}: {}", status, body));
    }

    response
        .json::<TokenResponse>()
        .await
        .map_err(|e| format!("Failed to parse token response: {}", e))
}

/// Refreshes the access token using a refresh token.
async fn refresh_access_token(
    config: &CloudConfig,
    refresh_token: &str,
) -> Result<TokenResponse, String> {
    let token_url = format!("{}/oauth2/v2.0/token", config.authority);
    let scopes = config.scopes.join(" ") + " offline_access";

    let params = [
        ("client_id", config.client_id.as_str()),
        ("grant_type", "refresh_token"),
        ("refresh_token", refresh_token),
        ("scope", &scopes),
    ];

    let client = reqwest::Client::new();
    let response = client
        .post(&token_url)
        .form(&params)
        .send()
        .await
        .map_err(|e| format!("Token refresh request failed: {}", e))?;

    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        return Err(format!(
            "Token refresh endpoint returned {}: {}",
            status, body
        ));
    }

    response
        .json::<TokenResponse>()
        .await
        .map_err(|e| format!("Failed to parse token refresh response: {}", e))
}

// ─────────────────────────────────────────────────────────────────────────────
// ID Token decoding (claims only, no signature verification)
// ─────────────────────────────────────────────────────────────────────────────

/// Decodes the payload of a JWT ID token to extract user claims.
/// Does NOT verify the signature since we trust the token endpoint.
fn decode_id_token_claims(id_token: &str) -> Option<IdTokenClaims> {
    let parts: Vec<&str> = id_token.split('.').collect();
    if parts.len() != 3 {
        return None;
    }

    // Decode the payload (second part)
    let payload_bytes = URL_SAFE_NO_PAD.decode(parts[1]).ok()?;
    serde_json::from_slice::<IdTokenClaims>(&payload_bytes).ok()
}

// ─────────────────────────────────────────────────────────────────────────────
// Keyring helpers (secure token storage)
// ─────────────────────────────────────────────────────────────────────────────

/// Keyring key for the refresh token of a given account and cloud environment.
fn keyring_key(cloud_env: &CloudEnvironment, home_account_id: &str) -> String {
    let env = match cloud_env {
        CloudEnvironment::Global => "global",
        CloudEnvironment::China => "china",
    };
    format!("refresh_token_{}_{}", home_account_id, env)
}

/// Stores a refresh token in the platform keyring.
fn store_refresh_token(
    cloud_env: &CloudEnvironment,
    home_account_id: &str,
    token: &str,
) -> Result<(), String> {
    let entry = keyring::Entry::new(
        KEYRING_SERVICE,
        &keyring_key(cloud_env, home_account_id),
    )
        .map_err(|e| format!("Keyring entry creation failed: {}", e))?;
    entry
        .set_password(token)
        .map_err(|e| format!("Keyring set_password failed: {}", e))
}

/// Loads a refresh token from the platform keyring.
fn load_refresh_token(cloud_env: &CloudEnvironment, home_account_id: &str) -> Option<String> {
    let entry =
        keyring::Entry::new(KEYRING_SERVICE, &keyring_key(cloud_env, home_account_id)).ok()?;
    entry.get_password().ok()
}

/// Deletes a refresh token from the platform keyring.
fn delete_refresh_token(cloud_env: &CloudEnvironment, home_account_id: &str) -> Result<(), String> {
    let entry = keyring::Entry::new(
        KEYRING_SERVICE,
        &keyring_key(cloud_env, home_account_id),
    )
        .map_err(|e| format!("Keyring entry creation failed: {}", e))?;
    entry
        .delete_credential()
        .map_err(|e| format!("Keyring delete failed: {}", e))
}

// ─────────────────────────────────────────────────────────────────────────────
// URL encoding helper (minimal, for use in auth URL construction)
// ─────────────────────────────────────────────────────────────────────────────

mod urlencoding {
    /// Percent-encodes a string for use in URL query parameters.
    pub fn encode(input: &str) -> String {
        let mut encoded = String::with_capacity(input.len() * 3);
        for byte in input.bytes() {
            match byte {
                b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                    encoded.push(byte as char);
                }
                _ => {
                    encoded.push_str(&format!("%{:02X}", byte));
                }
            }
        }
        encoded
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Tests
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_code_verifier_length() {
        let verifier = generate_code_verifier();
        assert!(verifier.len() >= 43 && verifier.len() <= 128);
    }

    #[test]
    fn test_code_verifier_characters() {
        let verifier = generate_code_verifier();
        let valid_chars: &str =
            "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-._~";
        for ch in verifier.chars() {
            assert!(
                valid_chars.contains(ch),
                "Invalid character '{}' in code verifier",
                ch
            );
        }
    }

    #[test]
    fn test_code_challenge_is_base64url_sha256() {
        let verifier = "dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk";
        let challenge = generate_code_challenge(verifier);
        // Known SHA256 of the above verifier, base64url-encoded without padding
        let mut hasher = Sha256::new();
        hasher.update(verifier.as_bytes());
        let expected = URL_SAFE_NO_PAD.encode(hasher.finalize());
        assert_eq!(challenge, expected);
    }

    #[test]
    fn test_urlencoding_basic() {
        assert_eq!(urlencoding::encode("hello"), "hello");
        assert_eq!(urlencoding::encode("hello world"), "hello%20world");
        assert_eq!(urlencoding::encode("a+b=c"), "a%2Bb%3Dc");
    }

    #[test]
    fn test_keyring_key_naming() {
        assert_eq!(
            keyring_key(&CloudEnvironment::Global, "account-1"),
            "refresh_token_account-1_global"
        );
        assert_eq!(
            keyring_key(&CloudEnvironment::China, "account-2"),
            "refresh_token_account-2_china"
        );
    }

    #[test]
    fn test_decode_id_token_claims_valid() {
        // Construct a minimal JWT with a known payload
        let payload = serde_json::json!({
            "name": "Test User",
            "oid": "12345-67890",
            "preferred_username": "test@example.com"
        });
        let payload_b64 = URL_SAFE_NO_PAD.encode(payload.to_string().as_bytes());
        let fake_jwt = format!("eyJhbGciOiJSUzI1NiJ9.{}.fake_signature", payload_b64);

        let claims = decode_id_token_claims(&fake_jwt).unwrap();
        assert_eq!(claims.name, Some("Test User".to_string()));
        assert_eq!(claims.oid, Some("12345-67890".to_string()));
        assert_eq!(
            claims.preferred_username,
            Some("test@example.com".to_string())
        );
    }

    #[test]
    fn test_decode_id_token_claims_invalid() {
        assert!(decode_id_token_claims("not.a.jwt.token").is_none());
        assert!(decode_id_token_claims("").is_none());
        assert!(decode_id_token_claims("only_one_part").is_none());
    }

    #[test]
    fn test_auth_module_new() {
        let module = AuthModule::new();
        assert!(!module.has_session(&CloudEnvironment::Global, "account-1"));
        assert!(!module.has_session(&CloudEnvironment::China, "account-2"));
    }

    #[test]
    fn test_client_id_for_environments() {
        let global_id = AuthModule::client_id_for(&CloudEnvironment::Global);
        let china_id = AuthModule::client_id_for(&CloudEnvironment::China);
        assert!(!global_id.is_empty());
        assert!(!china_id.is_empty());
        assert_ne!(global_id, china_id);
    }

    #[test]
    fn test_cloud_config_endpoints() {
        let global_config = AuthModule::cloud_config(&CloudEnvironment::Global);
        assert!(global_config
            .authority
            .contains("login.microsoftonline.com"));
        assert!(global_config.graph_base_url.contains("graph.microsoft.com"));

        let china_config = AuthModule::cloud_config(&CloudEnvironment::China);
        assert!(china_config
            .authority
            .contains("login.partner.microsoftonline.cn"));
        assert!(china_config
            .graph_base_url
            .contains("microsoftgraph.chinacloudapi.cn"));
    }
}
