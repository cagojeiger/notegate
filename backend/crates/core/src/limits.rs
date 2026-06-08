//! Product limits for notegate.
//!
//! Every create/list/search/read/subtree operation must be bounded. Most limits
//! are fixed product constants. Expensive workspace count caps can be overridden
//! through [`crate::Config`] for E2E/dev testing while keeping these spec
//! defaults.
//!
//! These are product limits, not security boundaries; authorization still
//! checks every request.

// --- Account, workspace, and credential limits ---

/// Maximum workspaces a single user owner account may own.
pub const OWNED_WORKSPACES_MAX: usize = 20;
/// Maximum active accounts that may have access to one workspace.
pub const WORKSPACE_ACCESS_MAX_ACCOUNTS: usize = 20;
/// Maximum active agents a single user creator account may create.
pub const AGENTS_PER_CREATOR_MAX: usize = 50;
/// Maximum live keys per agent.
pub const AGENT_KEYS_PER_AGENT_MAX: usize = 10;

// --- Path and name limits ---

/// Maximum workspace name length, in characters.
pub const WORKSPACE_NAME_MAX_LEN: usize = 63;
/// Maximum folder name length, in characters.
pub const FOLDER_NAME_MAX_LEN: usize = 128;
/// Maximum document file name length (including `.md`), in characters.
pub const DOCUMENT_FILE_NAME_MAX_LEN: usize = 128;
/// Maximum document title stem length (excluding `.md`), in characters.
pub const DOCUMENT_TITLE_STEM_MAX_LEN: usize = 125;
/// Maximum derived path length, in bytes.
pub const MAX_PATH_LEN: usize = 645;
/// Maximum path depth, in segments below the workspace root.
pub const MAX_PATH_DEPTH: usize = 5;
/// Maximum live nodes per workspace.
pub const WORKSPACE_MAX_NODES: usize = 10_000;
/// Maximum live documents per workspace.
pub const WORKSPACE_MAX_DOCUMENTS: usize = 5_000;
/// Maximum total live document bytes per workspace (256 MiB).
pub const WORKSPACE_MAX_DOCUMENT_BYTES: usize = 268_435_456;

// --- Listing and folder fanout limits ---

/// Maximum live direct children per folder.
pub const FOLDER_MAX_CHILDREN: usize = 200;

/// Runtime-overridable workspace/file-tree capacity limits.
///
/// These defaults are the product contract. Tests and local E2E runs may lower
/// them through `Config.limits`; code should receive a [`Limits`] value instead
/// of reading process environment directly.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Limits {
    pub workspace_max_nodes: usize,
    pub workspace_max_documents: usize,
    pub workspace_max_document_bytes: usize,
    pub folder_max_children: usize,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            workspace_max_nodes: WORKSPACE_MAX_NODES,
            workspace_max_documents: WORKSPACE_MAX_DOCUMENTS,
            workspace_max_document_bytes: WORKSPACE_MAX_DOCUMENT_BYTES,
            folder_max_children: FOLDER_MAX_CHILDREN,
        }
    }
}

/// Default children listing page size.
pub const CHILDREN_DEFAULT_LIMIT: i64 = 100;
/// Maximum children listing page size.
pub const CHILDREN_MAX_LIMIT: i64 = 200;

// --- Search limits ---

/// Default `find` page size.
pub const FIND_DEFAULT_LIMIT: i64 = 50;
/// Maximum `find` page size.
pub const FIND_MAX_LIMIT: i64 = 100;
/// Default `grep` page size.
pub const GREP_DEFAULT_LIMIT: i64 = 20;
/// Maximum `grep` page size.
pub const GREP_MAX_LIMIT: i64 = 100;
/// Default `grep` context lines.
pub const GREP_DEFAULT_CONTEXT: i64 = 2;
/// Maximum `grep` context lines.
pub const GREP_MAX_CONTEXT: i64 = 5;
/// Maximum search query length in Unicode scalar values.
pub const SEARCH_QUERY_MAX_CHARS: usize = 256;

// --- Read limits ---

/// Default maximum lines returned by `read`/`open`.
pub const READ_DEFAULT_MAX_LINES: i64 = 200;
/// Maximum lines returned by `read`/`open`.
pub const READ_MAX_LINES: i64 = 1_000;
/// Default maximum bytes returned by `read`/`open` (64 KiB).
pub const READ_DEFAULT_MAX_BYTES: usize = 65_536;
/// Maximum bytes returned by `read`/`open` (256 KiB).
pub const READ_MAX_BYTES: usize = 262_144;

// --- Document creation and write limits ---

/// Maximum bytes per document (512 KiB).
pub const DOCUMENT_MAX_BYTES: usize = 524_288;
/// Maximum lines per document.
pub const DOCUMENT_MAX_LINES: usize = 2_000;

// --- Subtree mutation limits ---

/// Maximum nodes a synchronous folder delete may touch.
pub const SUBTREE_DELETE_MAX_NODES: usize = 1_000;
/// Days a deleted node is retained before it is eligible for hard purge.
pub const DELETED_NODE_RETENTION_DAYS: i64 = 30;

// --- API pagination limits ---

/// Default `GET /workspaces` page size.
pub const WORKSPACES_DEFAULT_LIMIT: i64 = 50;
/// Maximum `GET /workspaces` page size.
pub const WORKSPACES_MAX_LIMIT: i64 = 100;
/// Default `GET /workspaces/{id}/access` page size.
pub const ACCESS_DEFAULT_LIMIT: i64 = 100;
/// Maximum `GET /workspaces/{id}/access` page size.
pub const ACCESS_MAX_LIMIT: i64 = 100;
/// Default `GET /agents` page size.
pub const AGENTS_DEFAULT_LIMIT: i64 = 100;
/// Maximum `GET /agents` page size.
pub const AGENTS_MAX_LIMIT: i64 = 100;
