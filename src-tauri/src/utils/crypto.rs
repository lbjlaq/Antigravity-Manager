use aes_gcm::{
    aead::{Aead, KeyInit},
    Aes256Gcm, Nonce,
};
use base64::{engine::general_purpose, Engine as _};
use rand::{rngs::OsRng, RngCore};
use serde::{Deserialize, Deserializer, Serializer};
use sha2::Digest;

const LEGACY_FIXED_NONCE: &[u8; 12] = b"antigravsalt";
const ENCRYPTED_PREFIX: &str = "ag_enc_";
const ENCRYPTED_V2_PREFIX: &str = "ag_enc_v2_";

fn get_encryption_key() -> [u8; 32] {
    #[cfg(not(target_os = "android"))]
    let device_id = machine_uid::get().unwrap_or_else(|_| "default".to_string());

    #[cfg(target_os = "android")]
    let device_id = get_android_device_id();

    let mut key = [0u8; 32];
    let hash = sha2::Sha256::digest(device_id.as_bytes());
    key.copy_from_slice(&hash);
    key
}

#[cfg(target_os = "android")]
fn get_android_device_id() -> String {
    if let Ok(dir) = crate::modules::account::get_data_dir() {
        let id_file = dir.join("device_id");
        if let Ok(content) = std::fs::read_to_string(&id_file) {
            let trimmed = content.trim();
            if !trimmed.is_empty() {
                return trimmed.to_string();
            }
        }
        let new_id = uuid::Uuid::new_v4().to_string();
        let _ = std::fs::write(&id_file, &new_id);
        new_id
    } else {
        "android_termux_default".to_string()
    }
}

pub fn serialize_password<S>(password: &str, serializer: S) -> Result<S::Ok, S::Error>
where
    S: Serializer,
{
    if password.starts_with(ENCRYPTED_PREFIX) || password.starts_with(ENCRYPTED_V2_PREFIX) {
        return serializer.serialize_str(password);
    }

    let encrypted = encrypt_string(password).map_err(serde::ser::Error::custom)?;
    serializer.serialize_str(&encrypted)
}

pub fn deserialize_password<'de, D>(deserializer: D) -> Result<String, D::Error>
where
    D: Deserializer<'de>,
{
    let raw = String::deserialize(deserializer)?;
    if raw.is_empty() {
        return Ok(raw);
    }

    if raw.starts_with(ENCRYPTED_V2_PREFIX) {
        match decrypt_string_v2(&raw[ENCRYPTED_V2_PREFIX.len()..]) {
            Ok(plaintext) => Ok(plaintext),
            Err(_) => {
                tracing::warn!(
                    "password decryption failed, key likely changed (machine-id differs from when it was encrypted)"
                );
                Ok(raw)
            }
        }
    } else if raw.starts_with(ENCRYPTED_PREFIX) {
        match decrypt_legacy(&raw[ENCRYPTED_PREFIX.len()..]) {
            Ok(plaintext) => Ok(plaintext),
            Err(_) => {
                tracing::warn!(
                    "password decryption failed, key likely changed (machine-id differs from when it was encrypted)"
                );
                Ok(raw)
            }
        }
    } else {
        match decrypt_legacy(&raw) {
            Ok(plaintext) => Ok(plaintext),
            Err(_) => Ok(raw),
        }
    }
}

pub fn encrypt_string(password: &str) -> Result<String, String> {
    let key = get_encryption_key();
    let cipher = Aes256Gcm::new(&key.into());

    let mut nonce_bytes = [0u8; 12];
    OsRng.fill_bytes(&mut nonce_bytes);
    let nonce = Nonce::from_slice(&nonce_bytes);

    let ciphertext = cipher
        .encrypt(nonce, password.as_bytes())
        .map_err(|e| format!("Encryption failed: {}", e))?;

    let encoded_nonce = general_purpose::STANDARD_NO_PAD.encode(nonce_bytes);
    let encoded_ciphertext = general_purpose::STANDARD_NO_PAD.encode(ciphertext);
    Ok(format!(
        "{}{}.{}",
        ENCRYPTED_V2_PREFIX, encoded_nonce, encoded_ciphertext
    ))
}

fn decrypt_legacy(encrypted_base64: &str) -> Result<String, String> {
    let key = get_encryption_key();
    let cipher = Aes256Gcm::new(&key.into());
    let nonce = Nonce::from_slice(LEGACY_FIXED_NONCE);

    let ciphertext = general_purpose::STANDARD
        .decode(encrypted_base64)
        .map_err(|e| format!("Base64 decode failed: {}", e))?;

    let plaintext = cipher
        .decrypt(nonce, ciphertext.as_ref())
        .map_err(|e| format!("Decryption failed: {}", e))?;

    String::from_utf8(plaintext).map_err(|e| format!("UTF-8 conversion failed: {}", e))
}

