//! PII encryption, lookup hashing, API-key hashing, and session-key derivation.
//!
//! This module implements the policy in `docs/spec/security.md`: runtime code
//! reads ENC/LOOKUP root secrets, derives purpose-specific subkeys with HKDF,
//! and never uses a raw root secret directly as an encryption/HMAC/signing key.

use std::fmt::Write as _;

use aes_gcm::aead::{Aead, AeadCore as _, KeyInit, OsRng as AeadOsRng, Payload};
use aes_gcm::{Aes256Gcm, Nonce};
use hkdf::Hkdf;
use hmac::{Hmac, Mac};
use rand::RngCore as _;
use rand::rngs::OsRng;
use secrecy::{ExposeSecret, SecretString};
use sha2::Sha256;

use crate::{Error, Result};

type HmacSha256 = Hmac<Sha256>;

const NONCE_LEN: usize = 12;
const KEY_LEN: usize = 32;
const CRYPTO_VERSION: i32 = 1;

const ENC_EPOCH_VERIFY_LABEL: &[u8] = b"notegate/enc/epoch-verify/v1";
const PII_FIELD_LABEL: &[u8] = b"notegate/enc/pii-field/v1";
const LOOKUP_EPOCH_VERIFY_LABEL: &[u8] = b"notegate/lookup/epoch-verify/v1";
const PROVIDER_SUB_HMAC_LABEL: &[u8] = b"notegate/lookup/provider-sub-hmac/v1";
const EMAIL_HMAC_LABEL: &[u8] = b"notegate/lookup/email-hmac/v1";
const API_KEY_HMAC_LABEL: &[u8] = b"notegate/lookup/api-key-hmac/v1";
const SESSION_SIGNING_LABEL: &[u8] = b"notegate/lookup/session-signing/v1";

const PROVIDER_SUB_PREFIX: &str = "provider-sub:v1:";
const EMAIL_PREFIX: &str = "email:v1:";
const API_KEY_PREFIX: &str = "api-key:v1:";
const KEY_EPOCH_PREFIX: &str = "key-epoch:v1:";

/// Domain for a registered root key epoch.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyDomain {
    Enc,
    Lookup,
}

impl KeyDomain {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Enc => "enc",
            Self::Lookup => "lookup",
        }
    }
}

/// Stable encrypted-field identifiers used in AEAD AAD.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PiiFieldKind {
    AccountDisplayName,
    UserEmail,
}

impl PiiFieldKind {
    fn as_str(self) -> &'static str {
        match self {
            Self::AccountDisplayName => "account.display_name",
            Self::UserEmail => "user.email",
        }
    }
}

/// Context bound to an encrypted PII field through AEAD AAD.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PiiAad {
    field: PiiFieldKind,
    account_id: String,
    key_id: String,
    version: i32,
}

impl PiiAad {
    pub fn new(
        field: PiiFieldKind,
        account_id: impl Into<String>,
        key_id: impl Into<String>,
    ) -> Self {
        Self {
            field,
            account_id: account_id.into(),
            key_id: key_id.into(),
            version: CRYPTO_VERSION,
        }
    }

    fn bytes(&self) -> Vec<u8> {
        format!(
            "app=notegate;field={};account_id={};key_id={};version={}",
            self.field.as_str(),
            self.account_id,
            self.key_id,
            self.version
        )
        .into_bytes()
    }
}

/// Runtime security provider derived from ENC and LOOKUP root secrets.
#[derive(Debug, Clone)]
pub struct PiiCrypto {
    enc_key_id: String,
    lookup_key_id: String,
    enc_epoch_verify_key: [u8; KEY_LEN],
    lookup_epoch_verify_key: [u8; KEY_LEN],
    pii_field_key: [u8; KEY_LEN],
    provider_sub_hmac_key: [u8; KEY_LEN],
    email_hmac_key: [u8; KEY_LEN],
    api_key_hmac_key: [u8; KEY_LEN],
    session_signing_key: [u8; KEY_LEN],
}

/// Encrypted field value split into ciphertext and nonce columns.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EncryptedField {
    pub ciphertext: Vec<u8>,
    pub nonce: Vec<u8>,
}

impl PiiCrypto {
    /// Build runtime subkeys from configured ENC/LOOKUP root secrets.
    pub fn from_root_secrets(
        enc_key_id: impl Into<String>,
        enc_root_secret: &SecretString,
        lookup_key_id: impl Into<String>,
        lookup_root_secret: &SecretString,
    ) -> Self {
        let enc_root = enc_root_secret.expose_secret().as_bytes();
        let lookup_root = lookup_root_secret.expose_secret().as_bytes();
        Self {
            enc_key_id: enc_key_id.into(),
            lookup_key_id: lookup_key_id.into(),
            enc_epoch_verify_key: hkdf_key(enc_root, ENC_EPOCH_VERIFY_LABEL),
            lookup_epoch_verify_key: hkdf_key(lookup_root, LOOKUP_EPOCH_VERIFY_LABEL),
            pii_field_key: hkdf_key(enc_root, PII_FIELD_LABEL),
            provider_sub_hmac_key: hkdf_key(lookup_root, PROVIDER_SUB_HMAC_LABEL),
            email_hmac_key: hkdf_key(lookup_root, EMAIL_HMAC_LABEL),
            api_key_hmac_key: hkdf_key(lookup_root, API_KEY_HMAC_LABEL),
            session_signing_key: hkdf_key(lookup_root, SESSION_SIGNING_LABEL),
        }
    }

