//! Shared REST data-transfer objects and the mappers from service/model types.
//!
//! These mirror the exact JSON shapes in `docs/spec/rest/README.md`
//! (AccountRef, Space output, Node output, Text output). The api layer
//! owns these so the `model`/`service` types stay transport-free; mapping here is
//! thin (no domain logic).

use std::collections::HashMap;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use utoipa::ToSchema;
use uuid::Uuid;

use notegate_model::{AccountRef as ModelAccountRef, ApiKey, AuditEvent, NodeKind};
use notegate_service::files::{FileChangeEvent, NodeView};
use notegate_service::spaces::SpaceView;

/// A lightweight account reference: `{id, kind, display_name}`.
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct AccountRef {
    pub id: Uuid,
    pub kind: String,
    pub display_name: String,
}

impl From<&ModelAccountRef> for AccountRef {
    fn from(value: &ModelAccountRef) -> Self {
        Self {
            id: value.id,
            kind: value.kind.as_str().to_owned(),
            display_name: value.display_name.clone(),
        }
    }
}

impl AccountRef {
    /// A placeholder ref for an account id that could not be resolved (e.g. a
    /// hard-deleted account still referenced by an attribution column). Keeps the
    /// shape stable rather than failing the whole response.
    pub fn unknown(id: Uuid) -> Self {
        Self {
            id,
            kind: "user".to_owned(),
            display_name: String::new(),
        }
    }

    /// Resolve an id against a batch-loaded account map, falling back to a
    /// placeholder when absent.
    pub fn resolve(id: Uuid, refs: &HashMap<Uuid, ModelAccountRef>) -> Self {
        refs.get(&id)
            .map(AccountRef::from)
            .unwrap_or_else(|| AccountRef::unknown(id))
    }
}

/// API-key creation request shared by user and agent key endpoints.
#[derive(Debug, Deserialize, ToSchema)]
pub(crate) struct CreateApiKeyBody {
    pub name: String,
    #[serde(default)]
    pub scopes: Vec<String>,
    pub expires_at: DateTime<Utc>,
}

/// API-key metadata returned by key list endpoints. The plaintext token is never
/// included here.
#[derive(Debug, Serialize, ToSchema)]
pub(crate) struct ApiKeyMetadataOut {
    pub id: Uuid,
    pub account_id: Uuid,
    pub name: String,
    pub scopes: Vec<String>,
    pub expires_at: DateTime<Utc>,
    pub created_at: DateTime<Utc>,
    pub revoked_at: Option<DateTime<Utc>>,
}

impl From<&ApiKey> for ApiKeyMetadataOut {
    fn from(key: &ApiKey) -> Self {
        Self {
            id: key.id,
            account_id: key.account_id,
            name: key.name.clone(),
            scopes: key.scopes.clone(),
            expires_at: key.expires_at,
            created_at: key.created_at,
            revoked_at: key.revoked_at,
        }
    }
}

#[derive(Debug, Serialize, ToSchema)]
pub(crate) struct ApiKeyMetadataListResponse {
    pub keys: Vec<ApiKeyMetadataOut>,
    pub page: crate::page::Page,
}

/// Audit event history entry returned by `GET /api/v1/me/audit-events`.
#[derive(Debug, Serialize, ToSchema)]
pub(crate) struct AuditEventOut {
    pub id: i64,
    pub created_at: DateTime<Utc>,
    pub actor_account_id: Option<Uuid>,
    pub actor: Option<AccountRef>,
    pub source: String,
    pub op_type: String,
    pub resource_type: String,
    pub resource_id: Option<Uuid>,
    pub metadata: Value,
}

impl AuditEventOut {
    pub(crate) fn from_event(event: &AuditEvent, refs: &HashMap<Uuid, ModelAccountRef>) -> Self {
        Self {
            id: event.id,
            created_at: event.created_at,
            actor_account_id: event.actor_account_id,
            actor: event
                .actor_account_id
                .and_then(|id| refs.get(&id).map(AccountRef::from)),
            source: event.source.clone(),
            op_type: event.op_type.clone(),
            resource_type: event.resource_type.clone(),
            resource_id: event.resource_id,
            metadata: event.metadata.clone(),
        }
    }
}

#[derive(Debug, Serialize, ToSchema)]
pub(crate) struct AuditEventListResponse {
    pub events: Vec<AuditEventOut>,
    pub page: crate::page::Page,
}

