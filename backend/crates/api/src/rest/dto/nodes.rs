use std::collections::HashMap;

use chrono::{DateTime, Utc};
use notegate_model::{AccountRef as ModelAccountRef, FileEncryptionMode, NodeKind};
use notegate_service::files::{NodeSummaryView, NodeView, WriteLockSource};
use serde::Serialize;
use serde_json::Value;
use utoipa::ToSchema;
use uuid::Uuid;

use super::AccountRef;
use crate::file_preview::{
    FileMediaKind, FilePreviewKind, file_media_kind, file_preview_kind, is_preview_size_allowed,
    is_previewable_image_type,
};

/// Node output: tree metadata, derived `path`, and attribution refs.
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct NodeOut {
    pub id: Uuid,
    pub space_id: Uuid,
    pub parent_id: Option<Uuid>,
    pub name: String,
    pub kind: String,
    pub path: String,
    pub sort_order: i32,
    pub metadata: Value,
    pub search_enabled: bool,
    pub write_locked: bool,
    pub effective_write_locked: bool,
    pub write_lock_sources: Vec<WriteLockSourceOut>,
    pub has_children: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content_sha256: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub byte_len: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub line_count: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text_storage_format: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text_at_rest_encryption: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub media_type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detected_media_type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub preview_available: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub file_preview_kind: Option<FilePreviewKind>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub file_media_kind: Option<FileMediaKind>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub original_filename: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub encryption_mode: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub encryption_metadata: Option<Value>,
    pub created_by: AccountRef,
    pub updated_by: AccountRef,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl NodeOut {
    /// Map a [`NodeView`] to output, resolving attribution from a batch-loaded
    /// account map.
    pub fn from_view(view: &NodeView, refs: &HashMap<Uuid, ModelAccountRef>) -> Self {
        let node = &view.node;
        Self {
            id: node.id,
            space_id: node.space_id,
            parent_id: node.parent_id,
            name: node.name.clone(),
            kind: node.kind.as_str().to_owned(),
            path: view.path.clone(),
            sort_order: node.sort_order,
            metadata: node.metadata.clone(),
            search_enabled: node.search_enabled,
            write_locked: node.write_locked,
            effective_write_locked: !view.write_lock_sources.is_empty(),
            write_lock_sources: view
                .write_lock_sources
                .iter()
                .map(WriteLockSourceOut::from)
                .collect(),
            has_children: view.has_children,
            content_sha256: view.text.as_ref().map(|text| text.content_sha256.clone()),
            byte_len: view
                .text
                .as_ref()
                .map(|text| text.byte_len)
                .or_else(|| view.file.as_ref().map(|file| file.byte_len)),
            line_count: view.text.as_ref().map(|text| text.line_count),
            text_storage_format: view
                .text
                .as_ref()
                .map(|text| text.storage_format.as_str().to_owned()),
            text_at_rest_encryption: view
                .text
                .as_ref()
                .map(|text| text.at_rest_encryption.as_str().to_owned()),
            media_type: view.file.as_ref().map(|file| file.media_type.clone()),
            detected_media_type: view
                .file
                .as_ref()
                .and_then(|file| file.detected_media_type.clone()),
            preview_available: view.file.as_ref().and_then(|file| {
                if file.encryption_mode != FileEncryptionMode::None
                    || !is_preview_size_allowed(file.byte_len)
                {
                    return Some(false);
                }
                file.detected_media_type
                    .as_deref()
                    .map(is_previewable_image_type)
            }),
            file_preview_kind: view.file.as_ref().and_then(|file| {
                file_preview_kind(
                    file.byte_len,
                    file.encryption_mode,
                    file.detected_media_type.as_deref(),
                )
            }),
            file_media_kind: view
                .file
                .as_ref()
                .map(|file| file_media_kind(&file.media_type, file.detected_media_type.as_deref())),
            original_filename: view
                .file
                .as_ref()
                .and_then(|file| file.original_filename.clone()),
            encryption_mode: view
                .file
                .as_ref()
                .map(|file| file.encryption_mode.as_str().to_owned()),
            encryption_metadata: view
                .file
                .as_ref()
                .and_then(|file| file.encryption_metadata.clone()),
            created_by: AccountRef::resolve(node.created_by_account_id, refs),
            updated_by: AccountRef::resolve(node.updated_by_account_id, refs),
            created_at: node.created_at,
            updated_at: node.updated_at,
        }
    }
}

#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct WriteLockSourceOut {
    pub node_id: Uuid,
    pub name: String,
    pub path: String,
}

