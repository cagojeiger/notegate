//! Adapt pure content metrics to persisted plain and client-encrypted text.

use notegate_text::content::sha256_hex;
pub use notegate_text::content::{Metrics, compute};

use super::{StoredContent, WriteTextBody};
use crate::error::{ServiceError, ServiceResult};

/// Bundle validated metrics with plain content for storage. Counts have already
/// been checked against the text limits before conversion to database columns.
pub fn into_stored_plain(metrics: Metrics, content: String) -> StoredContent {
    StoredContent {
        body: WriteTextBody::Plain(content),
        content_sha256: metrics.content_sha256,
        byte_len: metrics.byte_len as i64,
        line_count: metrics.line_count as i32,
    }
}

/// Compute stored metrics for a client-side encrypted payload. The payload is
/// opaque to the server, so line count is always `0` and hash/bytes are based on
/// its JSON serialization.
pub fn compute_encrypted(payload: serde_json::Value) -> ServiceResult<StoredContent> {
    if !payload.is_object() {
        return Err(ServiceError::InvalidInput(
            "encrypted_payload must be a JSON object".to_owned(),
        ));
    }
    let bytes = serde_json::to_vec(&payload)
        .map_err(|error| ServiceError::InvalidInput(error.to_string()))?;
    Ok(StoredContent {
        body: WriteTextBody::Encrypted(payload),
        content_sha256: sha256_hex(&bytes),
        byte_len: bytes.len() as i64,
        line_count: 0,
    })
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use super::*;

    #[test]
    fn stored_plain_content_keeps_engine_metrics() {
        let content = "가\r\n🙂\n".to_owned();
        let metrics = compute(&content);
        let stored = into_stored_plain(metrics.clone(), content.clone());
        assert_eq!(stored.body, WriteTextBody::Plain(content));
        assert_eq!(stored.content_sha256, metrics.content_sha256);
        assert_eq!(stored.byte_len, 10);
        assert_eq!(stored.line_count, 2);
    }

    #[test]
    fn encrypted_content_stays_opaque_and_hashes_serialized_bytes() {
        let payload = serde_json::json!({"ciphertext": "opaque\\nbytes"});
        let bytes = serde_json::to_vec(&payload).unwrap();
        let stored = compute_encrypted(payload.clone()).unwrap();
        assert_eq!(stored.body, WriteTextBody::Encrypted(payload));
        assert_eq!(stored.content_sha256, sha256_hex(&bytes));
        assert_eq!(stored.byte_len, bytes.len() as i64);
        assert_eq!(stored.line_count, 0);
        assert_eq!(
            compute_encrypted(serde_json::json!([])),
            Err(ServiceError::InvalidInput(
                "encrypted_payload must be a JSON object".to_owned()
            )),
        );
    }
}
