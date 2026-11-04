// Backup encryption: password-derived AES-256-GCM for the secret-bearing
// backup sections. The derived key is cached in the OS keyring so automatic
// backups can encrypt without prompting; restoring on another machine only
// needs the password (salt travels inside the backup file).

use aes_gcm::aead::{Aead, KeyInit, Payload};
use aes_gcm::{Aes256Gcm, Nonce};
use base64::engine::general_purpose::STANDARD as B64;
use base64::Engine;
use rand::RngCore;
use serde::{Deserialize, Serialize};

const BACKUP_KEY_ENTRY: &str = "backup_enc_key";
const KEYRING_SERVICE: &str = "shareonelist"; // matches auth/llm keyring usage

pub const SALT_LEN: usize = 16;
pub const KEY_LEN: usize = 32;
pub const NONCE_LEN: usize = 12;

/// Argon2id parameters — OWASP-recommended defaults (19 MiB, t=2, p=1).
const ARGON_MEMORY_KIB: u32 = 19_456;
const ARGON_TIME_COST: u32 = 2;
const ARGON_PARALLELISM: u32 = 1;

/// An error while encrypting/decrypting a backup payload.
#[derive(Debug, thiserror::Error)]
pub enum BackupCryptoError {
    #[error("wrong password or corrupted backup")]
    Decrypt,
    #[error("keyring error: {0}")]
    Keyring(String),
}

/// Algorithm metadata stored alongside the ciphertext.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct KdfParams {
    pub memory_kib: u32,
    pub time_cost: u32,
    pub parallelism: u32,
}

impl Default for KdfParams {
    fn default() -> Self {
        Self {
            memory_kib: ARGON_MEMORY_KIB,
            time_cost: ARGON_TIME_COST,
            parallelism: ARGON_PARALLELISM,
        }
    }
}

/// The `enc` object embedded in a version-2 backup payload. Salt and nonce
/// travel with the file so the key can be re-derived from the password.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct EncryptedSection {
    pub alg: String,
    pub kdf: String,
    pub kdf_params: KdfParams,
    pub salt: String,
    pub nonce: String,
    pub data: String,
}

/// Derive the AES-256 key from a password and the file's salt.
pub fn derive_key(password: &str, salt: &[u8]) -> Result<[u8; KEY_LEN], BackupCryptoError> {
    let params = argon2::Params::new(
        ARGON_MEMORY_KIB,
        ARGON_TIME_COST,
        ARGON_PARALLELISM,
        Some(KEY_LEN),
    )
    .map_err(|_| BackupCryptoError::Decrypt)?;
    let argon = argon2::Argon2::new(
        argon2::Algorithm::Argon2id,
        argon2::Version::V0x13,
        params,
    );
    let mut key = [0u8; KEY_LEN];
    argon
        .hash_password_into(password.as_bytes(), salt, &mut key)
        .map_err(|_| BackupCryptoError::Decrypt)?;
    Ok(key)
}

/// Encrypt a JSON payload section into a self-describing `enc` object.
pub fn encrypt_sections(
    key: &[u8; KEY_LEN],
    salt: &[u8],
    plaintext: &serde_json::Value,
) -> Result<EncryptedSection, BackupCryptoError> {
    let mut nonce_bytes = [0u8; NONCE_LEN];
    rand::thread_rng().fill_bytes(&mut nonce_bytes);

    let cipher = Aes256Gcm::new(key.into());
    let body = serde_json::to_vec(plaintext).map_err(|_| BackupCryptoError::Decrypt)?;
    let payload = Payload {
        msg: body.as_slice(),
        aad: b"ShareOneList-backup-v2",
    };
    let ciphertext = cipher
        .encrypt(Nonce::from_slice(&nonce_bytes), payload)
        .map_err(|_| BackupCryptoError::Decrypt)?;

    Ok(EncryptedSection {
        alg: "AES-256-GCM".to_string(),
        kdf: "argon2id".to_string(),
        kdf_params: KdfParams::default(),
        salt: B64.encode(salt),
        nonce: B64.encode(nonce_bytes),
        data: B64.encode(ciphertext),
    })
}

/// Decrypt and parse the `enc` object back into the JSON sections map.
pub fn decrypt_sections(
    password: &str,
    section: &EncryptedSection,
) -> Result<serde_json::Value, BackupCryptoError> {
    let salt = B64.decode(&section.salt).map_err(|_| BackupCryptoError::Decrypt)?;
    let key = derive_key_with_params(password, &salt, &section.kdf_params)?;
    let nonce_bytes: [u8; NONCE_LEN] = B64
        .decode(&section.nonce)
        .map_err(|_| BackupCryptoError::Decrypt)?
        .try_into()
        .map_err(|_| BackupCryptoError::Decrypt)?;

    let cipher = Aes256Gcm::new((&key).into());
    let payload = Payload {
        msg: &B64.decode(&section.data).map_err(|_| BackupCryptoError::Decrypt)?,
        aad: b"ShareOneList-backup-v2",
    };
    let plaintext = cipher
        .decrypt(Nonce::from_slice(&nonce_bytes), payload)
        .map_err(|_| BackupCryptoError::Decrypt)?;
    serde_json::from_slice(&plaintext).map_err(|_| BackupCryptoError::Decrypt)
}

