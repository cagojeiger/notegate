//! Mutating commands for the file tree.
pub mod checks;
pub mod copy_node;
pub mod create;
pub mod delete;
pub mod move_node;
pub mod save;
pub mod update;

use notegate_core::security::PiiCrypto;
use notegate_core::tier::UserTier;
use notegate_core::{Error, Result};
use notegate_model::files::WriteTextBody;
use serde_json::Value;
use uuid::Uuid;

struct StoredTextParts<'a> {
    storage_format: &'static str,
    content_text: Option<&'a str>,
    encrypted_payload: Option<&'a Value>,
    at_rest_encryption: &'static str,
    content_ciphertext: Option<Vec<u8>>,
    content_nonce: Option<Vec<u8>>,
    content_enc_key_id: Option<String>,
    content_enc_version: Option<i32>,
}

fn stored_text_parts<'a>(
    content: &'a notegate_model::files::StoredContent,
    encrypt_at_rest: bool,
    owner_tier: UserTier,
    crypto: &PiiCrypto,
    space_id: Uuid,
    node_id: Uuid,
) -> Result<StoredTextParts<'a>> {
    match &content.body {
        WriteTextBody::Plain(content) if encrypt_at_rest => {
            if !owner_tier.features().text_encryption {
                return Err(Error::conflict(
                    "text encryption is not available for the space owner's tier",
                ));
            }
            let encrypted = crypto.encrypt_text_content(
                &space_id.to_string(),
                &node_id.to_string(),
                content,
            )?;
            Ok(StoredTextParts {
                storage_format: "plain",
                content_text: None,
                encrypted_payload: None,
                at_rest_encryption: "server",
                content_ciphertext: Some(encrypted.ciphertext),
                content_nonce: Some(encrypted.nonce),
                content_enc_key_id: Some(crypto.enc_key_id().to_owned()),
                content_enc_version: Some(crypto.version()),
            })
        }
        WriteTextBody::Plain(content) => Ok(StoredTextParts {
            storage_format: "plain",
            content_text: Some(content),
            encrypted_payload: None,
            at_rest_encryption: "none",
            content_ciphertext: None,
            content_nonce: None,
            content_enc_key_id: None,
            content_enc_version: None,
        }),
        WriteTextBody::Encrypted(_) if encrypt_at_rest => Err(Error::conflict(
            "server text encryption requires plain text storage",
        )),
        WriteTextBody::Encrypted(payload) => Ok(StoredTextParts {
            storage_format: "encrypted",
            content_text: None,
            encrypted_payload: Some(payload),
            at_rest_encryption: "none",
            content_ciphertext: None,
            content_nonce: None,
            content_enc_key_id: None,
            content_enc_version: None,
        }),
    }
}
