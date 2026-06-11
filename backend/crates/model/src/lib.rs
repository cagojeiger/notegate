//! Pure domain types for notegate.

pub mod account;
pub mod agent;
pub mod api_key;
pub mod files;
pub mod identity;
pub mod node;
pub mod search;
pub mod space;
pub mod text;
pub mod user;

pub use account::{Account, AccountKind, AccountRef};
pub use agent::{Agent, AgentPage, CreateAgent, CreateAgentApiKey, ListAgents};
pub use api_key::{ApiKey, ApiKeyCursor, ApiKeyPage, CreateApiKey, ListApiKeys, MintedApiKey};
pub use identity::{Caller, CallerIdentity, Channel, ResolveAttrs};
pub use node::{Node, NodeKind};
pub use space::{
    ConnectAgent, ConnectionPage, CreateSpace, ListConnections, ListSpaces, Permission, Space,
    SpaceAgentConnection, SpaceCursor, SpacePage, SpaceView, UpdateSpace,
};
pub use text::{FileObject, FileStorageKind, TextObject, TextStorageFormat};
pub use user::User;
