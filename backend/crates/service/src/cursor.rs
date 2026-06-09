//! Shared opaque keyset-cursor codec.
//!
//! Every keyset-paginated query encodes its `ORDER BY` tuple through this single
//! module so the query and the cursor can never drift. Cursors are base64url
//! (no padding) over a versioned HMAC-SHA256 envelope and are opaque to clients.

use std::sync::OnceLock;

use base64::Engine as _;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use serde::Serialize;
use serde::de::DeserializeOwned;
use sha2::{Digest, Sha256};

const CURSOR_VERSION: u8 = 1;
const HMAC_SHA256_LEN: usize = 32;
const HMAC_BLOCK_LEN: usize = 64;
const MIN_SIGNING_KEY_LEN: usize = 32;
#[cfg(any(test, feature = "test-util"))]
const DEFAULT_TEST_SIGNING_KEY: &[u8] = b"notegate-test-cursor-signing-key-32-bytes";

static SIGNING_KEY: OnceLock<Vec<u8>> = OnceLock::new();

/// A cursor failed to decode (corrupt or tampered).
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("invalid cursor")]
pub struct CursorError;

/// Configure the cursor signing key for this process.
pub fn configure_signing_key(key: &[u8]) -> Result<(), CursorError> {
    if key.len() < MIN_SIGNING_KEY_LEN {
        return Err(CursorError);
    }
    let _already_configured = SIGNING_KEY.set(key.to_vec());
    Ok(())
}

/// Encode a keyset tuple into an opaque cursor string.
pub fn encode<T: Serialize>(value: &T) -> Result<String, CursorError> {
    let payload = serde_json::to_vec(value).map_err(|_error| CursorError)?;
    let signature = hmac_sha256(signing_key(), &payload);

    let mut envelope = Vec::with_capacity(1 + HMAC_SHA256_LEN + payload.len());
    envelope.push(CURSOR_VERSION);
    envelope.extend_from_slice(&signature);
    envelope.extend_from_slice(&payload);

    Ok(URL_SAFE_NO_PAD.encode(envelope))
}

/// Decode an opaque cursor string back into its keyset tuple.
pub fn decode<T: DeserializeOwned>(cursor: &str) -> Result<T, CursorError> {
    let envelope = URL_SAFE_NO_PAD
        .decode(cursor)
        .map_err(|_error| CursorError)?;
    let (version, rest) = envelope.split_first().ok_or(CursorError)?;
    if *version != CURSOR_VERSION || rest.len() < HMAC_SHA256_LEN {
        return Err(CursorError);
    }
    let (signature, payload) = rest.split_at(HMAC_SHA256_LEN);
    let expected = hmac_sha256(signing_key(), payload);
    if !constant_time_eq(signature, &expected) {
        return Err(CursorError);
    }
    serde_json::from_slice(payload).map_err(|_error| CursorError)
}

// In a non-test build the signing key MUST be configured at boot
// (`configure_signing_key` from `main.rs`); reaching `signing_key()` unconfigured is
// a boot-ordering bug, so fail closed (`expect`) rather than sign with a default.
#[allow(clippy::expect_used)]
fn signing_key() -> &'static [u8] {
    #[cfg(any(test, feature = "test-util"))]
    {
        SIGNING_KEY
            .get()
            .map(Vec::as_slice)
            .unwrap_or(DEFAULT_TEST_SIGNING_KEY)
    }
    #[cfg(not(any(test, feature = "test-util")))]
    {
        SIGNING_KEY
            .get()
            .map(Vec::as_slice)
            .expect("cursor signing key not configured")
    }
}

fn hmac_sha256(key: &[u8], message: &[u8]) -> [u8; HMAC_SHA256_LEN] {
    let mut key_block = [0_u8; HMAC_BLOCK_LEN];
    if key.len() > HMAC_BLOCK_LEN {
        let digest = Sha256::digest(key);
        for (slot, byte) in key_block.iter_mut().zip(digest.iter().copied()) {
            *slot = byte;
        }
    } else {
        for (slot, byte) in key_block.iter_mut().zip(key.iter().copied()) {
            *slot = byte;
        }
    }

    let mut ipad = [0x36_u8; HMAC_BLOCK_LEN];
    let mut opad = [0x5c_u8; HMAC_BLOCK_LEN];
    for ((inner, outer), key_byte) in ipad.iter_mut().zip(opad.iter_mut()).zip(key_block) {
        *inner ^= key_byte;
        *outer ^= key_byte;
    }

    let mut inner = Sha256::new();
    inner.update(ipad);
    inner.update(message);
    let inner_digest = inner.finalize();

    let mut outer = Sha256::new();
    outer.update(opad);
    outer.update(inner_digest);
    let digest = outer.finalize();

    let mut out = [0_u8; HMAC_SHA256_LEN];
    for (slot, byte) in out.iter_mut().zip(digest.iter().copied()) {
        *slot = byte;
    }
    out
}

fn constant_time_eq(left: &[u8], right: &[u8]) -> bool {
    if left.len() != right.len() {
        return false;
    }
    let mut diff = 0_u8;
    for (a, b) in left.iter().zip(right) {
        diff |= a ^ b;
    }
    diff == 0
}

#[cfg(test)]
mod tests {
    #![allow(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::indexing_slicing,
        clippy::panic,
        clippy::unwrap_in_result
    )]
    use super::*;
    use uuid::Uuid;

    #[test]
    fn config_rejects_short_key() {
        assert_eq!(configure_signing_key(b"short"), Err(CursorError));
    }

    #[test]
    fn round_trips_a_tuple() {
        let value = (3_i32, "name".to_owned(), Uuid::nil());
        let encoded = encode(&value).unwrap();
        let decoded: (i32, String, Uuid) = decode(&encoded).unwrap();
        assert_eq!(decoded, value);
    }

    #[test]
    fn rejects_garbage() {
        let decoded: Result<(i32,), _> = decode("!!!not-base64!!!");
        assert_eq!(decoded, Err(CursorError));
    }

    #[test]
    fn rejects_tampered_payload() {
        let value = (3_i32, "name".to_owned(), Uuid::nil());
        let encoded = encode(&value).unwrap();
        let mut bytes = URL_SAFE_NO_PAD.decode(&encoded).unwrap();
        let last = bytes.last_mut().expect("payload byte");
        *last ^= 1;
        let tampered = URL_SAFE_NO_PAD.encode(bytes);
        let decoded: Result<(i32, String, Uuid), _> = decode(&tampered);
        assert_eq!(decoded, Err(CursorError));
    }
}
