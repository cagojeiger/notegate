//! Shared opaque keyset-cursor codec.
//!
//! Every keyset-paginated query encodes its `ORDER BY` tuple through this single
//! module so the query and the cursor can never drift. Cursors are base64url
//! (no padding) over a JSON-serialized tuple and are opaque to clients.

use base64::Engine as _;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use serde::Serialize;
use serde::de::DeserializeOwned;

/// A cursor failed to decode (corrupt or tampered).
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("invalid cursor")]
pub struct CursorError;

/// Encode a keyset tuple into an opaque cursor string.
pub fn encode<T: Serialize>(value: &T) -> Result<String, CursorError> {
    let bytes = serde_json::to_vec(value).map_err(|_error| CursorError)?;
    Ok(URL_SAFE_NO_PAD.encode(bytes))
}

/// Decode an opaque cursor string back into its keyset tuple.
pub fn decode<T: DeserializeOwned>(cursor: &str) -> Result<T, CursorError> {
    let bytes = URL_SAFE_NO_PAD
        .decode(cursor)
        .map_err(|_error| CursorError)?;
    serde_json::from_slice(&bytes).map_err(|_error| CursorError)
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
}
