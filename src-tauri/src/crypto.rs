//! 加密引擎：Argon2id 密钥派生 + AES-256-GCM 加解密
use aes_gcm::{
    aead::{Aead, KeyInit},
    Aes256Gcm, Key, Nonce,
};
use argon2::{Argon2, Algorithm, Params, Version};
use base64::{engine::general_purpose::STANDARD as B64, Engine};
use rand::rngs::OsRng;
use rand::RngCore;
use thiserror::Error;
use zeroize::Zeroizing;

#[derive(Error, Debug)]
pub enum CryptoError {
    #[error("加密失败: {0}")]
    Encrypt(String),
    #[error("解密失败: {0}")]
    Decrypt(String),
    #[error("密钥派生失败: {0}")]
    Derive(String),
}

/// 生成随机盐值
pub fn generate_salt(len: usize) -> Vec<u8> {
    let mut salt = vec![0u8; len];
    OsRng.fill_bytes(&mut salt);
    salt
}

/// 生成随机 256-bit DEK
pub fn generate_dek() -> Vec<u8> {
    let mut dek = vec![0u8; 32];
    OsRng.fill_bytes(&mut dek);
    dek
}

/// 通过 Argon2id 从主密码派生加密密钥
pub fn derive_key(master_password: &str, salt: &[u8]) -> Result<[u8; 32], CryptoError> {
    let mut key = [0u8; 32];
    let params = Params::new(64 * 1024, 3, 4, Some(32))
        .map_err(|e| CryptoError::Derive(e.to_string()))?;
    let argon2 = Argon2::new(Algorithm::Argon2id, Version::V0x13, params);
    argon2
        .hash_password_into(master_password.as_bytes(), salt, &mut key)
        .map_err(|e| CryptoError::Derive(e.to_string()))?;
    Ok(key)
}

/// AES-256-GCM 加密，返回 base64( nonce + ciphertext )
pub fn encrypt(plaintext: &[u8], key: &[u8]) -> Result<String, CryptoError> {
    let cipher = Aes256Gcm::new(Key::<Aes256Gcm>::from_slice(key));
    let mut nonce_bytes = [0u8; 12];
    OsRng.fill_bytes(&mut nonce_bytes);
    let nonce = Nonce::from_slice(&nonce_bytes);
    let ciphertext = cipher
        .encrypt(nonce, plaintext)
        .map_err(|e| CryptoError::Encrypt(e.to_string()))?;
    let mut out = nonce_bytes.to_vec();
    out.extend_from_slice(&ciphertext);
    Ok(B64.encode(out))
}

/// AES-256-GCM 解密，输入 base64( nonce + ciphertext )
pub fn decrypt(encoded: &str, key: &[u8]) -> Result<Vec<u8>, CryptoError> {
    let raw = B64
        .decode(encoded)
        .map_err(|e| CryptoError::Decrypt(e.to_string()))?;
    if raw.len() < 12 {
        return Err(CryptoError::Decrypt("密文长度不足".into()));
    }
    let (nonce_bytes, ciphertext) = raw.split_at(12);
    let cipher = Aes256Gcm::new(Key::<Aes256Gcm>::from_slice(key));
    let nonce = Nonce::from_slice(nonce_bytes);
    cipher
        .decrypt(nonce, ciphertext)
        .map_err(|e| CryptoError::Decrypt(e.to_string()))
}

/// 加密字符串（自动处理 UTF-8）
pub fn encrypt_str(plaintext: &str, key: &[u8]) -> Result<String, CryptoError> {
    encrypt(plaintext.as_bytes(), key)
}

/// 解密字符串
pub fn decrypt_str(encoded: &str, key: &[u8]) -> Result<Zeroizing<String>, CryptoError> {
    let bytes = decrypt(encoded, key)?;
    let s = String::from_utf8(bytes).map_err(|e| CryptoError::Decrypt(e.to_string()))?;
    Ok(Zeroizing::new(s))
}

/// 加密二进制数据（用于图片等）
pub fn encrypt_bytes(plaintext: &[u8], key: &[u8]) -> Result<String, CryptoError> {
    encrypt(plaintext, key)
}

/// 解密二进制数据
pub fn decrypt_bytes(encoded: &str, key: &[u8]) -> Result<Vec<u8>, CryptoError> {
    decrypt(encoded, key)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_roundtrip() {
        let salt = generate_salt(16);
        let key = derive_key("test-password-123", &salt).unwrap();
        let plaintext = "hello securenote 你好世界";
        let enc = encrypt_str(plaintext, &key).unwrap();
        let dec = decrypt_str(&enc, &key).unwrap();
        assert_eq!(plaintext, dec.as_str());
    }

    #[test]
    fn test_wrong_key_fails() {
        let salt = generate_salt(16);
        let key1 = derive_key("password-a", &salt).unwrap();
        let key2 = derive_key("password-b", &salt).unwrap();
        let enc = encrypt_str("secret", &key1).unwrap();
        assert!(decrypt_str(&enc, &key2).is_err());
    }

    #[test]
    fn test_bytes_roundtrip() {
        let key = generate_dek();
        let mut k = [0u8; 32];
        k.copy_from_slice(&key);
        let data = vec![1u8, 2, 3, 4, 5, 255, 0, 128];
        let enc = encrypt_bytes(&data, &k).unwrap();
        let dec = decrypt_bytes(&enc, &k).unwrap();
        assert_eq!(data, dec);
    }
}
