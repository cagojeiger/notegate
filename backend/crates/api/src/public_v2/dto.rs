use chrono::{DateTime, Utc};
use notegate_model::{
    AccountKind, Caller, FileEncryptionMode, NodeKind, Permission, TextAtRestEncryption,
    TextStorageFormat,
};
use notegate_service::files::{NodeSummaryView, NodeView, WriteLockSource};
use notegate_service::spaces::SpaceView;
use serde::Serialize;
use serde_json::Value;
use utoipa::ToSchema;
use uuid::Uuid;

#[derive(Debug, Serialize, ToSchema)]
pub struct MeResponse {
    pub account: AccountOut,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct AccountOut {
    pub id: Uuid,
    pub kind: AccountKindOut,
    pub display_name: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum AccountKindOut {
    User,
    Agent,
}

impl From<AccountKind> for AccountKindOut {
    fn from(kind: AccountKind) -> Self {
        match kind {
            AccountKind::User => Self::User,
            AccountKind::Agent => Self::Agent,
        }
    }
}

impl From<&Caller> for MeResponse {
    fn from(caller: &Caller) -> Self {
        Self {
            account: AccountOut {
                id: caller.account.id,
                kind: caller.account.kind.into(),
                display_name: caller.account.display_name.clone(),
            },
        }
    }
}

#[derive(Debug, Serialize, ToSchema)]
/// Pagination metadata shared by list, tree, and search responses.
pub struct PageOut {
    /// Effective page size after server defaults and caps are applied.
    pub limit: i64,
    /// Number of items in this response.
    pub returned: usize,
    /// Whether another page is available.
    pub has_more: bool,
    /// Opaque continuation cursor. Send it unchanged with the same operation and filters.
    pub next_cursor: Option<String>,
}

impl PageOut {
    pub fn new(limit: i64, returned: usize, has_more: bool, next_cursor: Option<String>) -> Self {
        Self {
            limit,
            returned,
            has_more,
            next_cursor,
        }
    }
}

#[derive(Debug, Serialize, ToSchema)]
/// A space connected to the authenticated Agent account.
pub struct SpaceOut {
    pub id: Uuid,
    pub name: String,
    /// Effective connection permission: `read` or `write`.
    pub permission: PermissionOut,
    pub root_node_id: Uuid,
    /// Default search policy inherited by newly created nodes.
    pub default_search_enabled: bool,
    /// Default server-managed at-rest encryption policy for newly created text nodes.
    pub default_text_encryption_enabled: bool,
    pub features: SpaceFeaturesOut,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum PermissionOut {
    Read,
    Write,
}

impl From<Permission> for PermissionOut {
    fn from(permission: Permission) -> Self {
        match permission {
            Permission::Read => Self::Read,
            Permission::Write => Self::Write,
        }
    }
}

#[derive(Debug, Serialize, ToSchema)]
/// Tier-gated capabilities currently available in this space.
pub struct SpaceFeaturesOut {
    pub text_encryption: bool,
    pub write_lock: bool,
}

impl From<&SpaceView> for SpaceOut {
    fn from(view: &SpaceView) -> Self {
        Self {
            id: view.space.id,
            name: view.space.name.clone(),
            permission: view.permission.into(),
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

#[derive(Debug, Serialize, ToSchema)]
/// Complete node metadata. Kind-specific fields are omitted when not applicable.
pub struct NodeOut {
    pub id: Uuid,
    pub space_id: Uuid,
    pub parent_id: Option<Uuid>,
    pub name: String,
    /// `folder`, `text`, or `file`.
    pub kind: NodeKindOut,
    /// Canonical absolute path derived from parent relationships and names.
    pub path: String,
    pub sort_order: i32,
    pub search_enabled: bool,
    /// Whether this node itself is directly write-locked.
    pub write_locked: bool,
    /// Whether this node is write-locked directly or by an ancestor.
    pub effective_write_locked: bool,
    /// Direct lock sources that make `effective_write_locked` true.
    pub write_lock_sources: Vec<WriteLockSourceOut>,
    pub has_children: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content_sha256: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub byte_len: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub line_count: Option<i32>,
    /// Text storage representation, when kind is `text`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text_storage_format: Option<TextStorageFormatOut>,
    /// Server-managed at-rest encryption state, when kind is `text`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text_at_rest_encryption: Option<TextAtRestEncryptionOut>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub media_type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub original_filename: Option<String>,
    /// File encryption mode: `none` or `client`, when kind is `file`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub encryption_mode: Option<FileEncryptionModeOut>,
    /// Opaque client encryption metadata. Never contains the encryption key.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub encryption_metadata: Option<Value>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum NodeKindOut {
    Folder,
    Text,
    File,
}

impl From<NodeKind> for NodeKindOut {
    fn from(kind: NodeKind) -> Self {
        match kind {
            NodeKind::Folder => Self::Folder,
            NodeKind::Text => Self::Text,
            NodeKind::File => Self::File,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum TextStorageFormatOut {
    Plain,
    Encrypted,
}

impl From<TextStorageFormat> for TextStorageFormatOut {
    fn from(format: TextStorageFormat) -> Self {
        match format {
            TextStorageFormat::Plain => Self::Plain,
            TextStorageFormat::Encrypted => Self::Encrypted,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum TextAtRestEncryptionOut {
    None,
    Server,
}

impl From<TextAtRestEncryption> for TextAtRestEncryptionOut {
    fn from(encryption: TextAtRestEncryption) -> Self {
        match encryption {
            TextAtRestEncryption::None => Self::None,
            TextAtRestEncryption::Server => Self::Server,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum FileEncryptionModeOut {
    None,
    Client,
}

impl From<FileEncryptionMode> for FileEncryptionModeOut {
    fn from(mode: FileEncryptionMode) -> Self {
        match mode {
            FileEncryptionMode::None => Self::None,
            FileEncryptionMode::Client => Self::Client,
        }
    }
}

impl From<&NodeView> for NodeOut {
    fn from(view: &NodeView) -> Self {
        let node = &view.node;
        Self {
            id: node.id,
            space_id: node.space_id,
            parent_id: node.parent_id,
            name: node.name.clone(),
            kind: node.kind.into(),
            path: view.path.clone(),
            sort_order: node.sort_order,
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
            text_storage_format: view.text.as_ref().map(|text| text.storage_format.into()),
            text_at_rest_encryption: view
                .text
                .as_ref()
                .map(|text| text.at_rest_encryption.into()),
            media_type: view.file.as_ref().map(|file| file.media_type.clone()),
            original_filename: view
                .file
                .as_ref()
                .and_then(|file| file.original_filename.clone()),
            encryption_mode: view.file.as_ref().map(|file| file.encryption_mode.into()),
            encryption_metadata: view
                .file
                .as_ref()
                .and_then(|file| file.encryption_metadata.clone()),
            created_at: node.created_at,
            updated_at: node.updated_at,
        }
    }
}

#[derive(Debug, Serialize, ToSchema)]
/// Compact node representation used by tree, children, and search responses.
pub struct NodeSummaryOut {
    pub id: Uuid,
    pub parent_id: Option<Uuid>,
    pub name: String,
    pub kind: NodeKindOut,
    /// Canonical absolute path.
    pub path: String,
    pub has_children: bool,
    /// Effective direct-or-inherited write-lock state.
    pub effective_write_locked: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub byte_len: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub line_count: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub media_type: Option<String>,
    pub updated_at: DateTime<Utc>,
}

impl From<&NodeSummaryView> for NodeSummaryOut {
    fn from(view: &NodeSummaryView) -> Self {
        Self {
            id: view.node.id,
            parent_id: view.node.parent_id,
            name: view.node.name.clone(),
            kind: view.node.kind.into(),
            path: view.path.clone(),
            has_children: view.has_children,
            effective_write_locked: view.effective_write_locked,
            byte_len: view
                .text
                .as_ref()
                .map(|text| text.byte_len)
                .or_else(|| view.file.as_ref().map(|file| file.byte_len)),
            line_count: view.text.as_ref().map(|text| text.line_count),
            media_type: view.file.as_ref().map(|file| file.media_type.clone()),
            updated_at: view.node.updated_at,
        }
    }
}

impl NodeSummaryOut {
    pub fn from_view(view: &NodeView) -> Self {
        Self {
            id: view.node.id,
            parent_id: view.node.parent_id,
            name: view.node.name.clone(),
            kind: view.node.kind.into(),
            path: view.path.clone(),
            has_children: view.has_children,
            effective_write_locked: !view.write_lock_sources.is_empty(),
            byte_len: view
                .text
                .as_ref()
                .map(|text| text.byte_len)
                .or_else(|| view.file.as_ref().map(|file| file.byte_len)),
            line_count: view.text.as_ref().map(|text| text.line_count),
            media_type: view.file.as_ref().map(|file| file.media_type.clone()),
            updated_at: view.node.updated_at,
        }
    }
}

#[derive(Debug, Serialize, ToSchema)]
/// A node on the ancestor chain with a direct write lock.
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
