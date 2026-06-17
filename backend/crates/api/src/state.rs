//! Shared application state injected into every handler.

use std::sync::Arc;

use notegate_core::Config;
use notegate_core::security::PiiCrypto;
use notegate_db::{
    AccountRepo, AgentRepo, ApiKeyRepo, BrowserSessionRepo, ConnectionRepo, FilesRepo, PgPool,
    SpaceRepo,
};
use notegate_service::accounts::AccountService;
use notegate_service::agents::AgentService;
use notegate_service::connections::ConnectionService;
use notegate_service::files::FilesService;
use notegate_service::search::SearchService;
use notegate_service::spaces::SpaceService;

use crate::identity::CallerResolver;

use crate::auth::jwt::JwtAuthority;
use crate::auth::oidc::OidcProvider;

/// Space lifecycle service over the db-backed [`SpaceRepo`].
pub type Spaces = SpaceService;
/// Current-account lifecycle service over the db-backed [`AccountRepo`].
pub type Accounts = AccountService;
/// Agent-connection service over the db-backed [`ConnectionRepo`].
pub type Connections = ConnectionService;
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
    pub spaces: Spaces,
    pub account_lifecycle: Accounts,
    pub connections: Connections,
    pub agents: Agents,
    pub files: Files,
    pub search: Search,
    /// Account lookup for resolving attribution refs in REST output.
    pub accounts: AccountRepo,
    pub browser_sessions: BrowserSessionRepo,
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
        let spaces = SpaceService::new(SpaceRepo::new(db.clone()));
        let api_key_repo = ApiKeyRepo::with_lookup_key(
            db.clone(),
            pii_crypto.lookup_key_id(),
            pii_crypto.version(),
        );
        let account_lifecycle = AccountService::with_api_keys(
            AccountRepo::with_crypto_and_default_user_tier(
                db.clone(),
                pii_crypto.clone(),
                config.default_user_tier,
            ),
            api_key_repo.clone(),
            pii_crypto.clone(),
        );
        let connections = ConnectionService::new(ConnectionRepo::new(db.clone()));
        let agent_repo = AgentRepo::new(db.clone());
        let agents =
            AgentService::with_crypto(agent_repo.clone(), api_key_repo, pii_crypto.clone());
        let files_repo = FilesRepo::with_limits(db.clone(), config.limits);
        let files = FilesService::new(files_repo.clone());
        let search = SearchService::new(files_repo);
        let accounts = AccountRepo::with_crypto_and_default_user_tier(
            db.clone(),
            pii_crypto.clone(),
            config.default_user_tier,
        );
        let browser_sessions = BrowserSessionRepo::with_lookup_key(
            db.clone(),
            pii_crypto.lookup_key_id(),
            pii_crypto.version(),
        );
        Self {
            db,
            config,
            jwt,
            oidc,
            resolver,
            http,
            security: pii_crypto,
            spaces,
            account_lifecycle,
            connections,
            agents,
            files,
            search,
            accounts,
            browser_sessions,
        }
    }
}