    /// Deterministic test/dev provider used by repository tests and lazy constructors.
    pub fn test() -> Self {
        Self::from_root_secrets(
            "test-enc",
            &SecretString::from("notegate-test-enc-root-secret-32-bytes".to_owned()),
            "test-lookup",
            &SecretString::from("notegate-test-lookup-root-secret-32-bytes".to_owned()),
        )
    }

    pub fn enc_key_id(&self) -> &str {
        &self.enc_key_id
    }

    pub fn lookup_key_id(&self) -> &str {
        &self.lookup_key_id
    }

    pub fn version(&self) -> i32 {
        CRYPTO_VERSION
    }

    /// Temporary compatibility name until `account_encryption_keys` is removed.
    pub fn kek_id(&self) -> &str {
        self.enc_key_id()
    }

    /// Temporary compatibility name until `account_encryption_keys` is removed.
    pub fn kek_version(&self) -> Option<&str> {
        Some("1")
    }

    /// Temporary compatibility name until hash key ids are wired through rows.
    pub fn hash_version(&self) -> i32 {
        self.version()
    }

    pub fn session_signing_key(&self) -> &[u8] {
        &self.session_signing_key
    }

    pub fn enc_epoch_verify_tag(&self, key_id: &str) -> Result<String> {
        self.key_epoch_verify_tag(KeyDomain::Enc, key_id)
    }

    pub fn lookup_epoch_verify_tag(&self, key_id: &str) -> Result<String> {
        self.key_epoch_verify_tag(KeyDomain::Lookup, key_id)
    }

    fn key_epoch_verify_tag(&self, domain: KeyDomain, key_id: &str) -> Result<String> {
        let key = match domain {
            KeyDomain::Enc => &self.enc_epoch_verify_key,
            KeyDomain::Lookup => &self.lookup_epoch_verify_key,
        };
        hmac_hex(
            key,
            &format!("{KEY_EPOCH_PREFIX}{}:{key_id}", domain.as_str()),
        )
    }

    pub fn encrypt_pii_string(&self, aad: &PiiAad, plaintext: &str) -> Result<EncryptedField> {
        encrypt_with_key_and_aad(&self.pii_field_key, plaintext.as_bytes(), &aad.bytes())
    }

    pub fn decrypt_pii_string(&self, aad: &PiiAad, field: &EncryptedField) -> Result<String> {
        let bytes = decrypt_with_key_and_aad(&self.pii_field_key, field, &aad.bytes())?;
        String::from_utf8(bytes).map_err(|_error| Error::internal("invalid encrypted utf8"))
    }

    /// Temporary compatibility for the current DB layer until the DEK schema is removed.
    pub fn generate_dek(&self) -> [u8; KEY_LEN] {
        let mut dek = [0_u8; KEY_LEN];
        OsRng.fill_bytes(&mut dek);
        dek
    }

    /// Temporary compatibility for the current DB layer until the DEK schema is removed.
    pub fn wrap_dek(&self, dek: &[u8; KEY_LEN]) -> Result<Vec<u8>> {
        let encrypted = encrypt_with_key(&self.pii_field_key, dek)?;
        Ok(join_nonce_ciphertext(encrypted))
    }

    /// Temporary compatibility for the current DB layer until the DEK schema is removed.
    pub fn unwrap_dek(&self, wrapped: &[u8]) -> Result<[u8; KEY_LEN]> {
        let encrypted = split_nonce_ciphertext(wrapped)?;
        let bytes = decrypt_with_key(&self.pii_field_key, &encrypted)?;
        if bytes.len() != KEY_LEN {
            return Err(Error::internal("invalid wrapped DEK length"));
        }
        let mut dek = [0_u8; KEY_LEN];
        dek.copy_from_slice(&bytes);
        Ok(dek)
    }

    /// Temporary compatibility for the current DB layer until PII AAD is wired through.
    pub fn encrypt_string(&self, dek: &[u8; KEY_LEN], plaintext: &str) -> Result<EncryptedField> {
        encrypt_with_key(dek, plaintext.as_bytes())
    }

    /// Temporary compatibility for the current DB layer until PII AAD is wired through.
    pub fn decrypt_string(&self, dek: &[u8; KEY_LEN], field: &EncryptedField) -> Result<String> {
        let bytes = decrypt_with_key(dek, field)?;
        String::from_utf8(bytes).map_err(|_error| Error::internal("invalid encrypted utf8"))
    }

