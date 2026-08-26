//! Shared application state injected into every handler.

use std::sync::Arc;

use notegate_core::Config;
use notegate_core::security::PiiCrypto;
use notegate_db::{
    AccountRepo, AgentRepo, ApiKeyRepo, AuditEventRepo, BackgroundJobRepo, BrowserSessionRepo,
    CommandInvocationRepo, ConnectionRepo, FilesRepo, LinkGraphRepo, PgPool, SpaceRepo, UsageRepo,
};
use notegate_search::SearchRuntime;
use notegate_service::accounts::AccountService;
use notegate_service::agents::AgentService;
use notegate_service::connections::ConnectionService;
use notegate_service::files::FilesService;
use notegate_service::link_graph::LinkGraphService;
use notegate_service::spaces::SpaceService;
use notegate_service::usage::UsageService;
use tokio_util::sync::CancellationToken;

use crate::identity::CallerResolver;
use crate::internal_search::SearchClient;
use crate::object_storage::ObjectStorage;
use crate::observability::MetricsHandle;

use crate::admission::DocxValidationAdmission;
use crate::auth::jwt::JwtAuthority;
use crate::auth::oidc::OidcProvider;
use crate::metadata_write_behind::MetadataWriteBuffer;

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
/// Derived Markdown-link graph projection service.
pub type LinkGraph = LinkGraphService;
/// User-facing account and Space usage service.
pub type Usage = UsageService;

#[derive(Debug, Clone)]
pub(crate) struct DatabasePoolObservation {
    pub(crate) role: &'static str,
    pub(crate) pool: PgPool,
    pub(crate) max_connections: u32,
}

#[derive(Clone)]
pub(crate) struct ControlPlaneState {
    pub(crate) readiness_pool: PgPool,
    pub(crate) database_pools: Vec<DatabasePoolObservation>,
    pub(crate) metrics: Option<MetricsHandle>,
    pub(crate) search_metrics_runtime: Option<SearchRuntime>,
}

impl ControlPlaneState {
    pub(crate) fn primary(
        pool: PgPool,
        max_connections: u32,
        metrics: Option<MetricsHandle>,
    ) -> Self {
        Self {
            readiness_pool: pool.clone(),
            database_pools: vec![DatabasePoolObservation {
                role: "primary",
                pool,
                max_connections,
            }],
            metrics,
            search_metrics_runtime: None,
        }
    }

    pub(crate) fn with_read_pool(mut self, pool: PgPool, max_connections: u32) -> Self {
        self.database_pools.push(DatabasePoolObservation {
            role: "read",
            pool,
            max_connections,
        });
        self
    }

    pub(crate) fn with_search_metrics_runtime(mut self, runtime: Option<SearchRuntime>) -> Self {
        self.search_metrics_runtime = runtime;
        self
    }
}

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
    pub(crate) search: SearchClient,
    pub link_graph: LinkGraph,
    pub(crate) docx_validation_admission: DocxValidationAdmission,
    pub usage: Usage,
    /// Account lookup for resolving attribution refs in REST output.
    pub accounts: AccountRepo,
    pub browser_sessions: BrowserSessionRepo,
    pub(crate) metadata_writes: MetadataWriteBuffer,
    pub(crate) command_invocations: CommandInvocationRepo,
    pub(crate) metrics: Option<MetricsHandle>,
    pub(crate) search_metrics_runtime: Option<SearchRuntime>,
    pub(crate) read_db_metrics: Option<(PgPool, u32)>,
    pub(crate) shutdown: CancellationToken,
}

impl AppState {
    #[cfg(test)]
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
        let search_store =
            FilesRepo::with_limits_and_crypto(db.clone(), config.limits, pii_crypto.clone())
                .with_metrics_enabled(config.metrics_enabled);
        let search = SearchClient::local(SearchRuntime::new(
            search_store,
            config.search_body_cache,
            config.metrics_enabled,
        ));
        Self::build(db, config, jwt, oidc, resolver, http, pii_crypto, search)
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn new_with_search_client(
        db: PgPool,
        config: Arc<Config>,
        jwt: Arc<JwtAuthority>,
        oidc: Arc<OidcProvider>,
        resolver: Arc<dyn CallerResolver>,
        http: reqwest::Client,
        pii_crypto: PiiCrypto,
        search: SearchClient,
    ) -> Self {
        Self::build(db, config, jwt, oidc, resolver, http, pii_crypto, search)
    }

    #[allow(clippy::too_many_arguments)]
    fn build(
        db: PgPool,
        config: Arc<Config>,
        jwt: Arc<JwtAuthority>,
        oidc: Arc<OidcProvider>,
        resolver: Arc<dyn CallerResolver>,
        http: reqwest::Client,
        pii_crypto: PiiCrypto,
        search: SearchClient,
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
        let command_invocations = CommandInvocationRepo::new(db.clone());
        let account_lifecycle = AccountService::new(
            account_repo.clone(),
            AuditEventRepo::new(db.clone()),
            command_invocations.clone(),
            BackgroundJobRepo::new(db.clone()),
        );
        let connections = ConnectionService::new(ConnectionRepo::new(db.clone()));
        let agent_repo = AgentRepo::new(db.clone());
        let agents =
            AgentService::with_crypto(agent_repo.clone(), api_key_repo, pii_crypto.clone());
        let files_repo =
            FilesRepo::with_limits_and_crypto(db.clone(), config.limits, pii_crypto.clone())
                .with_metrics_enabled(config.metrics_enabled);
        let files = FilesService::new(files_repo.clone());
        let link_graph = LinkGraphService::new(
            LinkGraphRepo::new(db.clone()),
            files_repo,
            notegate_db::LinkGraphWorkRepo::new(db.clone()),
        );
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
            link_graph,
            docx_validation_admission: DocxValidationAdmission::default(),
            usage,
            accounts: account_repo,
            browser_sessions,
            metadata_writes: MetadataWriteBuffer::default(),
            command_invocations,
            metrics: None,
            search_metrics_runtime: None,
            read_db_metrics: None,
            shutdown: CancellationToken::new(),
        }
    }

    pub(crate) fn with_metrics(mut self, metrics: Option<MetricsHandle>) -> Self {
        self.metrics = metrics;
        self
    }

    pub(crate) fn with_search_metrics_runtime(mut self, runtime: Option<SearchRuntime>) -> Self {
        self.search_metrics_runtime = runtime;
        self
    }

    pub(crate) fn with_read_db_metrics(mut self, pool: PgPool, max_connections: u32) -> Self {
        self.read_db_metrics = Some((pool, max_connections));
        self
    }

    pub(crate) fn control_plane_state(&self) -> ControlPlaneState {
        let mut state = ControlPlaneState::primary(
            self.db.clone(),
            self.config.db_max_connections,
            self.metrics.clone(),
        )
        .with_search_metrics_runtime(self.search_metrics_runtime.clone());
        if let Some((pool, max_connections)) = &self.read_db_metrics {
            state = state.with_read_pool(pool.clone(), *max_connections);
        }
        state
    }

    pub(crate) fn with_shutdown_token(mut self, shutdown: CancellationToken) -> Self {
        self.shutdown = shutdown;
        self
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
