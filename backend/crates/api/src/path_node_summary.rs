//! Shared path-first node output used by MCP tools and internal search transport.

use chrono::{DateTime, Utc};
use notegate_model::NodeKind;
use notegate_model::files::NodeView;
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Serialize, Deserialize)]
pub(crate) struct PathNodeSummary {
    pub path: String,
    pub name: String,
    pub kind: NodeKind,
    pub has_children: bool,
    pub sort_order: i32,
    pub search_enabled: bool,
    pub write_locked: bool,
    pub effective_write_locked: bool,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
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
    pub encryption_mode: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub original_filename: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub encryption_metadata: Option<Value>,
}

impl From<NodeView> for PathNodeSummary {
    fn from(view: NodeView) -> Self {
        Self::from(&view)
    }
}

impl From<&NodeView> for PathNodeSummary {
    fn from(view: &NodeView) -> Self {
        let mut summary = Self {
            path: view.path.clone(),
            name: view.node.name.clone(),
            kind: view.node.kind,
            has_children: view.has_children,
            sort_order: view.node.sort_order,
            search_enabled: view.node.search_enabled,
            write_locked: view.node.write_locked,
            effective_write_locked: !view.write_lock_sources.is_empty(),
            created_at: view.node.created_at,
            updated_at: view.node.updated_at,
            content_sha256: None,
            byte_len: None,
            line_count: None,
            text_storage_format: None,
            text_at_rest_encryption: None,
            media_type: None,
            encryption_mode: None,
            original_filename: None,
            encryption_metadata: None,
        };
        if let Some(text) = &view.text {
            summary.content_sha256 = Some(text.content_sha256.clone());
            summary.byte_len = Some(text.byte_len);
            summary.line_count = Some(text.line_count);
            summary.text_storage_format = Some(text.storage_format.as_str().to_owned());
            summary.text_at_rest_encryption = Some(text.at_rest_encryption.as_str().to_owned());
        }
        if let Some(file) = &view.file {
            summary.byte_len = Some(file.byte_len);
            summary.media_type = Some(file.media_type.clone());
            summary.encryption_mode = Some(file.encryption_mode.as_str().to_owned());
            summary.original_filename = file.original_filename.clone();
            summary.encryption_metadata = file.encryption_metadata.clone();
        }
        summary
    }
}

#[cfg(test)]
mod tests {
    use notegate_model::files::{FileStats, TextStats, WriteLockSource};
    use notegate_model::{FileEncryptionMode, Node, TextAtRestEncryption, TextStorageFormat};
    use serde_json::json;
    use uuid::Uuid;

    use super::*;

    fn view(kind: NodeKind) -> NodeView {
        let now = Utc::now();
        let account_id = Uuid::new_v4();
        NodeView {
            node: Node {
                id: Uuid::new_v4(),
                space_id: Uuid::new_v4(),
                parent_id: Some(Uuid::new_v4()),
                name: "entry".to_owned(),
                kind,
                sort_order: 3,
                metadata: json!({"not": "returned"}),
                search_enabled: true,
                write_locked: false,
                created_by_account_id: account_id,
                updated_by_account_id: account_id,
                deleted_by_account_id: None,
                purge_after: None,
                created_at: now,
                updated_at: now,
                deleted_at: None,
            },
            path: "/entry".to_owned(),
            has_children: kind == NodeKind::Folder,
            text: None,
            file: None,
            write_lock_sources: vec![WriteLockSource {
                node_id: Uuid::new_v4(),
                name: "locked".to_owned(),
                path: "/locked".to_owned(),
            }],
        }
    }

    #[test]
    fn folder_summary_contains_only_common_path_first_fields() {
        let view = view(NodeKind::Folder);
        let expected = json!({
            "path": "/entry",
            "name": "entry",
            "kind": "folder",
            "has_children": true,
            "sort_order": 3,
            "search_enabled": true,
            "write_locked": false,
            "effective_write_locked": true,
            "created_at": view.node.created_at,
            "updated_at": view.node.updated_at,
        });

        assert_eq!(json!(PathNodeSummary::from(view)), expected);
    }

    #[test]
    fn text_and_file_summaries_add_only_their_storage_fields() {
        let mut text = view(NodeKind::Text);
        text.text = Some(TextStats {
            content_sha256: "abc123".to_owned(),
            byte_len: 42,
            line_count: 2,
            storage_format: TextStorageFormat::Plain,
            at_rest_encryption: TextAtRestEncryption::Server,
        });
        let text = json!(PathNodeSummary::from(text));
        assert_eq!(text.get("content_sha256"), Some(&json!("abc123")));
        assert_eq!(text.get("byte_len"), Some(&json!(42)));
        assert_eq!(text.get("line_count"), Some(&json!(2)));
        assert_eq!(text.get("text_storage_format"), Some(&json!("plain")));
        assert_eq!(text.get("text_at_rest_encryption"), Some(&json!("server")));
        assert!(text.get("media_type").is_none());

        let mut file = view(NodeKind::File);
        file.file = Some(FileStats {
            media_type: "application/octet-stream".to_owned(),
            detected_media_type: Some("application/pdf".to_owned()),
            byte_len: 84,
            original_filename: Some("report.pdf".to_owned()),
            encryption_mode: FileEncryptionMode::Client,
            encryption_metadata: Some(json!({"key": "wrapped"})),
        });
        let file = json!(PathNodeSummary::from(file));
        assert_eq!(file.get("byte_len"), Some(&json!(84)));
        assert_eq!(
            file.get("media_type"),
            Some(&json!("application/octet-stream"))
        );
        assert_eq!(file.get("encryption_mode"), Some(&json!("client")));
        assert_eq!(file.get("original_filename"), Some(&json!("report.pdf")));
        assert_eq!(
            file.get("encryption_metadata"),
            Some(&json!({"key": "wrapped"}))
        );
        assert!(file.get("detected_media_type").is_none());
        assert!(file.get("content_sha256").is_none());
    }
}
