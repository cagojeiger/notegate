//! PII encryption and lookup hashing helpers.
//!
//! This is the local implementation of the policy in `docs/spec/security.md`.
//! It uses an application-provided master secret as the local KEK and an HMAC
//! pepper for lookup hashes. A future KMS adapter can replace the DEK wrapping
//! boundary without changing repository call sites.

use std::fmt::Write as _;

use aes_gcm::aead::{Aead, KeyInit};
use aes_gcm::{Aes256Gcm, Nonce};
use hmac::{Hmac, Mac};
use rand::RngCore as _;
use rand::rngs::OsRng;
use secrecy::{ExposeSecret, SecretString};
use sha2::{Digest as _, Sha256};

use crate::{Error, Result};

type HmacSha256 = Hmac<Sha256>;

const NONCE_LEN: usize = 12;
const DEK_LEN: usize = 32;

/// Local PII crypto provider.
#[derive(Debug, Clone)]
pub struct PiiCrypto {
    kek: [u8; DEK_LEN],
    pepper: Vec<u8>,
    kek_id: String,
    kek_version: Option<String>,
    hash_version: i32,
}

/// Encrypted field value split into ciphertext and nonce columns.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EncryptedField {
    pub ciphertext: Vec<u8>,
    pub nonce: Vec<u8>,
}

impl PiiCrypto {
    /// Build from configured secrets. The strings are hashed into fixed-length
    /// keys so deployments can provide long random strings without caring about
    /// raw key encoding.
    pub fn from_secrets(master_key: &SecretString, hash_pepper: &SecretString) -> Self {
        Self {
            kek: sha256_key(master_key.expose_secret().as_bytes()),
            pepper: hash_pepper.expose_secret().as_bytes().to_vec(),
            kek_id: "local".to_owned(),
            kek_version: Some("1".to_owned()),
            hash_version: 1,
        }
    }

    /// Deterministic test/dev provider used by repository unit tests and lazy
    /// constructors. Production `AppState` injects [`Self::from_secrets`].
    pub fn test() -> Self {
        Self {
            kek: sha256_key(b"notegate-test-pii-master-key-32-bytes"),
            pepper: b"notegate-test-pii-hmac-pepper-32-bytes".to_vec(),
            kek_id: "test".to_owned(),
            kek_version: Some("1".to_owned()),
            hash_version: 1,
        }
    }

    pub fn kek_id(&self) -> &str {
        &self.kek_id
    }

    pub fn kek_version(&self) -> Option<&str> {
        self.kek_version.as_deref()
    }

    pub fn hash_version(&self) -> i32 {
        self.hash_version
    }

    pub fn generate_dek(&self) -> [u8; DEK_LEN] {
        let mut dek = [0_u8; DEK_LEN];
        OsRng.fill_bytes(&mut dek);
        dek
    }

    /// Wrap a DEK with the local KEK. The returned blob is `nonce || ciphertext`.
    pub fn wrap_dek(&self, dek: &[u8; DEK_LEN]) -> Result<Vec<u8>> {
        let encrypted = encrypt_with_key(&self.kek, dek)?;
        Ok(join_nonce_ciphertext(encrypted))
    }

    /// Unwrap a DEK previously returned by [`Self::wrap_dek`].
    pub fn unwrap_dek(&self, wrapped: &[u8]) -> Result<[u8; DEK_LEN]> {
        let encrypted = split_nonce_ciphertext(wrapped)?;
        let bytes = decrypt_with_key(&self.kek, &encrypted)?;
        if bytes.len() != DEK_LEN {
            return Err(Error::internal("invalid wrapped DEK length"));
        }
        let mut dek = [0_u8; DEK_LEN];
        dek.copy_from_slice(&bytes);
        Ok(dek)
    }

    pub fn encrypt_string(&self, dek: &[u8; DEK_LEN], plaintext: &str) -> Result<EncryptedField> {
        encrypt_with_key(dek, plaintext.as_bytes())
    }

    pub fn decrypt_string(&self, dek: &[u8; DEK_LEN], field: &EncryptedField) -> Result<String> {
        let bytes = decrypt_with_key(dek, field)?;
        String::from_utf8(bytes).map_err(|_error| Error::internal("invalid encrypted utf8"))
    }

