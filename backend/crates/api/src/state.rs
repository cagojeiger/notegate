//! Shared application state injected into every handler.

use std::sync::Arc;

use notegate_core::Config;
use notegate_core::security::PiiCrypto;
use notegate_db::{AccessRepo, AccountRepo, AgentRepo, FilesRepo, PgPool, WorkspaceRepo};
use notegate_service::access::AccessService;
use notegate_service::accounts::AccountService;
use notegate_service::agents::AgentService;
use notegate_service::files::FilesService;
use notegate_service::search::SearchService;
use notegate_service::workspaces::WorkspaceService;

use crate::identity::CallerResolver;

use crate::auth::jwt::JwtAuthority;
use crate::auth::oidc::OidcProvider;

/// Workspace lifecycle service over the db-backed [`WorkspaceRepo`].
pub type Workspaces = WorkspaceService;
/// Current-account lifecycle service over the db-backed [`AccountRepo`].
pub type Accounts = AccountService;
/// Access-management service over the db-backed [`AccessRepo`].
pub type Access = AccessService;
/// Agent lifecycle service over the db-backed [`AgentRepo`].
pub type Agents = AgentService;
/// File-tree command service over the db-backed [`FilesRepo`].
pub type Files = FilesService;
/// Search service over the db-backed [`FilesRepo`].
pub type Search = SearchService;

#[derive(Clone)]
pub struct AppState {
    pub db: PgPool,
    pub config: Arc<Config>,
    pub jwt: Arc<JwtAuthority>,
    pub oidc: Arc<OidcProvider>,
    pub resolver: Arc<dyn CallerResolver>,
    pub http: reqwest::Client,
    pub security: PiiCrypto,
    pub workspaces: Workspaces,
    pub account_lifecycle: Accounts,
    pub access: Access,
    pub agents: Agents,
    pub files: Files,
    pub search: Search,
    /// Account lookup for resolving attribution refs in REST output.
    pub accounts: AccountRepo,
}

impl AppState {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        db: PgPool,
        config: Arc<Config>,
        jwt: Arc<JwtAuthority>,
        oidc: Arc<OidcProvider>,
        resolver: Arc<dyn CallerResolver>,
        http: reqwest::Client,
        pii_crypto: PiiCrypto,
    ) -> Self {
        let workspaces = WorkspaceService::new(WorkspaceRepo::new(db.clone()));
        let account_lifecycle =
            AccountService::new(AccountRepo::with_crypto(db.clone(), pii_crypto.clone()));
        let access = AccessService::new(AccessRepo::new(db.clone()));
        let agent_repo = AgentRepo::with_lookup_key(
            db.clone(),
            pii_crypto.lookup_key_id(),
            pii_crypto.version(),
        );
        let agents = AgentService::new(agent_repo.clone());
        let files_repo = FilesRepo::with_limits(db.clone(), config.limits);
        let files = FilesService::with_limits(files_repo.clone(), config.limits);
        let search = SearchService::new(files_repo);
        let accounts = AccountRepo::with_crypto(db.clone(), pii_crypto.clone());
        Self {
            db,
            config,
            jwt,
            oidc,
            resolver,
            http,
            security: pii_crypto,
            workspaces,
            account_lifecycle,
            access,
            agents,
            files,
            search,
            accounts,
        }
    }
}
