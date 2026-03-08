use aes_gcm::{
    aead::{Aead, KeyInit},
    Aes256Gcm, Nonce,
};
use base64::{engine::general_purpose, Engine as _};
use serde::{Deserialize, Deserializer, Serializer};
use sha2::Digest;

const FIXED_NONCE: &[u8; 12] = b"antigravsalt";
const ENCRYPTED_PREFIX: &str = "ag_enc_";

/// 生成加密密钥 (基于设备 ID)
fn get_encryption_key() -> [u8; 32] {
    // 使用设备唯一标识生成密钥
    let device_id = machine_uid::get().unwrap_or_else(|_| "default".to_string());
    let mut key = [0u8; 32];
    let hash = sha2::Sha256::digest(device_id.as_bytes());
    key.copy_from_slice(&hash);
    key
}

pub fn serialize_password<S>(password: &str, serializer: S) -> Result<S::Ok, S::Error>
where
    S: Serializer,
{
    // [FIX #1738] 防止双重加密：检查是否已包含魔术前缀
    if password.starts_with(ENCRYPTED_PREFIX) {
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

    // [FIX #1738] 检查魔术前缀
    if raw.starts_with(ENCRYPTED_PREFIX) {
        // 新版格式：去前缀后解密
        let ciphertext = &raw[ENCRYPTED_PREFIX.len()..];
        match decrypt_string_internal(ciphertext) {
            Ok(plaintext) => Ok(plaintext),
            Err(_) => {
                // 解密失败（如密钥变更），返回原始密文以防止数据丢失
                Ok(raw)
            }
        }
    } else {
        // 兼容旧版：尝试直接解密
        match decrypt_string_internal(&raw) {
            Ok(plaintext) => {
                // 只有当解密出有效的 UTF-8 且看起来像合理个字符串时才认为是旧版密文
                // 这里 decrypt_string_internal 已经保证了 UTF-8，
                // 如果是用户输入的明文，通常解密会失败（Base64 错误或 Tag 校验错误）。
                Ok(plaintext)
            }
            Err(_) => {
                // 解密失败，认为是普通明文（用户输入的无前缀密码）
                Ok(raw)
            }
        }
    }
}

pub fn encrypt_string(password: &str) -> Result<String, String> {
    let key = get_encryption_key();
    let cipher = Aes256Gcm::new(&key.into());
    // In production, we should use a random nonce and prepend it to the ciphertext
    // For simplicity in this demo, we use a fixed nonce (NOT SECURE for repeats)
    // improving security: use random nonce
    let nonce = Nonce::from_slice(FIXED_NONCE);

    let ciphertext = cipher
        .encrypt(nonce, password.as_bytes())
        .map_err(|e| format!("Encryption failed: {}", e))?;

    let base64_ciphertext = general_purpose::STANDARD.encode(ciphertext);
    // [FIX #1738] 添加魔术前缀
    Ok(format!("{}{}", ENCRYPTED_PREFIX, base64_ciphertext))
}

/// 内部解密函数 (输入必须是纯 Base64 密文，不含前缀)
fn decrypt_string_internal(encrypted_base64: &str) -> Result<String, String> {
    let key = get_encryption_key();
    let cipher = Aes256Gcm::new(&key.into());
    let nonce = Nonce::from_slice(FIXED_NONCE);

    let ciphertext = general_purpose::STANDARD
        .decode(encrypted_base64)
        .map_err(|e| format!("Base64 decode failed: {}", e))?;

    let plaintext = cipher
        .decrypt(nonce, ciphertext.as_ref())
        .map_err(|e| format!("Decryption failed: {}", e))?;

    String::from_utf8(plaintext).map_err(|e| format!("UTF-8 conversion failed: {}", e))
}

pub fn decrypt_string(encrypted: &str) -> Result<String, String> {
    if encrypted.starts_with(ENCRYPTED_PREFIX) {
        decrypt_string_internal(&encrypted[ENCRYPTED_PREFIX.len()..])
    } else {
        decrypt_string_internal(encrypted)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_encrypt_decrypt_cycle() {
        let password = "my_secret_password";
        let encrypted = encrypt_string(password).unwrap();

        assert!(encrypted.starts_with(ENCRYPTED_PREFIX));
        assert_ne!(password, encrypted);

        let decrypted = decrypt_string(&encrypted).unwrap();
        assert_eq!(password, decrypted);
    }

    #[test]
    fn test_legacy_compatibility() {
        // 模拟旧版加密（手动调用内部逻辑生成无前缀密文）
        let password = "legacy_password";
        let key = get_encryption_key();
        let cipher = Aes256Gcm::new(&key.into());
        let nonce = Nonce::from_slice(FIXED_NONCE);
        let ciphertext = cipher.encrypt(nonce, password.as_bytes()).unwrap();
        let legacy_encrypted = general_purpose::STANDARD.encode(ciphertext);

        assert!(!legacy_encrypted.starts_with(ENCRYPTED_PREFIX));

        // 使用新版解密逻辑
        let decrypted = decrypt_string(&legacy_encrypted).unwrap();
        assert_eq!(password, decrypted);
    }
}
