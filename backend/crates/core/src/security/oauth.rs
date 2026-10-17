//! Short-lived OAuth state and purpose-separated credential HMACs.
use base64::{Engine as _, engine::general_purpose::STANDARD};
use serde::{Deserialize, Serialize};
use super::{EncryptedField, PiiCrypto, decrypt_with_key_and_aad, encrypt_with_key_and_aad, hmac_hex};
use crate::{Error, Result};

#[derive(Debug, Serialize, Deserialize)]
pub struct EncryptedOAuthValue {
    key_id: String,
    ciphertext: String,
    nonce: String,
}

impl PiiCrypto {
    pub fn oauth_hash(&self, purpose: &str, value: &str) -> Result<String> {
        hmac_hex(&self.oauth_hmac_key, &format!("oauth:v1:{purpose}:{value}"))
    }

    pub fn encrypt_oauth_state(&self, id: &str, plaintext: &str) -> Result<EncryptedOAuthValue> {
        let aad = format!("notegate:oauth-state:v1:{id}:{}", self.enc_key_id());
        let field = encrypt_with_key_and_aad(&self.oauth_field_key, plaintext.as_bytes(), aad.as_bytes())?;
        Ok(EncryptedOAuthValue {
            key_id: self.enc_key_id().to_owned(),
            ciphertext: STANDARD.encode(field.ciphertext),
            nonce: STANDARD.encode(field.nonce),
        })
    }

    pub fn decrypt_oauth_state(&self, id: &str, value: &EncryptedOAuthValue) -> Result<String> {
        if value.key_id != self.enc_key_id() { return Err(Error::internal("OAuth state key mismatch")); }
        let field = EncryptedField {
            ciphertext: STANDARD.decode(&value.ciphertext).map_err(|_| Error::internal("invalid OAuth state"))?,
            nonce: STANDARD.decode(&value.nonce).map_err(|_| Error::internal("invalid OAuth state"))?,
        };
        let aad = format!("notegate:oauth-state:v1:{id}:{}", self.enc_key_id());
        let bytes = decrypt_with_key_and_aad(&self.oauth_field_key, &field, aad.as_bytes())?;
        String::from_utf8(bytes).map_err(|_| Error::internal("invalid OAuth state encoding"))
    }
}