/// File change event history entry returned by `GET /api/v1/spaces/{space_id}/file-change-events`.
#[derive(Debug, Serialize, ToSchema)]
pub(crate) struct FileChangeEventOut {
    pub id: i64,
    pub created_at: DateTime<Utc>,
    pub space_id: Uuid,
    pub node_id: Option<Uuid>,
    pub actor_account_id: Option<Uuid>,
    pub actor: Option<AccountRef>,
    pub op_type: String,
    pub metadata: Value,
}

impl FileChangeEventOut {
    pub(crate) fn from_event(
        event: &FileChangeEvent,
        refs: &HashMap<Uuid, ModelAccountRef>,
    ) -> Self {
        Self {
            id: event.id,
            created_at: event.created_at,
            space_id: event.space_id,
            node_id: event.node_id,
            actor_account_id: event.actor_account_id,
            actor: event
                .actor_account_id
                .and_then(|id| refs.get(&id).map(AccountRef::from)),
            op_type: event.op_type.clone(),
            metadata: event.metadata.clone(),
        }
    }
}

#[derive(Debug, Serialize, ToSchema)]
pub(crate) struct FileChangeEventListResponse {
    pub events: Vec<FileChangeEventOut>,
    pub page: crate::page::Page,
}

/// Space output: metadata, caller permission, and derived `root_node_id`.
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct SpaceOut {
    pub id: Uuid,
    pub name: String,
    pub sort_order: i32,
    pub permission: String,
    pub root_node_id: Uuid,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl From<&SpaceView> for SpaceOut {
    fn from(view: &SpaceView) -> Self {
        Self {
            id: view.space.id,
            name: view.space.name.clone(),
            sort_order: view.space.sort_order,
            permission: view.permission.as_str().to_owned(),
            root_node_id: view.root_node_id,
            created_at: view.space.created_at,
            updated_at: view.space.updated_at,
        }
    }
}

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
    pub has_children: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content_sha256: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub byte_len: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub line_count: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub storage_kind: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub media_type: Option<String>,
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
            has_children: view.has_children,
            content_sha256: view
                .text
                .as_ref()
                .map(|text| text.content_sha256.clone())
                .or_else(|| view.file.as_ref().map(|file| file.content_sha256.clone())),
            byte_len: view
                .text
                .as_ref()
                .map(|text| text.byte_len)
                .or_else(|| view.file.as_ref().map(|file| file.byte_len)),
            line_count: view.text.as_ref().map(|text| text.line_count),
            storage_kind: view
                .file
                .as_ref()
                .map(|file| file.storage_kind.as_str().to_owned()),
            media_type: view.file.as_ref().map(|file| file.media_type.clone()),
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

#[cfg(test)]
mod tests {
    #![allow(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::indexing_slicing,
        clippy::panic,
        clippy::unwrap_in_result
    )]
    use notegate_model::{AccountKind, FileEncryptionMode, FileStorageKind, Node};
    use notegate_service::files::{FileStats, TextStats};
    use serde_json::json;

    use super::*;

    fn base_node(kind: NodeKind) -> Node {
        let now = Utc::now();
        Node {
            id: Uuid::new_v4(),
            space_id: Uuid::new_v4(),
            parent_id: Some(Uuid::new_v4()),
            name: "note.md".to_owned(),
            kind,
            sort_order: 0,
            metadata: json!({"pinned": true}),
            created_by_account_id: Uuid::new_v4(),
            updated_by_account_id: Uuid::new_v4(),
            deleted_by_account_id: None,
            purge_after: None,
            created_at: now,
            updated_at: now,
            deleted_at: None,
        }
    }

    fn base_view(kind: NodeKind) -> NodeView {
        NodeView {
            node: base_node(kind),
            path: "/docs/note.md".to_owned(),
            has_children: false,
            text: None,
            file: None,
        }
    }

    fn text_stats() -> TextStats {
        TextStats {
            content_sha256: "text-sha".to_owned(),
            byte_len: 42,
            line_count: 3,
        }
    }

    fn file_stats() -> FileStats {
        FileStats {
            storage_kind: FileStorageKind::Object,
            media_type: "image/png".to_owned(),
            byte_len: 1024,
            content_sha256: "file-sha".to_owned(),
            original_filename: Some("photo.png".to_owned()),
            encryption_mode: FileEncryptionMode::Client,
            encryption_metadata: Some(json!({"iv": "abc"})),
        }
    }

    #[test]
    fn node_out_from_view_folder_has_no_content_fields() {
        let view = base_view(NodeKind::Folder);
        let out = NodeOut::from_view(&view, &HashMap::new());

        assert_eq!(out.kind, "folder");
        assert_eq!(out.path, "/docs/note.md");
        assert!(out.content_sha256.is_none());
        assert!(out.byte_len.is_none());
        assert!(out.line_count.is_none());
        assert!(out.storage_kind.is_none());
        assert!(out.media_type.is_none());
        assert!(out.original_filename.is_none());
        assert!(out.encryption_mode.is_none());
        assert!(out.encryption_metadata.is_none());
    }

    #[test]
    fn node_out_from_view_text_derives_fields_from_text_stats() {
        let mut view = base_view(NodeKind::Text);
        view.text = Some(text_stats());
        let out = NodeOut::from_view(&view, &HashMap::new());

        assert_eq!(out.kind, "text");
        assert_eq!(out.content_sha256, Some("text-sha".to_owned()));
        assert_eq!(out.byte_len, Some(42));
        assert_eq!(out.line_count, Some(3));
        // Text-only fields stay unset for a text node.
        assert!(out.storage_kind.is_none());
        assert!(out.media_type.is_none());
        assert!(out.original_filename.is_none());
        assert!(out.encryption_mode.is_none());
        assert!(out.encryption_metadata.is_none());
    }

    #[test]
    fn node_out_from_view_file_derives_fields_from_file_stats() {
        let mut view = base_view(NodeKind::File);
        view.file = Some(file_stats());
        let out = NodeOut::from_view(&view, &HashMap::new());

        assert_eq!(out.kind, "file");
        assert_eq!(out.content_sha256, Some("file-sha".to_owned()));
        assert_eq!(out.byte_len, Some(1024));
        // line_count is text-only; a file never has one.
        assert!(out.line_count.is_none());
        assert_eq!(out.storage_kind, Some("object".to_owned()));
        assert_eq!(out.media_type, Some("image/png".to_owned()));
        assert_eq!(out.original_filename, Some("photo.png".to_owned()));
        assert_eq!(out.encryption_mode, Some("client".to_owned()));
        assert_eq!(out.encryption_metadata, Some(json!({"iv": "abc"})));
    }

    #[test]
    fn node_out_from_view_file_without_original_filename_or_encryption_metadata() {
        let mut view = base_view(NodeKind::File);
        let mut stats = file_stats();
        stats.original_filename = None;
        stats.encryption_metadata = None;
        stats.encryption_mode = FileEncryptionMode::None;
        view.file = Some(stats);
        let out = NodeOut::from_view(&view, &HashMap::new());

        assert!(out.original_filename.is_none());
        assert!(out.encryption_metadata.is_none());
        assert_eq!(out.encryption_mode, Some("none".to_owned()));
    }

    #[test]
    fn node_out_from_view_resolves_attribution_from_refs_map() {
        let view = base_view(NodeKind::Folder);
        let created_by = ModelAccountRef {
            id: view.node.created_by_account_id,
            kind: AccountKind::User,
            display_name: "Creator".to_owned(),
        };
        let updated_by = ModelAccountRef {
            id: view.node.updated_by_account_id,
            kind: AccountKind::Agent,
            display_name: "Updater Bot".to_owned(),
        };
        let mut refs = HashMap::new();
        refs.insert(created_by.id, created_by);
        refs.insert(updated_by.id, updated_by);

        let out = NodeOut::from_view(&view, &refs);

        assert_eq!(out.created_by.display_name, "Creator");
        assert_eq!(out.created_by.kind, "user");
        assert_eq!(out.updated_by.display_name, "Updater Bot");
        assert_eq!(out.updated_by.kind, "agent");
    }

    #[test]
    fn node_out_from_view_falls_back_to_unknown_account_for_missing_refs() {
        let view = base_view(NodeKind::Folder);
        let out = NodeOut::from_view(&view, &HashMap::new());

        assert_eq!(out.created_by.id, view.node.created_by_account_id);
        assert_eq!(out.created_by.kind, "user");
        assert_eq!(out.created_by.display_name, "");
        assert_eq!(out.updated_by.id, view.node.updated_by_account_id);
        assert_eq!(out.updated_by.display_name, "");
    }

    #[test]
    fn node_ref_from_node_view_maps_id_path_kind() {
        let view = base_view(NodeKind::Text);
        let node_ref = NodeRef::from(&view);

        assert_eq!(node_ref.id, view.node.id);
        assert_eq!(node_ref.path, "/docs/note.md");
        assert_eq!(node_ref.kind, "text");
    }

    #[test]
    fn attribution_ids_collects_distinct_ids_preserving_first_seen_order() {
        let shared_id = Uuid::new_v4();
        let mut first = base_view(NodeKind::Folder);
        first.node.created_by_account_id = shared_id;
        first.node.updated_by_account_id = shared_id;
        let mut second = base_view(NodeKind::Text);
        let other_id = second.node.created_by_account_id;
        second.node.updated_by_account_id = shared_id;

        let ids = attribution_ids([&first, &second]);

        assert_eq!(ids, vec![shared_id, other_id]);
    }

    #[test]
    fn parse_kind_accepts_known_values() {
        assert_eq!(parse_kind("folder").unwrap(), NodeKind::Folder);
        assert_eq!(parse_kind("text").unwrap(), NodeKind::Text);
        assert_eq!(parse_kind("file").unwrap(), NodeKind::File);
    }

    #[test]
    fn parse_kind_rejects_unknown_value() {
        assert!(parse_kind("bogus").is_err());
    }

    #[test]
    fn account_ref_from_model_account_ref_copies_fields() {
        let model_ref = ModelAccountRef {
            id: Uuid::new_v4(),
            kind: AccountKind::Agent,
            display_name: "Agent Smith".to_owned(),
        };
        let out = AccountRef::from(&model_ref);

        assert_eq!(out.id, model_ref.id);
        assert_eq!(out.kind, "agent");
        assert_eq!(out.display_name, "Agent Smith");
    }

    #[test]
    fn account_ref_unknown_uses_placeholder_shape() {
        let id = Uuid::new_v4();
        let out = AccountRef::unknown(id);

        assert_eq!(out.id, id);
        assert_eq!(out.kind, "user");
        assert_eq!(out.display_name, "");
    }

    #[test]
    fn audit_event_out_from_audit_event_maps_all_fields() {
        let event = AuditEvent {
            id: 7,
            created_at: Utc::now(),
            actor_account_id: Some(Uuid::new_v4()),
            source: "web".to_owned(),
            op_type: "space.create".to_owned(),
            resource_type: "space".to_owned(),
            resource_id: Some(Uuid::new_v4()),
            metadata: json!({"name": "personal"}),
        };

        let actor_id = event.actor_account_id.expect("actor id");
        let refs = HashMap::from([(
            actor_id,
            ModelAccountRef {
                id: actor_id,
                kind: AccountKind::User,
                display_name: "Audit User".to_owned(),
            },
        )]);
        let out = AuditEventOut::from_event(&event, &refs);

        assert_eq!(out.id, event.id);
        assert_eq!(out.created_at, event.created_at);
        assert_eq!(out.actor_account_id, event.actor_account_id);
        assert_eq!(
            out.actor.as_ref().map(|actor| actor.id),
            event.actor_account_id
        );
        assert_eq!(out.source, event.source);
        assert_eq!(out.op_type, event.op_type);
        assert_eq!(out.resource_type, event.resource_type);
        assert_eq!(out.resource_id, event.resource_id);
        assert_eq!(out.metadata, event.metadata);
    }

    #[test]
    fn file_change_event_out_from_file_change_event_maps_all_fields() {
        let event = FileChangeEvent {
            id: 11,
            created_at: Utc::now(),
            space_id: Uuid::new_v4(),
            node_id: Some(Uuid::new_v4()),
            actor_account_id: Some(Uuid::new_v4()),
            op_type: "text.write".to_owned(),
            metadata: json!({"byte_len_after": 5}),
        };

        let actor_id = event.actor_account_id.expect("actor id");
        let refs = HashMap::from([(
            actor_id,
            ModelAccountRef {
                id: actor_id,
                kind: AccountKind::Agent,
                display_name: "File Agent".to_owned(),
            },
        )]);
        let out = FileChangeEventOut::from_event(&event, &refs);

        assert_eq!(out.id, event.id);
        assert_eq!(out.created_at, event.created_at);
        assert_eq!(out.space_id, event.space_id);
        assert_eq!(out.node_id, event.node_id);
        assert_eq!(out.actor_account_id, event.actor_account_id);
        assert_eq!(
            out.actor.as_ref().map(|actor| actor.id),
            event.actor_account_id
        );
        assert_eq!(out.op_type, event.op_type);
        assert_eq!(out.metadata, event.metadata);
    }
}
