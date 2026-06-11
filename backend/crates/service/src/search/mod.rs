//! Space search: `find` (node name metadata) and `grep` (content).
//!
//! The service owns authorization, limit clamping, opaque cursors, and
//! service-side grep line splitting. The two query implementations live in the
//! [`find`] and [`grep`] submodules; shared types, the permission gate, and query validation live here.

use notegate_core::limits;
use notegate_db::FilesRepo;
use notegate_model::Permission;
pub use notegate_model::search::{
    FindCursor, FindPage, FindRequest, GrepCandidate, GrepCursor, GrepMatch, GrepPage, GrepRequest,
};
use uuid::Uuid;

use crate::error::{ServiceError, ServiceResult};
use crate::files::policy::{self, FileCommand};

mod find;
mod grep;

/// Search service. The `find`/`grep` query methods are implemented in the
/// [`find`] and [`grep`] submodules.
#[derive(Debug, Clone)]
pub struct SearchService {
    store: FilesRepo,
}

impl SearchService {
    pub fn new(store: FilesRepo) -> Self {
        Self { store }
    }

    /// Resolve the caller's permission (none ⇒ `404`) and gate by command
    /// (insufficient permission ⇒ `403`). Mirrors the file service's authorization.
    async fn authorize(
        &self,
        space_id: Uuid,
        account_id: Uuid,
        command: FileCommand,
    ) -> ServiceResult<Permission> {
        let permission = self
            .store
            .permission_for(space_id, account_id)
            .await?
            .ok_or_else(|| ServiceError::NotFound("space not found".to_owned()))?;
        policy::require(permission, command)?;
        Ok(permission)
    }
}

/// Reject empty, multi-line, or very long search strings before they can become
/// broad or expensive `ILIKE` scans.
fn validate_query(q: &str) -> ServiceResult<&str> {
    let trimmed = q.trim();
    if trimmed.is_empty() {
        return Err(ServiceError::InvalidInput(
            "search query cannot be empty".to_owned(),
        ));
    }
    if trimmed.contains(['\n', '\r']) {
        return Err(ServiceError::InvalidInput(
            "search query must be a single line".to_owned(),
        ));
    }
    if trimmed.chars().count() > limits::SEARCH_QUERY_MAX_CHARS {
        return Err(ServiceError::InvalidInput(format!(
            "search query must be at most {} characters",
            limits::SEARCH_QUERY_MAX_CHARS
        )));
    }
    Ok(trimmed)
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
    use crate::cursor;
    use chrono::Utc;

    /// The `find` cursor round-trips through the shared opaque codec, preserving
    /// its exact `(name, id)` tuple.
    #[test]
    fn find_cursor_round_trips() {
        let value = FindCursor {
            name: "note.md".to_owned(),
            id: Uuid::new_v4(),
        };
        let encoded = cursor::encode(&value).unwrap();
        let decoded: FindCursor = cursor::decode(&encoded).unwrap();
        assert_eq!(decoded, value);
    }

    /// The `grep` cursor round-trips, preserving `(updated_at, node_id,
    /// match_offset)` exactly — including the intra-text offset.
    #[test]
    fn grep_cursor_round_trips() {
        let value = GrepCursor {
            updated_at: Utc::now(),
            node_id: Uuid::new_v4(),
            match_offset: 7,
        };
        let encoded = cursor::encode(&value).unwrap();
        let decoded: GrepCursor = cursor::decode(&encoded).unwrap();
        assert_eq!(decoded, value);
    }

    /// A garbage cursor fails to decode for both cursor types.
    #[test]
    fn garbage_cursor_fails_to_decode() {
        assert!(cursor::decode::<FindCursor>("!!!not-base64!!!").is_err());
        assert!(cursor::decode::<GrepCursor>("not-a-cursor").is_err());
    }

    #[test]
    fn invalid_queries_are_rejected() {
        assert!(matches!(
            validate_query("   "),
            Err(ServiceError::InvalidInput(_))
        ));
        assert!(matches!(
            validate_query("alpha\nbeta"),
            Err(ServiceError::InvalidInput(_))
        ));
        let too_long = "x".repeat(limits::SEARCH_QUERY_MAX_CHARS + 1);
        assert!(matches!(
            validate_query(&too_long),
            Err(ServiceError::InvalidInput(_))
        ));
        assert_eq!(validate_query("  note  ").unwrap(), "note");
    }
}
