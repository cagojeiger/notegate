//! Shared REST data-transfer objects and the mappers from service/model types.
//!
//! These mirror the exact JSON shapes in `docs/spec/rest/README.md`
//! (AccountRef, Space output, Node output, Text output). The api layer
//! owns these so the `model`/`service` types stay transport-free; mapping here is
//! thin (no domain logic).

use std::collections::HashMap;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use uuid::Uuid;

use notegate_model::{AccountRef as ModelAccountRef, ApiKey};
use notegate_service::spaces::SpaceView;

mod events;
mod nodes;

pub(crate) use events::{
    AuditEventListResponse, AuditEventOut, FileChangeDeltaOut, FileChangeEventListResponse,
    FileChangeEventOut, FileChangeSyncResponse, McpInvocationListResponse, McpInvocationOut,
};
pub use nodes::{NodeOut, NodeRef, NodeSummaryOut, attribution_ids, parse_kind};

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

/// Agent API-key creation request.
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

/// Space output: metadata, caller permission, and derived `root_node_id`.
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct SpaceOut {
    pub id: Uuid,
    pub name: String,
    pub sort_order: i32,
    pub navigation_pinned: bool,
    pub user_mcp_enabled: bool,
    pub permission: String,
    pub root_node_id: Uuid,
    pub default_search_enabled: bool,
    pub default_text_encryption_enabled: bool,
    pub features: SpaceFeaturesOut,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct SpaceFeaturesOut {
    pub text_encryption: bool,
    pub write_lock: bool,
}

impl From<&SpaceView> for SpaceOut {
    fn from(view: &SpaceView) -> Self {
        Self {
            id: view.space.id,
            name: view.space.name.clone(),
            sort_order: view.space.sort_order,
            navigation_pinned: view.space.navigation_pinned_at.is_some(),
            user_mcp_enabled: view.space.user_mcp_enabled_at.is_some(),
            permission: view.permission.as_str().to_owned(),
            root_node_id: view.root_node_id,
            default_search_enabled: view.space.default_search_enabled,
            default_text_encryption_enabled: view.space.default_text_encryption_enabled,
            features: SpaceFeaturesOut {
                text_encryption: view.features.text_encryption,
                write_lock: view.features.write_lock,
            },
            created_at: view.space.created_at,
            updated_at: view.space.updated_at,
        }
    }
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
    use notegate_model::{
        AccountKind, AuditEvent, FileEncryptionMode, Node, NodeKind, NodeSummary,
        TextAtRestEncryption, TextStorageFormat,
    };
    use notegate_service::files::{
        FileChangeEvent, FileStats, NodeSummaryView, NodeView, TextStats, WriteLockSource,
    };
    use serde_json::json;

    use crate::file_preview::FilePreviewKind;

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
            revision: 1,
            metadata: json!({"pinned": true}),
            search_enabled: true,
            write_locked: false,
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
            write_lock_sources: Vec::new(),
        }
    }

    fn text_stats() -> TextStats {
        TextStats {
            content_sha256: "text-sha".to_owned(),
            byte_len: 42,
            line_count: 3,
            storage_format: TextStorageFormat::Plain,
            at_rest_encryption: TextAtRestEncryption::Server,
        }
    }

    fn file_stats() -> FileStats {
        FileStats {
            media_type: "image/png".to_owned(),
            detected_media_type: Some("image/png".to_owned()),
            byte_len: 1024,
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
        assert!(out.search_enabled);
        assert!(out.content_sha256.is_none());
        assert!(out.byte_len.is_none());
        assert!(out.line_count.is_none());
        assert!(out.text_storage_format.is_none());
        assert!(out.text_at_rest_encryption.is_none());
        assert!(out.media_type.is_none());
        assert!(out.original_filename.is_none());
        assert!(out.encryption_mode.is_none());
        assert!(out.encryption_metadata.is_none());
        assert!(out.preview_available.is_none());
        assert!(out.file_preview_kind.is_none());
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
        assert_eq!(out.text_storage_format.as_deref(), Some("plain"));
        assert_eq!(out.text_at_rest_encryption, Some("server".to_owned()));
        assert!(out.media_type.is_none());
        assert!(out.original_filename.is_none());
        assert!(out.encryption_mode.is_none());
        assert!(out.encryption_metadata.is_none());
        assert!(out.preview_available.is_none());
        assert!(out.file_preview_kind.is_none());
    }

    #[test]
    fn node_out_distinguishes_direct_and_inherited_write_locks() {
        let mut view = base_view(NodeKind::Text);
        let source_id = Uuid::new_v4();
        view.write_lock_sources.push(WriteLockSource {
            node_id: source_id,
            name: "locked".to_owned(),
            path: "/locked".to_owned(),
        });

        let out = NodeOut::from_view(&view, &HashMap::new());

        assert!(!out.write_locked);
        assert!(out.effective_write_locked);
        assert_eq!(out.write_lock_sources.len(), 1);
        assert_eq!(out.write_lock_sources[0].node_id, source_id);
        assert_eq!(out.write_lock_sources[0].path, "/locked");
    }

    #[test]
    fn node_summary_out_preserves_effective_write_lock() {
        let node = base_node(NodeKind::Text);
        let view = NodeSummaryView {
            node: NodeSummary {
                id: node.id,
                space_id: node.space_id,
                parent_id: node.parent_id,
                name: node.name,
                kind: node.kind,
                sort_order: node.sort_order,
                revision: node.revision,
                updated_at: node.updated_at,
            },
            path: "/locked/note.md".to_owned(),
            has_children: false,
            effective_write_locked: true,
            text: None,
            file: None,
        };

        let out = NodeSummaryOut::from(&view);

        assert_eq!(out.effective_write_locked, Some(true));
        assert_eq!(out.path, "/locked/note.md");
    }

    #[test]
    fn node_out_from_view_file_derives_fields_from_file_stats() {
        let mut view = base_view(NodeKind::File);
        view.file = Some(file_stats());
        let out = NodeOut::from_view(&view, &HashMap::new());

        assert_eq!(out.kind, "file");
        assert!(out.content_sha256.is_none());
        assert_eq!(out.byte_len, Some(1024));
        assert!(out.line_count.is_none());
        assert!(out.text_storage_format.is_none());
        assert!(out.text_at_rest_encryption.is_none());
        assert_eq!(out.media_type, Some("image/png".to_owned()));
        assert_eq!(out.detected_media_type, Some("image/png".to_owned()));
        assert_eq!(out.preview_available, Some(false));
        assert_eq!(out.file_preview_kind, None);
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
        assert_eq!(out.preview_available, Some(true));
        assert_eq!(out.file_preview_kind, Some(FilePreviewKind::Image));
    }

    #[test]
    fn node_out_disables_preview_above_the_preview_size_limit() {
        let mut view = base_view(NodeKind::File);
        let mut stats = file_stats();
        stats.encryption_mode = FileEncryptionMode::None;
        stats.byte_len = crate::file_preview::PREVIEW_MAX_BYTES + 1;
        view.file = Some(stats);

        let out = NodeOut::from_view(&view, &HashMap::new());

        assert_eq!(out.preview_available, Some(false));
        assert_eq!(out.file_preview_kind, None);
    }

    #[test]
    fn node_out_disables_oversized_legacy_preview_without_media_detection() {
        let mut view = base_view(NodeKind::File);
        let mut stats = file_stats();
        stats.encryption_mode = FileEncryptionMode::None;
        stats.byte_len = crate::file_preview::PREVIEW_MAX_BYTES + 1;
        stats.detected_media_type = None;
        view.file = Some(stats);

        let out = NodeOut::from_view(&view, &HashMap::new());

        assert_eq!(out.detected_media_type, None);
        assert_eq!(out.preview_available, Some(false));
        assert_eq!(out.file_preview_kind, None);
    }

    #[test]
    fn node_out_exposes_pdf_preview_kind_without_image_preview() {
        let mut view = base_view(NodeKind::File);
        let mut stats = file_stats();
        stats.encryption_mode = FileEncryptionMode::None;
        stats.media_type = "application/pdf".to_owned();
        stats.detected_media_type = Some("application/pdf".to_owned());
        view.file = Some(stats);

        let out = NodeOut::from_view(&view, &HashMap::new());

        assert_eq!(out.preview_available, Some(false));
        assert_eq!(out.file_preview_kind, Some(FilePreviewKind::Pdf));
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