fn decrypt_string_v2(encrypted: &str) -> Result<String, String> {
    let (nonce_base64, ciphertext_base64) = encrypted
        .split_once('.')
        .ok_or_else(|| "Invalid encrypted payload".to_string())?;

    let nonce_bytes = general_purpose::STANDARD_NO_PAD
        .decode(nonce_base64)
        .map_err(|e| format!("Nonce decode failed: {}", e))?;
    if nonce_bytes.len() != 12 {
        return Err("Invalid nonce length".to_string());
    }

    let ciphertext = general_purpose::STANDARD_NO_PAD
        .decode(ciphertext_base64)
        .map_err(|e| format!("Ciphertext decode failed: {}", e))?;

    let key = get_encryption_key();
    let cipher = Aes256Gcm::new(&key.into());
    let plaintext = cipher
        .decrypt(Nonce::from_slice(&nonce_bytes), ciphertext.as_ref())
        .map_err(|e| format!("Decryption failed: {}", e))?;

    String::from_utf8(plaintext).map_err(|e| format!("UTF-8 conversion failed: {}", e))
}

pub fn decrypt_string(encrypted: &str) -> Result<String, String> {
    if encrypted.starts_with(ENCRYPTED_V2_PREFIX) {
        decrypt_string_v2(&encrypted[ENCRYPTED_V2_PREFIX.len()..])
    } else if encrypted.starts_with(ENCRYPTED_PREFIX) {
        decrypt_legacy(&encrypted[ENCRYPTED_PREFIX.len()..])
    } else {
        decrypt_legacy(encrypted)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_encrypt_decrypt_cycle() {
        let password = "my_secret_password";
        let encrypted = encrypt_string(password).unwrap();

        assert!(encrypted.starts_with(ENCRYPTED_V2_PREFIX));
        assert_ne!(password, encrypted);

        let decrypted = decrypt_string(&encrypted).unwrap();
        assert_eq!(password, decrypted);
    }

    #[test]
    fn test_encrypt_uses_unique_nonce() {
        let password = "my_secret_password";
        let encrypted_a = encrypt_string(password).unwrap();
        let encrypted_b = encrypt_string(password).unwrap();

        assert_ne!(encrypted_a, encrypted_b);
        assert_eq!(decrypt_string(&encrypted_a).unwrap(), password);
        assert_eq!(decrypt_string(&encrypted_b).unwrap(), password);
    }

    #[test]
    fn test_legacy_compatibility() {
        let password = "legacy_password";
        let key = get_encryption_key();
        let cipher = Aes256Gcm::new(&key.into());
        let nonce = Nonce::from_slice(LEGACY_FIXED_NONCE);
        let ciphertext = cipher.encrypt(nonce, password.as_bytes()).unwrap();
        let legacy_encrypted = general_purpose::STANDARD.encode(ciphertext);

        assert!(!legacy_encrypted.starts_with(ENCRYPTED_PREFIX));

        let decrypted = decrypt_string(&legacy_encrypted).unwrap();
        assert_eq!(password, decrypted);
    }

    #[derive(Deserialize)]
    struct PasswordContainer {
        #[serde(deserialize_with = "deserialize_password")]
        password: String,
    }

    #[test]
    fn test_deserialize_password_plaintext() {
        let json = r#"{"password": "plain_password_123"}"#;
        let parsed: PasswordContainer = serde_json::from_str(json).unwrap();
        assert_eq!(parsed.password, "plain_password_123");
    }

    #[test]
    fn test_deserialize_password_valid_encrypted() {
        let encrypted = encrypt_string("secret_pass_456").unwrap();
        let json = format!(r#"{{"password": "{}"}}"#, encrypted);
        let parsed: PasswordContainer = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.password, "secret_pass_456");
    }

    #[test]
    fn test_deserialize_password_failed_decryption_returns_raw() {
        // Corrupted v2 payload: should return raw string as fallback
        let corrupted_v2 = "ag_enc_v2_invalidnonce.invalidciphertext";
        let json_v2 = format!(r#"{{"password": "{}"}}"#, corrupted_v2);
        let parsed_v2: PasswordContainer = serde_json::from_str(&json_v2).unwrap();
        assert_eq!(parsed_v2.password, corrupted_v2);

        // Corrupted legacy payload with ag_enc_ prefix: should return raw string as fallback
        let corrupted_legacy = "ag_enc_invalidbase64content==";
        let json_legacy = format!(r#"{{"password": "{}"}}"#, corrupted_legacy);
        let parsed_legacy: PasswordContainer = serde_json::from_str(&json_legacy).unwrap();
        assert_eq!(parsed_legacy.password, corrupted_legacy);
    }
}
