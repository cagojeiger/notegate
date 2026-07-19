//! Shared application state injected into every handler.

use std::sync::Arc;

use notegate_core::Config;
use notegate_core::security::PiiCrypto;
use notegate_db::{
    AccountRepo, AgentRepo, ApiKeyRepo, AuditEventRepo, BrowserSessionRepo, ConnectionRepo,
    FilesRepo, PgPool, SpaceRepo, UsageRepo,
};
use notegate_service::accounts::AccountService;
use notegate_service::agents::AgentService;
use notegate_service::connections::ConnectionService;
use notegate_service::files::FilesService;
use notegate_service::search::SearchService;
use notegate_service::spaces::SpaceService;
use notegate_service::usage::UsageService;

use crate::identity::CallerResolver;
use crate::object_storage::ObjectStorage;

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
/// User-facing account and Space usage service.
pub type Usage = UsageService;

#[derive(Clone)]
pub struct AppState {
    pub db: PgPool,
    pub config: Arc<Config>,
    pub jwt: Arc<JwtAuthority>,
    pub oidc: Arc<OidcProvider>,
    pub resolver: Arc<dyn CallerResolver>,
    pub http: reqwest::Client,
    pub object_storage: ObjectStorage,
    pub security: PiiCrypto,
    pub spaces: Spaces,
    pub account_lifecycle: Accounts,
    pub connections: Connections,
    pub agents: Agents,
    pub files: Files,
    pub search: Search,
    pub usage: Usage,
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
        let object_storage = ObjectStorage::new(&config.s3);
        let spaces = SpaceService::new(SpaceRepo::new(db.clone()));
        let api_key_repo = ApiKeyRepo::with_lookup_key(
            db.clone(),
            pii_crypto.lookup_key_id(),
            pii_crypto.version(),
        );
        let account_repo = AccountRepo::with_crypto_and_default_user_tier(
            db.clone(),
            pii_crypto.clone(),
            config.default_user_tier,
        );
        let account_lifecycle = AccountService::with_api_keys(
            account_repo.clone(),
            api_key_repo.clone(),
            AuditEventRepo::new(db.clone()),
            pii_crypto.clone(),
        );
        let connections = ConnectionService::new(ConnectionRepo::new(db.clone()));
        let agent_repo = AgentRepo::new(db.clone());
        let agents =
            AgentService::with_crypto(agent_repo.clone(), api_key_repo, pii_crypto.clone());
        let files_repo = FilesRepo::with_limits(db.clone(), config.limits);
        let files = FilesService::new(files_repo.clone());
        let search = SearchService::new(files_repo);
        let usage = UsageService::new(UsageRepo::new(db.clone()), config.limits);
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
            object_storage,
            security: pii_crypto,
            spaces,
            account_lifecycle,
            connections,
            agents,
            files,
            search,
            usage,
            accounts: account_repo,
            browser_sessions,
        }
    }
}

#[cfg(test)]
pub(crate) fn test_s3_config() -> notegate_core::S3Config {
    notegate_core::S3Config {
        endpoint: "http://localhost:9000".to_owned(),
        public_endpoint: None,
        region: "us-east-1".to_owned(),
        bucket: "notegate".to_owned(),
        access_key: "notegate-test".to_owned(),
        secret_key: secrecy::SecretString::from("notegate-test-secret".to_owned()),
        force_path_style: true,
    }
}
