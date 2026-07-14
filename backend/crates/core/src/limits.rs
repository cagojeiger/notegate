//! Product limits for notegate.
//!
//! Every create/list/search/read/subtree operation must be bounded. Most limits
//! are fixed hard-max product constants. Runtime effective quotas may be lower,
//! but they must not exceed these core maxima.
//! Expensive file-tree capacity caps can be lowered through [`crate::Config`]
//! for E2E/dev testing while keeping these spec maxima.
//!
//! These are product limits, not security boundaries; authorization still
//! checks every request.

// --- HTTP ingress limits ---

/// Maximum HTTP request body size accepted by the API server (2 MiB).
pub const HTTP_REQUEST_BODY_MAX_BYTES: usize = 2_097_152;
/// Maximum wall-clock time for one HTTP request before a 408 response.
pub const HTTP_REQUEST_TIMEOUT_SECS: u64 = 30;
/// Maximum wall-clock time for control-plane probes before a 408 response.
pub const HTTP_CONTROL_PLANE_TIMEOUT_SECS: u64 = 5;
/// Maximum HTTP requests accepted per API process per minute.
pub const HTTP_RATE_LIMIT_REQUESTS_PER_MINUTE: u32 = 600;

// --- Account, space, and credential limits ---

/// Maximum spaces a single user owner account may own.
pub const OWNED_SPACES_MAX: usize = 20;
/// Maximum active agent connections per space.
pub const CONNECTIONS_PER_SPACE_MAX: usize = 50;
/// Maximum live spaces a single agent may be connected to.
pub const CONNECTED_SPACES_PER_AGENT_MAX: usize = 100;
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

/// Maximum space name length, in characters.
pub const SPACE_NAME_MAX_LEN: usize = 63;
/// Maximum folder name length, in characters.
pub const FOLDER_NAME_MAX_LEN: usize = 128;
/// Maximum text node name length, in characters.
pub const TEXT_NAME_MAX_LEN: usize = 128;
/// Maximum derived path length, in bytes.
pub const MAX_PATH_LEN: usize = 903;
/// Maximum path depth, in segments below the space root.
pub const MAX_PATH_DEPTH: usize = 7;
/// Maximum live nodes per space.
pub const SPACE_MAX_NODES: usize = 25_000;
/// Maximum total live Text bytes per space (1 GiB).
pub const SPACE_MAX_TEXT_BYTES: usize = 1_073_741_824;
/// Maximum total live File bytes per space (1 GiB).
pub const SPACE_MAX_FILE_BYTES: usize = 1_073_741_824;
/// Maximum file bytes stored inline in PostgreSQL (256 KiB).
pub const FILE_INLINE_PG_MAX_BYTES: usize = 262_144;
/// Maximum bytes per uploaded file (100 MiB).
pub const FILE_MAX_BYTES: usize = 104_857_600;
/// Maximum concurrent in-flight object uploads (unfinished `uploading` rows)
/// per account. Bounds how much a caller can stage in object storage before any
/// quota is charged; stale rows are reclaimed by the cleanup worker.
pub const OBJECT_UPLOAD_MAX_PENDING: usize = 16;

// --- Node metadata limits ---

/// Maximum serialized JSON bytes per node metadata object (16 KiB).
pub const NODE_METADATA_MAX_BYTES: usize = 16_384;
/// Maximum nesting depth for node metadata JSON. Root object depth is 1.
pub const NODE_METADATA_MAX_DEPTH: usize = 4;
/// Maximum node metadata object key length, in Unicode scalar values.
pub const NODE_METADATA_KEY_MAX_CHARS: usize = 64;
/// Maximum string value length inside node metadata, in Unicode scalar values.
pub const NODE_METADATA_STRING_MAX_CHARS: usize = 2_048;

// --- Listing and folder fanout limits ---

/// Maximum live direct children per folder.
pub const FOLDER_MAX_CHILDREN: usize = 1_000;