    pub fn provider_sub_hash(&self, provider: &str, subject: &str) -> Result<String> {
        hmac_hex(
            &self.provider_sub_hmac_key,
            &format!("{PROVIDER_SUB_PREFIX}{provider}:{subject}"),
        )
    }

    pub fn email_hash(&self, email: &str) -> Result<String> {
        hmac_hex(
            &self.email_hmac_key,
            &format!("{EMAIL_PREFIX}{}", normalize_email(email)),
        )
    }

    pub fn api_key_hash(&self, api_key_id: &str, secret: &str) -> Result<String> {
        hmac_hex(
            &self.api_key_hmac_key,
            &format!("{API_KEY_PREFIX}{api_key_id}:{secret}"),
        )
    }
}

pub fn normalize_email(email: &str) -> String {
    email.trim().to_ascii_lowercase()
}

fn hkdf_key(root: &[u8], label: &[u8]) -> [u8; KEY_LEN] {
    let hk = Hkdf::<Sha256>::new(None, root);
    let mut out = [0_u8; KEY_LEN];
    if hk.expand(label, &mut out).is_err() {
        return [0_u8; KEY_LEN];
    }
    out
}

fn encrypt_with_key(key: &[u8; KEY_LEN], plaintext: &[u8]) -> Result<EncryptedField> {
    encrypt_with_key_and_aad(key, plaintext, &[])
}

fn encrypt_with_key_and_aad(
    key: &[u8; KEY_LEN],
    plaintext: &[u8],
    aad: &[u8],
) -> Result<EncryptedField> {
    let cipher = Aes256Gcm::new_from_slice(key)
        .map_err(|_error| Error::internal("invalid encryption key"))?;
    let nonce = Aes256Gcm::generate_nonce(&mut AeadOsRng);
    let ciphertext = cipher
        .encrypt(
            &nonce,
            Payload {
                msg: plaintext,
                aad,
            },
        )
        .map_err(|_error| Error::internal("PII encryption failed"))?;
    Ok(EncryptedField {
        ciphertext,
        nonce: nonce.to_vec(),
    })
}

fn decrypt_with_key(key: &[u8; KEY_LEN], field: &EncryptedField) -> Result<Vec<u8>> {
    decrypt_with_key_and_aad(key, field, &[])
}

fn decrypt_with_key_and_aad(
    key: &[u8; KEY_LEN],
    field: &EncryptedField,
    aad: &[u8],
) -> Result<Vec<u8>> {
    if field.nonce.len() != NONCE_LEN {
        return Err(Error::internal("invalid encryption nonce"));
    }
    let cipher = Aes256Gcm::new_from_slice(key)
        .map_err(|_error| Error::internal("invalid encryption key"))?;
    cipher
        .decrypt(
            Nonce::from_slice(&field.nonce),
            Payload {
                msg: field.ciphertext.as_ref(),
                aad,
            },
        )
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

fn hmac_hex(key: &[u8; KEY_LEN], value: &str) -> Result<String> {
    let mut mac = <HmacSha256 as Mac>::new_from_slice(key)
        .map_err(|_error| Error::internal("invalid HMAC key"))?;
    mac.update(value.as_bytes());
    Ok(hex_lower(&mac.finalize().into_bytes()))
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
    #![allow(clippy::unwrap_used, clippy::expect_used)]

    use super::*;

    #[test]
    fn pii_aad_encrypt_decrypt_round_trips() {
        let crypto = PiiCrypto::test();
        let aad = PiiAad::new(
            PiiFieldKind::AccountDisplayName,
            "account-1",
            crypto.enc_key_id(),
        );
        let encrypted = crypto.encrypt_pii_string(&aad, "Kang").unwrap();
        assert_ne!(encrypted.ciphertext, b"Kang");
        assert_eq!(crypto.decrypt_pii_string(&aad, &encrypted).unwrap(), "Kang");
    }

    #[test]
    fn pii_aad_mismatch_rejects_decrypt() {
        let crypto = PiiCrypto::test();
        let aad = PiiAad::new(
            PiiFieldKind::AccountDisplayName,
            "account-1",
            crypto.enc_key_id(),
        );
        let wrong_aad = PiiAad::new(PiiFieldKind::UserEmail, "account-1", crypto.enc_key_id());
        let encrypted = crypto.encrypt_pii_string(&aad, "Kang").unwrap();
        assert!(crypto.decrypt_pii_string(&wrong_aad, &encrypted).is_err());
    }

    #[test]
    fn wrapped_dek_round_trips_during_schema_transition() {
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

    #[test]
    fn hmac_purposes_are_separated() {
        let crypto = PiiCrypto::test();
        assert_ne!(
            crypto.provider_sub_hash("authgate", "same").unwrap(),
            crypto.email_hash("same").unwrap()
        );
    }

    #[test]
    fn epoch_verify_tags_include_domain() {
        let crypto = PiiCrypto::test();
        assert_ne!(
            crypto.enc_epoch_verify_tag("key-1").unwrap(),
            crypto.lookup_epoch_verify_tag("key-1").unwrap()
        );
    }
}