impl From<&WriteLockSource> for WriteLockSourceOut {
    fn from(source: &WriteLockSource) -> Self {
        Self {
            node_id: source.node_id,
            name: source.name.clone(),
            path: source.path.clone(),
        }
    }
}

/// Compact node output for paginated tree and list collections.
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct NodeSummaryOut {
    pub id: Uuid,
    pub parent_id: Option<Uuid>,
    pub name: String,
    pub kind: String,
    pub path: String,
    pub has_children: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub effective_write_locked: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub byte_len: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub line_count: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub preview_available: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub file_preview_kind: Option<FilePreviewKind>,
    pub updated_at: DateTime<Utc>,
}

impl From<&NodeSummaryView> for NodeSummaryOut {
    fn from(view: &NodeSummaryView) -> Self {
        let node = &view.node;
        Self {
            id: node.id,
            parent_id: node.parent_id,
            name: node.name.clone(),
            kind: node.kind.as_str().to_owned(),
            path: view.path.clone(),
            has_children: view.has_children,
            effective_write_locked: view.effective_write_locked.then_some(true),
            byte_len: view
                .text
                .as_ref()
                .map(|text| text.byte_len)
                .or_else(|| view.file.as_ref().map(|file| file.byte_len)),
            line_count: view.text.as_ref().map(|text| text.line_count),
            preview_available: view.file.as_ref().and_then(|file| {
                if file.encryption_mode != FileEncryptionMode::None
                    || !is_preview_size_allowed(file.byte_len)
                {
                    return Some(false);
                }
                file.detected_media_type
                    .as_deref()
                    .map(is_previewable_image_type)
            }),
            file_preview_kind: view.file.as_ref().and_then(|file| {
                file_preview_kind(
                    file.byte_len,
                    file.encryption_mode,
                    file.detected_media_type.as_deref(),
                )
            }),
            updated_at: node.updated_at,
        }
    }
}

/// The condensed node reference embedded in `children` and `text` responses
/// (`{id, path}` plus kind where the spec shows it).
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct NodeRef {
    pub id: Uuid,
    pub path: String,
    pub kind: String,
}

impl From<&NodeView> for NodeRef {
    fn from(view: &NodeView) -> Self {
        Self {
            id: view.node.id,
            path: view.path.clone(),
            kind: view.node.kind.as_str().to_owned(),
        }
    }
}

/// Collect the distinct `created_by`/`updated_by` account ids referenced by a set
/// of node views, for a single batched [`AccountRef`] resolution.
pub fn attribution_ids<'a>(views: impl IntoIterator<Item = &'a NodeView>) -> Vec<Uuid> {
    let mut ids = Vec::new();
    for view in views {
        for id in [
            view.node.created_by_account_id,
            view.node.updated_by_account_id,
        ] {
            if !ids.contains(&id) {
                ids.push(id);
            }
        }
    }
    ids
}

/// Parse a `kind` query/body string into a [`NodeKind`], rejecting unknowns.
pub fn parse_kind(value: &str) -> Result<NodeKind, crate::error::ApiError> {
    NodeKind::parse(value).ok_or_else(|| {
        crate::error::ApiError::invalid_field("kind must be 'folder', 'text', or 'file'")
    })
}
