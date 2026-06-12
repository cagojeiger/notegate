//! OAuth detail for a user account. `id` equals the owning `accounts.id`.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// The user-specific OAuth identity returned to internal callers.
///
/// Provider subject and email lookup hashes are storage-only security details.
/// They are intentionally not exposed through the model or `/me` output.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct User {
    /// Equal to the owning `accounts.id`.
    pub id: Uuid,
    pub email: Option<String>,
    pub tier: String,
    pub anonymized_at: Option<DateTime<Utc>>,
}
