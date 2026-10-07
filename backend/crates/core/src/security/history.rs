//! History payload encryption. IDs, time and structural routing stay queryable.
use base64::{Engine as _, engine::general_purpose::STANDARD};
use serde::{Deserialize, Serialize};

use super::{
    CRYPTO_VERSION, EncryptedField, PiiCrypto, decrypt_with_key_and_aad, encrypt_with_key_and_aad,
};
use crate::{Error, Result};

#[derive(Debug, Serialize, Deserialize)]
pub struct EncryptedHistoryValue {
    key_id: String,
    version: i32,
    ciphertext: String,
    nonce: String,
}

impl PiiCrypto {
    pub fn encrypt_history(&self, binding: &str, plaintext: &str) -> Result<EncryptedHistoryValue> {
        let field = encrypt_with_key_and_aad(
            &self.history_field_key,
            plaintext.as_bytes(),
            &history_aad(binding, self.enc_key_id()),
        )?;
        Ok(EncryptedHistoryValue {
            key_id: self.enc_key_id().to_owned(),
            version: CRYPTO_VERSION,
            ciphertext: STANDARD.encode(field.ciphertext),
            nonce: STANDARD.encode(field.nonce),
        })
    }

    pub fn decrypt_history(&self, binding: &str, value: &EncryptedHistoryValue) -> Result<String> {
        if value.key_id != self.enc_key_id() || value.version != CRYPTO_VERSION {
            return Err(Error::internal(
                "history encryption key or version mismatch",
            ));
        }
        let field = EncryptedField {
            ciphertext: STANDARD
                .decode(&value.ciphertext)
                .map_err(|_| Error::internal("invalid history ciphertext"))?,
            nonce: STANDARD
                .decode(&value.nonce)
                .map_err(|_| Error::internal("invalid history nonce"))?,
        };
        let bytes = decrypt_with_key_and_aad(
            &self.history_field_key,
            &field,
            &history_aad(binding, &value.key_id),
        )?;
        String::from_utf8(bytes).map_err(|_| Error::internal("invalid history utf8"))
    }
}

fn history_aad(binding: &str, key_id: &str) -> Vec<u8> {
    format!("app=notegate;field=history;binding={binding};key_id={key_id};version={CRYPTO_VERSION}")
        .into_bytes()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn history_is_bound_to_stream_owner_and_row() -> Result<()> {
        let crypto = PiiCrypto::test();
        let binding = "changes/space-1/42";
        let encrypted = crypto.encrypt_history(binding, "confidential name and purpose")?;
        assert_eq!(
            crypto.decrypt_history(binding, &encrypted)?,
            "confidential name and purpose"
        );
        for other in [
            "changes/space-2/42",
            "changes/space-1/43",
            "invocations/space-1/42",
        ] {
            assert!(crypto.decrypt_history(other, &encrypted).is_err());
        }
        let mut tampered = encrypted;
        tampered.version += 1;
        assert!(crypto.decrypt_history(binding, &tampered).is_err());
        Ok(())
    }
}
