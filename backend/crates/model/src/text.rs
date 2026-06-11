//! Text and file content metadata.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TextStorageFormat {
    Plain,
    Encrypted,
}

impl TextStorageFormat {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Plain => "plain",
            Self::Encrypted => "encrypted",
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum FileStorageKind {
    InlinePg,
    Object,
}

/// The stored content of a text node, with plaintext-derived metrics.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TextObject {
    pub node_id: Uuid,
    pub space_id: Uuid,
    pub content: Option<String>,
    pub encrypted_payload: Option<Value>,
    pub content_sha256: String,
    pub byte_len: i64,
    pub line_count: i32,
    pub media_type: String,
    pub encoding: String,
    pub storage_format: TextStorageFormat,
    pub created_by_account_id: Uuid,
    pub updated_by_account_id: Uuid,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// Binary/object file metadata. Content bytes are returned through file APIs, not text read.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct FileObject {
    pub node_id: Uuid,
    pub space_id: Uuid,
    pub storage_kind: FileStorageKind,
    pub media_type: String,
    pub byte_len: i64,
    pub content_sha256: String,
    pub original_filename: Option<String>,
    pub uploaded_at: DateTime<Utc>,
}
