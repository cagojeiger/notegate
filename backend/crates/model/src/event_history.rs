//! Shared event-history read-model types.

use chrono::{DateTime, Utc};

/// Keyset cursor for event list order `(created_at DESC, id DESC)`.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct EventCursor {
    pub created_at: DateTime<Utc>,
    pub id: i64,
}