/// Derive a key honoring the params recorded in the backup file (forward
/// compatibility if the defaults ever change).
fn derive_key_with_params(
    password: &str,
    salt: &[u8],
    params: &KdfParams,
) -> Result<[u8; KEY_LEN], BackupCryptoError> {
    let argon_params = argon2::Params::new(
        params.memory_kib,
        params.time_cost,
        params.parallelism,
        Some(KEY_LEN),
    )
    .map_err(|_| BackupCryptoError::Decrypt)?;
    let argon = argon2::Argon2::new(
        argon2::Algorithm::Argon2id,
        argon2::Version::V0x13,
        argon_params,
    );
    let mut key = [0u8; KEY_LEN];
    argon
        .hash_password_into(password.as_bytes(), salt, &mut key)
        .map_err(|_| BackupCryptoError::Decrypt)?;
    Ok(key)
}

/// The locally cached backup key: derived once from the user's password and
/// kept in the OS keyring so automatic backups never need a prompt.
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct StoredKey {
    salt: String,
    key: String,
}

/// Generate a fresh salt, derive the key from `password`, and store both in
/// the keyring. Returns the salt for callers that want to verify the setup.
pub fn set_backup_password(password: &str) -> Result<[u8; SALT_LEN], BackupCryptoError> {
    let mut salt = [0u8; SALT_LEN];
    rand::thread_rng().fill_bytes(&mut salt);
    let key = derive_key(password, &salt)?;
    let stored = StoredKey {
        salt: B64.encode(salt),
        key: B64.encode(key),
    };
    let entry = keyring::Entry::new(KEYRING_SERVICE, BACKUP_KEY_ENTRY)
        .map_err(|e| BackupCryptoError::Keyring(e.to_string()))?;
    let json = serde_json::to_string(&stored)
        .map_err(|_| BackupCryptoError::Keyring("serialize failed".to_string()))?;
    entry
        .set_password(&json)
        .map_err(|e| BackupCryptoError::Keyring(e.to_string()))?;
    Ok(salt)
}

/// Whether a backup password has been set on this machine.
pub fn has_backup_key() -> bool {
    load_stored_key().is_some()
}

/// Load the cached (salt, key) for silent encryption during automatic
/// backups. `None` when no password was set yet.
pub fn load_backup_key() -> Option<([u8; KEY_LEN], [u8; SALT_LEN])> {
    let stored = load_stored_key()?;
    let salt: [u8; SALT_LEN] = B64.decode(stored.salt).ok()?.try_into().ok()?;
    let key: [u8; KEY_LEN] = B64.decode(stored.key).ok()?.try_into().ok()?;
    Some((key, salt))
}

fn load_stored_key() -> Option<StoredKey> {
    let entry = keyring::Entry::new(KEYRING_SERVICE, BACKUP_KEY_ENTRY).ok()?;
    let json = entry.get_password().ok()?;
    serde_json::from_str(&json).ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn encrypt_decrypt_roundtrip() {
        let salt = [7u8; SALT_LEN];
        let key = derive_key("correct horse battery staple", &salt).unwrap();
        let sections = json!({
            "tokens": [{"homeAccountId": "acc", "cloudEnv": "global", "refreshToken": "0.abc#def"}],
            "llmApiKeys": {"prov-1": "sk-secret"},
        });

        let enc = encrypt_sections(&key, &salt, &sections).unwrap();
        // Nothing readable survives into the encoded section.
        assert!(!enc.data.contains("sk-secret"));
        assert_eq!(enc.alg, "AES-256-GCM");
        assert_eq!(enc.kdf, "argon2id");

        let decrypted = decrypt_sections("correct horse battery staple", &enc).unwrap();
        assert_eq!(decrypted, sections);
    }

    #[test]
    fn wrong_password_fails() {
        let salt = [1u8; SALT_LEN];
        let key = derive_key("right", &salt).unwrap();
        let enc = encrypt_sections(&key, &salt, &json!({"a": 1})).unwrap();
        assert!(matches!(
            decrypt_sections("wrong", &enc),
            Err(BackupCryptoError::Decrypt)
        ));
    }

    #[test]
    fn tampered_ciphertext_fails() {
        let salt = [2u8; SALT_LEN];
        let key = derive_key("pw", &salt).unwrap();
        let mut enc = encrypt_sections(&key, &salt, &json!({"a": 1})).unwrap();
        // Flip one byte in the middle of the base64 data.
        let mut data = B64.decode(&enc.data).unwrap();
        let mid = data.len() / 2;
        data[mid] ^= 0xff;
        enc.data = B64.encode(data);
        assert!(decrypt_sections("pw", &enc).is_err());
    }

    #[test]
    fn key_derivation_is_deterministic_and_salt_sensitive() {
        let salt_a = [3u8; SALT_LEN];
        let salt_b = [4u8; SALT_LEN];
        assert_eq!(
            derive_key("pw", &salt_a).unwrap(),
            derive_key("pw", &salt_a).unwrap()
        );
        assert_ne!(
            derive_key("pw", &salt_a).unwrap(),
            derive_key("pw", &salt_b).unwrap()
        );
        assert_ne!(
            derive_key("pw", &salt_a).unwrap(),
            derive_key("other", &salt_a).unwrap()
        );
    }

    #[test]
    fn kdf_params_are_honored() {
        let salt = [5u8; SALT_LEN];
        let sections = json!({"a": 1});
        let key = derive_key("pw", &salt).unwrap();
        let mut enc = encrypt_sections(&key, &salt, &sections).unwrap();
        // Higher cost with the same password still decrypts (params travel
        // in the file), while claiming different params breaks decryption.
        enc.kdf_params.time_cost += 1;
        assert!(decrypt_sections("pw", &enc).is_err());
        enc.kdf_params.time_cost -= 1;
        assert_eq!(decrypt_sections("pw", &enc).unwrap(), sections);
    }
}
