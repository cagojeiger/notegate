//! Product limits for notegate.
//!
//! Every create/list/search/read/subtree operation must be bounded. Most limits
//! are fixed hard-max product constants. A later tier system may choose lower
//! effective quotas per account, but it must not exceed these core maxima.
//! Expensive file-tree capacity caps can be lowered through [`crate::Config`]
//! for E2E/dev testing while keeping these spec maxima.
//!
//! These are product limits, not security boundaries; authorization still
//! checks every request.

// --- HTTP ingress limits ---

/// Maximum HTTP request body size accepted by the API server (1 MiB).
pub const HTTP_REQUEST_BODY_MAX_BYTES: usize = 1_048_576;
/// Maximum wall-clock time for one HTTP request before a 408 response.
pub const HTTP_REQUEST_TIMEOUT_SECS: u64 = 30;
/// Maximum wall-clock time for control-plane probes before a 408 response.
pub const HTTP_CONTROL_PLANE_TIMEOUT_SECS: u64 = 5;
/// Maximum HTTP requests accepted per API process per minute.
pub const HTTP_RATE_LIMIT_REQUESTS_PER_MINUTE: u32 = 1_800;

// --- Account, workspace, and credential limits ---

/// Maximum workspaces a single user owner account may own.
pub const OWNED_WORKSPACES_MAX: usize = 20;
/// Maximum live workspaces a single user or agent account may access.
pub const ACCESSIBLE_WORKSPACES_PER_ACCOUNT_MAX: usize = 100;
/// Maximum active accounts that may have access to one workspace.
pub const WORKSPACE_ACCESS_MAX_ACCOUNTS: usize = 20;
/// Maximum active agents a single user creator account may create.
pub const AGENTS_PER_CREATOR_MAX: usize = 50;
/// Maximum live API keys for a user account.
pub const USER_API_KEYS_PER_ACCOUNT_MAX: usize = 2;
/// Maximum live API keys for an agent account.
pub const AGENT_API_KEYS_PER_ACCOUNT_MAX: usize = 5;
/// Maximum user API-key lifetime in days.
pub const USER_API_KEY_MAX_TTL_DAYS: i64 = 30;
/// Maximum agent API-key lifetime in days.
pub const AGENT_API_KEY_MAX_TTL_DAYS: i64 = 365;

// --- Identity, path, and name limits ---

/// Maximum OAuth provider subject length, in characters.
pub const OAUTH_PROVIDER_SUB_MAX_CHARS: usize = 255;
/// Maximum user display name length, in characters.
pub const USER_DISPLAY_NAME_MAX_CHARS: usize = 128;
/// Maximum user email length, in characters.
pub const USER_EMAIL_MAX_CHARS: usize = 254;
/// Maximum agent name length, in characters.
pub const AGENT_NAME_MAX_CHARS: usize = 63;
/// Maximum API-key display name length, in characters.
pub const API_KEY_NAME_MAX_CHARS: usize = 63;

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
/// Default `GET /*/keys` page size.
pub const API_KEYS_DEFAULT_LIMIT: i64 = 50;
/// Maximum `GET /*/keys` page size.
pub const API_KEYS_MAX_LIMIT: i64 = 100;
