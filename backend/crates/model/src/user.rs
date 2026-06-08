//! OAuth detail for a user account. `id` equals the owning `accounts.id`.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// The user-specific OAuth identity. `sub`/`email` become `NULL` on
/// anonymization, recorded by `anonymized_at`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct User {
    /// Equal to the owning `accounts.id`.
    pub id: Uuid,
    pub sub: Option<String>,
    pub email: Option<String>,
    pub anonymized_at: Option<DateTime<Utc>>,
}
