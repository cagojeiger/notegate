//! Error mapping for the file tree repo.
//!
//! Unique/check violations from in-tx invariants (sibling-name uniqueness, the
//! name/`.md`/dotdot CHECKs, the byte/line bounds) surface as validation errors
//! so they map to `409`/`400`; everything else is an internal failure.

use notegate_core::Error;

/// Map a sibling-name unique violation or a CHECK violation to a clean
/// validation error; fall back to the generic internal mapping otherwise.
pub fn map_constraint_error(error: sqlx::Error) -> Error {
    if let sqlx::Error::Database(db_error) = &error {
        if db_error.is_unique_violation() {
            return Error::validation("a node with this name already exists in this folder");
        }
        if db_error.is_check_violation() {
            return Error::validation("the node violates a content or name constraint");
        }
    }
    map_sqlx_error(error)
}

/// Generic internal mapping for a files-repo query failure.
pub fn map_sqlx_error(error: sqlx::Error) -> Error {
    Error::internal(format!("files repository query failed: {error}"))
}
