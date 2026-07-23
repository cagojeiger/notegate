//! Row types for the file tree (`nodes`) and text content (`text_objects`),
//! plus the shared column lists. There is no stored `path` column — the display
//! path is derived via a recursive CTE (see `queries`).

use chrono::{DateTime, Utc};
use notegate_core::{Error, Result};
use notegate_model::{
    FileEncryptionMode, FileObject, Node, NodeKind, TextObject, TextStorageFormat,
};
use serde_json::Value;
use sqlx::FromRow;
use uuid::Uuid;

/// A row from `nodes`.
#[derive(Debug, FromRow)]
pub struct NodeRow {
    pub id: Uuid,
    pub space_id: Uuid,
    pub parent_id: Option<Uuid>,
    pub name: String,
    pub kind: String,
    pub sort_order: i32,
    pub metadata: Value,
    pub created_by_account_id: Uuid,
    pub updated_by_account_id: Uuid,
    pub deleted_by_account_id: Option<Uuid>,
    pub purge_after: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub deleted_at: Option<DateTime<Utc>>,
}

impl NodeRow {
    /// Convert into the domain [`Node`], parsing the kind.
    pub fn into_node(self) -> Result<Node> {
        let kind = NodeKind::parse(&self.kind)
            .ok_or_else(|| Error::internal(format!("unknown node kind: {}", self.kind)))?;
        Ok(Node {
            id: self.id,
            space_id: self.space_id,
            parent_id: self.parent_id,
            name: self.name,
            kind,
            sort_order: self.sort_order,
            metadata: self.metadata,
            created_by_account_id: self.created_by_account_id,
            updated_by_account_id: self.updated_by_account_id,
            deleted_by_account_id: self.deleted_by_account_id,
            purge_after: self.purge_after,
            created_at: self.created_at,
            updated_at: self.updated_at,
            deleted_at: self.deleted_at,
        })
    }
}

/// A row from `text_objects`.
#[derive(Debug, FromRow)]
pub struct TextRow {
    pub node_id: Uuid,
    pub space_id: Uuid,
    pub content: Option<String>,
    pub encrypted_payload: Option<Value>,
    pub content_sha256: String,
    pub byte_len: i64,
    pub line_count: i32,
    pub media_type: String,
    pub encoding: String,
    pub storage_format: String,
    pub created_by_account_id: Uuid,
    pub updated_by_account_id: Uuid,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl TextRow {
    pub fn into_text(self) -> Result<TextObject> {
        let storage_format = match self.storage_format.as_str() {
            "plain" => TextStorageFormat::Plain,
            "encrypted" => TextStorageFormat::Encrypted,
            value => {
                return Err(Error::internal(format!(
                    "unknown text storage format: {value}"
                )));
            }
        };
        Ok(TextObject {
            node_id: self.node_id,
            space_id: self.space_id,
            content: self.content,
            encrypted_payload: self.encrypted_payload,
            content_sha256: self.content_sha256,
            byte_len: self.byte_len,
            line_count: self.line_count,
            media_type: self.media_type,
            encoding: self.encoding,
            storage_format,
            created_by_account_id: self.created_by_account_id,
            updated_by_account_id: self.updated_by_account_id,
            created_at: self.created_at,
            updated_at: self.updated_at,
        })
    }
}

/// Selectable columns of `nodes`, in [`NodeRow`] order.
pub const NODE_COLUMNS: &str = "id, space_id, parent_id, name, kind, sort_order, metadata, \
     created_by_account_id, updated_by_account_id, deleted_by_account_id, purge_after, created_at, updated_at, deleted_at";

/// Selectable columns of `text_objects`, in [`TextRow`] order.
pub const TEXT_COLUMNS: &str = "node_id, space_id, content_text AS content, encrypted_payload, content_sha256, \
     byte_len, line_count, media_type, encoding, storage_format, \
     created_by_account_id, updated_by_account_id, created_at, updated_at";

/// A row from `file_objects`; content bytes live in object storage.
#[derive(Debug, FromRow)]
pub struct FileRow {
    pub node_id: Uuid,
    pub space_id: Uuid,
    pub object_key: String,
    pub media_type: String,
    pub detected_media_type: Option<String>,
    pub byte_len: i64,
    pub original_filename: Option<String>,
    pub encryption_mode: String,
    pub encryption_metadata: Option<Value>,
    pub uploaded_at: DateTime<Utc>,
}

impl FileRow {
    pub fn into_file(self) -> Result<FileObject> {
        let encryption_mode =
            FileEncryptionMode::parse(&self.encryption_mode).ok_or_else(|| {
                Error::internal(format!(
                    "unknown file encryption mode: {}",
                    self.encryption_mode
                ))
            })?;
        Ok(FileObject {
            node_id: self.node_id,
            space_id: self.space_id,
            object_key: self.object_key,
            media_type: self.media_type,
            detected_media_type: self.detected_media_type,
            byte_len: self.byte_len,
            original_filename: self.original_filename,
            encryption_mode,
            encryption_metadata: self.encryption_metadata,
            uploaded_at: self.uploaded_at,
        })
    }
}

/// Selectable columns of `file_objects`, in [`FileRow`] order.
pub const FILE_COLUMNS: &str = "node_id, space_id, object_key, media_type, detected_media_type, byte_len, \
     original_filename, encryption_mode, encryption_metadata, uploaded_at";