/// Runtime-overridable space/file-tree capacity limits.
///
/// These defaults are the product contract. Tests and local E2E runs may lower
/// them through `Config.limits`; code should receive a [`Limits`] value instead
/// of reading process environment directly.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Limits {
    pub space_max_nodes: usize,
    pub space_max_text_bytes: usize,
    pub space_max_file_bytes: usize,
    pub folder_max_children: usize,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            space_max_nodes: SPACE_MAX_NODES,
            space_max_text_bytes: SPACE_MAX_TEXT_BYTES,
            space_max_file_bytes: SPACE_MAX_FILE_BYTES,
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
/// Maximum direct children fetched by one search scanner page.
pub const SEARCH_CHILDREN_PAGE_MAX: i64 = 200;
/// Maximum node summaries inspected by one search request.
pub const SEARCH_NODE_SCAN_MAX: usize = 1_000;
/// Maximum search candidates inspected by one find/grep request.
/// Implementations may fetch one extra sentinel row to detect `has_more`.
pub const SEARCH_CANDIDATE_PAGE_MAX: i64 = 1_000;
/// Maximum plain text bytes read by one grep request (8 MiB).
pub const GREP_SCAN_MAX_BYTES: usize = 8_388_608;
/// Maximum search query length in Unicode scalar values.
pub const SEARCH_QUERY_MAX_CHARS: usize = 256;
/// Maximum include or exclude glob patterns accepted by one grep request.
pub const SEARCH_GLOB_PATTERNS_MAX: usize = 32;
/// Maximum length of one include/exclude glob pattern, in Unicode scalar values.
pub const SEARCH_GLOB_PATTERN_MAX_CHARS: usize = 256;

// --- Read limits ---

/// Default maximum lines returned by `read`/`open`.
pub const READ_DEFAULT_MAX_LINES: i64 = 200;
/// Maximum lines returned by `read`/`open`.
pub const READ_MAX_LINES: i64 = 5_000;
/// Default maximum bytes returned by `read`/`open` (64 KiB).
pub const READ_DEFAULT_MAX_BYTES: usize = 65_536;
/// Maximum bytes returned by `read`/`open` (1 MiB).
pub const READ_MAX_BYTES: usize = 1_048_576;

// --- Text creation and write limits ---

/// Maximum bytes per text (1 MiB).
pub const TEXT_MAX_BYTES: usize = 1_048_576;
/// Maximum lines per text.
pub const TEXT_MAX_LINES: usize = 5_000;

// --- Subtree mutation limits ---

/// Maximum nodes a synchronous folder delete may touch.
pub const SUBTREE_DELETE_MAX_NODES: usize = 1_000;
/// Days a deleted space is retained before it is eligible for hard purge.
pub const DELETED_SPACE_RETENTION_DAYS: i64 = 30;
/// Days a deleted node is retained before it is eligible for hard purge.
pub const DELETED_NODE_RETENTION_DAYS: i64 = 30;
/// Days a soft-deleted account is retained before the purge run anonymizes its PII
/// and frees the provider-sub tombstone.
pub const ACCOUNT_DELETION_RETENTION_DAYS: i64 = 15;
/// Days a revoked or expired API key row is retained before the purge run deletes it.
pub const DEAD_API_KEY_RETENTION_DAYS: i64 = 30;

// --- API pagination limits ---

/// Default `GET /spaces/{id}/nodes` page size.
pub const NODES_DEFAULT_LIMIT: i64 = 50;
/// Maximum `GET /spaces/{id}/nodes` page size.
pub const NODES_MAX_LIMIT: i64 = 100;
/// Default `GET /spaces` page size.
pub const SPACES_DEFAULT_LIMIT: i64 = 50;
/// Maximum `GET /spaces` page size.
pub const SPACES_MAX_LIMIT: i64 = 100;
/// Default `GET /spaces/{id}/agents` page size.
pub const CONNECTIONS_DEFAULT_LIMIT: i64 = 100;
/// Maximum `GET /spaces/{id}/agents` page size.
pub const CONNECTIONS_MAX_LIMIT: i64 = 100;
/// Default `GET /agents` page size.
pub const AGENTS_DEFAULT_LIMIT: i64 = 100;
/// Maximum `GET /agents` page size.
pub const AGENTS_MAX_LIMIT: i64 = 100;
/// Default `GET /*/keys` page size.
pub const API_KEYS_DEFAULT_LIMIT: i64 = 50;
/// Maximum `GET /*/keys` page size.
pub const API_KEYS_MAX_LIMIT: i64 = 100;
/// Default `GET /me/audit-events` page size.
pub const AUDIT_EVENTS_DEFAULT_LIMIT: i64 = 50;
/// Maximum `GET /me/audit-events` page size.
pub const AUDIT_EVENTS_MAX_LIMIT: i64 = 100;
/// Default `GET /spaces/{id}/file-change-events` page size.
pub const FILE_CHANGE_EVENTS_DEFAULT_LIMIT: i64 = 50;
/// Maximum `GET /spaces/{id}/file-change-events` page size.
pub const FILE_CHANGE_EVENTS_MAX_LIMIT: i64 = 100;
