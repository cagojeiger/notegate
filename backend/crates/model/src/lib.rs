//! Pure domain types for notegate.

pub mod account;
pub mod agent;
pub mod api_key;
pub mod audit_event;
pub mod background_job;
pub mod event_history;
pub mod file_change_event;
pub mod files;
pub mod identity;
pub mod link_index;
pub mod mcp_invocation;
pub mod node;
pub mod search;
pub mod space;
pub mod text;
pub mod user;

pub use account::{Account, AccountKind, AccountRef};
pub use agent::{Agent, AgentPage, CreateAgent, CreateAgentApiKey, ListAgents};
pub use api_key::{ApiKey, ApiKeyCursor, ApiKeyPage, CreateApiKey, ListApiKeys, MintedApiKey};
pub use audit_event::{AuditEvent, AuditEventCursor, AuditEventPage, ListAuditEvents};
pub use background_job::{
    BackgroundJob, BackgroundJobAttempt, BackgroundJobCursor, BackgroundJobDetail,
    BackgroundJobPage, ListBackgroundJobs,
};
pub use event_history::EventCursor;
pub use file_change_event::{
    FileChangeEvent, FileChangeEventCursor, FileChangeEventIdCursor, FileChangeEventPage,
    FileChangeSyncPage, ListFileChangeEvents, ListFileChangeEventsById, SyncFileChanges,
};
pub use identity::{Caller, CallerIdentity, Channel, ResolveAttrs};
pub use link_index::{
    IncomingLinkCursor, LinkReferenceKind, LinkReferencePage, LinkReferenceView, LinkSyncStatus,
    ListLinkReferences, OutgoingLinkCursor, SpaceLinkIndexView,
};
pub use mcp_invocation::{
    ListMcpInvocations, McpInvocation, McpInvocationCursor, McpInvocationPage,
};
pub use node::{Node, NodeKind, NodeSummary};
pub use space::{
    ConnectAgent, ConnectionPage, CreateSpace, ListConnections, ListSpaces, Permission, Space,
    SpaceAgentConnection, SpaceCursor, SpaceOrderUpdate, SpacePage, SpaceView, UpdateSpace,
};
pub use text::{
    FileEncryptionMode, FileObject, TextAtRestEncryption, TextObject, TextStorageFormat,
};
pub use user::User;