    pub fn hmac_hex(&self, value: &str) -> Result<String> {
        let mut mac = <HmacSha256 as Mac>::new_from_slice(&self.pepper)
            .map_err(|_error| Error::internal("invalid HMAC key"))?;
        mac.update(value.as_bytes());
        Ok(hex_lower(&mac.finalize().into_bytes()))
    }

    pub fn provider_sub_hash(&self, provider: &str, subject: &str) -> Result<String> {
        self.hmac_hex(&format!("{provider}:{subject}"))
    }

    pub fn email_hash(&self, email: &str) -> Result<String> {
        self.hmac_hex(&normalize_email(email))
    }
}

pub fn normalize_email(email: &str) -> String {
    email.trim().to_ascii_lowercase()
}

fn encrypt_with_key(key: &[u8; DEK_LEN], plaintext: &[u8]) -> Result<EncryptedField> {
    let cipher = Aes256Gcm::new_from_slice(key)
        .map_err(|_error| Error::internal("invalid encryption key"))?;
    let mut nonce = [0_u8; NONCE_LEN];
    OsRng.fill_bytes(&mut nonce);
    let ciphertext = cipher
        .encrypt(Nonce::from_slice(&nonce), plaintext)
        .map_err(|_error| Error::internal("PII encryption failed"))?;
    Ok(EncryptedField {
        ciphertext,
        nonce: nonce.to_vec(),
    })
}

fn decrypt_with_key(key: &[u8; DEK_LEN], field: &EncryptedField) -> Result<Vec<u8>> {
    if field.nonce.len() != NONCE_LEN {
        return Err(Error::internal("invalid encryption nonce"));
    }
    let cipher = Aes256Gcm::new_from_slice(key)
        .map_err(|_error| Error::internal("invalid encryption key"))?;
    cipher
        .decrypt(Nonce::from_slice(&field.nonce), field.ciphertext.as_ref())
        .map_err(|_error| Error::internal("PII decryption failed"))
}

fn join_nonce_ciphertext(field: EncryptedField) -> Vec<u8> {
    let mut out = Vec::with_capacity(NONCE_LEN + field.ciphertext.len());
    out.extend_from_slice(&field.nonce);
    out.extend_from_slice(&field.ciphertext);
    out
}

fn split_nonce_ciphertext(value: &[u8]) -> Result<EncryptedField> {
    if value.len() <= NONCE_LEN {
        return Err(Error::internal("invalid wrapped DEK"));
    }
    let (nonce, ciphertext) = value.split_at(NONCE_LEN);
    Ok(EncryptedField {
        nonce: nonce.to_vec(),
        ciphertext: ciphertext.to_vec(),
    })
}

fn sha256_key(value: &[u8]) -> [u8; DEK_LEN] {
    let digest = Sha256::digest(value);
    let mut key = [0_u8; DEK_LEN];
    key.copy_from_slice(&digest);
    key
}

fn hex_lower(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        let _ = write!(&mut out, "{byte:02x}");
    }
    out
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use super::*;

    #[test]
    fn encrypt_decrypt_round_trips() {
        let crypto = PiiCrypto::test();
        let dek = crypto.generate_dek();
        let encrypted = crypto.encrypt_string(&dek, "Kang").unwrap();
        assert_ne!(encrypted.ciphertext, b"Kang");
        assert_eq!(crypto.decrypt_string(&dek, &encrypted).unwrap(), "Kang");
    }

    #[test]
    fn wrapped_dek_round_trips() {
        let crypto = PiiCrypto::test();
        let dek = crypto.generate_dek();
        let wrapped = crypto.wrap_dek(&dek).unwrap();
        assert_ne!(wrapped, dek);
        assert_eq!(crypto.unwrap_dek(&wrapped).unwrap(), dek);
    }

    #[test]
    fn hmac_is_stable_and_normalizes_email() {
        let crypto = PiiCrypto::test();
        assert_eq!(
            crypto.email_hash(" User@Example.Test ").unwrap(),
            crypto.email_hash("user@example.test").unwrap()
        );
    }
}
