//! API key credential metadata shared by user and agent accounts.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ApiKey {
    pub id: Uuid,
    pub account_id: Uuid,
    pub token_hash: String,
    pub name: String,
    pub scopes: Vec<String>,
    pub created_by: Option<Uuid>,
    pub created_at: DateTime<Utc>,
    pub last_used_at: Option<DateTime<Utc>>,
    pub expires_at: Option<DateTime<Utc>>,
    pub revoked_at: Option<DateTime<Utc>>,
    pub revoked_by: Option<Uuid>,
    pub revoked_reason: Option<String>,
    pub rotated_from_key_id: Option<Uuid>,
}

#[derive(Debug, Clone)]
pub struct CreateApiKey {
    pub name: String,
    pub scopes: Vec<String>,
    pub expires_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Default)]
pub struct ListApiKeys {
    pub limit: Option<i64>,
    pub cursor: Option<String>,
}

/// Keyset cursor for API key metadata list order `(created_at DESC, id DESC)`.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ApiKeyCursor {
    pub created_at: DateTime<Utc>,
    pub id: Uuid,
}

#[derive(Debug, Clone)]
pub struct ApiKeyPage {
    pub items: Vec<ApiKey>,
    pub limit: i64,
    pub has_more: bool,
    pub next_cursor: Option<String>,
}

#[derive(Debug, Clone)]
pub struct MintedApiKey {
    pub key: ApiKey,
    pub token: String,
}
