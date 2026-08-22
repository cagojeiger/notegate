use std::io;
use std::sync::Arc;
use std::time::Duration;

use notegate_core::Config;
use notegate_core::security::PiiCrypto;
use tokio::net::TcpListener;
#[cfg(unix)]
use tokio::signal::unix::{SignalKind, signal};
use tokio::task::JoinSet;
use tokio_util::sync::CancellationToken;
use tracing::info;
use tracing_subscriber::filter::filter_fn;
use tracing_subscriber::layer::SubscriberExt as _;
use tracing_subscriber::{EnvFilter, Layer as _, util::SubscriberInitExt as _};

use crate::runtime_plan::{ListenerSurface, RuntimePlan, SearchClientTarget};
use crate::state::{AppState, ControlPlaneState};
use crate::{
    auth, internal_search, object_storage, observability, openapi, process_runtime, routes,
    usage_bootstrap,
};

pub(crate) async fn run() -> anyhow::Result<()> {
    if std::env::args().any(|arg| arg == "--print-openapi") {
        println!("{}", openapi::json_pretty()?);
        return Ok(());
    }
    let recalculate_usage = std::env::args().any(|arg| arg == "--recalculate-usage");

    // Load `.env` for local development; absence is fine in production.
    let _ = dotenvy::dotenv();

    let config = Arc::new(Config::load()?);
    let process_mode = config.process_mode;
    let plan = RuntimePlan::from_config(process_mode, config.search_service_url.as_deref());
    init_tracing(config.metrics_enabled);
    let metrics = observability::install(config.metrics_enabled, process_mode)?;

    // fail-fast: install the SIGTERM handler during boot so a failure here
    // aborts startup instead of leaving us without graceful shutdown.
    let signals = ShutdownSignals::install()?;

    let pools =
        notegate_db::PgPools::connect(&config, plan.uses_read_pool && !recalculate_usage).await?;
    let pool = pools.primary().clone();
    if recalculate_usage {
        notegate_db::run_migrations(&pool).await?;
        let spaces_recalculated = usage_bootstrap::recalculate_all(&pool).await?;
        println!("recalculated {spaces_recalculated} spaces");
        pools.close().await;
        return Ok(());
    }
    if plan.runs_api {
        notegate_db::run_migrations(&pool).await?;
        usage_bootstrap::ensure(&pool).await?;
    } else {
        notegate_db::check_readiness(&pool).await?;
    }
    info!(
        event = "db.ready",
        max_connections = config.db_max_connections,
        process_mode = process_mode.as_str(),
    );

    let pii_crypto = PiiCrypto::from_root_secrets(
        config.enc_root_key_id.clone(),
        &config.enc_root_secret,
        config.lookup_root_key_id.clone(),
        &config.lookup_root_secret,
    )?;
    let key_epochs = notegate_db::CryptoKeyEpochRepo::new(pool.clone());
    if plan.runs_api {
        key_epochs.ensure_active(&pii_crypto).await?;
        info!(event = "crypto_key_epochs.ensured");
    } else {
        key_epochs.verify_active(&pii_crypto).await?;
        info!(event = "crypto_key_epochs.verified");
    }

    let bind_addr = config.bind_addr;
    let search_bind_addr = config.search_bind_addr;
    notegate_core::cursor::configure_signing_key(pii_crypto.session_signing_key())?;
    let application_shutdown_token = CancellationToken::new();
    let internal_signing_key = pii_crypto.internal_search_signing_key();
    let main_listener_metrics = (plan.metrics_endpoint == ListenerSurface::Main)
        .then(|| metrics.clone())
        .flatten();
    let search_listener_metrics = (plan.metrics_endpoint == ListenerSurface::Search)
        .then(|| metrics.clone())
        .flatten();
    let search_runtime = plan.search_listener.then(|| {
        let authority_store = notegate_db::FilesRepo::with_limits_and_crypto(
            pool.clone(),
            config.limits,
            pii_crypto.clone(),
        )
        .with_metrics_enabled(config.metrics_enabled);
        let query_store = notegate_db::FilesRepo::with_limits_and_crypto(
            pools.read().clone(),
            config.limits,
            pii_crypto.clone(),
        )
        .with_metrics_enabled(config.metrics_enabled);
        notegate_search::SearchRuntime::with_authority_and_query_stores(
            authority_store,
            query_store,
            config.search_body_cache,
            config.metrics_enabled,
        )
    });
    let search_state = search_runtime.clone().map(|runtime| {
        let read_db = pools
            .has_separate_read_pool()
            .then(|| (pools.read().clone(), pools.read_max_connections()));
        internal_search::SearchServerState::new(
            pool.clone(),
            pools.primary_max_connections(),
            read_db,
            runtime,
            internal_signing_key,
            search_listener_metrics,
        )
    });
    let main_search_metrics_runtime = (plan.metrics_endpoint == ListenerSurface::Main)
        .then(|| search_runtime.clone())
        .flatten();

    let state = if plan.runs_api {
        let http = reqwest::Client::builder()
            .timeout(Duration::from_secs(10))
            .redirect(reqwest::redirect::Policy::none())
            .build()?;
        let search = match plan.search_client {
            SearchClientTarget::LocalListener => internal_search::SearchClient::http(
                &internal_search::loopback_base_url(search_bind_addr),
                internal_signing_key,
            )?,
            SearchClientTarget::RemoteService(base_url) => {
                internal_search::SearchClient::http(base_url, internal_signing_key)?
            }
            SearchClientTarget::Disabled => internal_search::SearchClient::disabled(),
        };
        let jwks_url = format!("{}/keys", config.authgate_url);
        // The db-backed identity resolver resolves users and API-key owners.
        let account_repo = notegate_db::AccountRepo::with_crypto_and_default_user_tier(
            pool.clone(),
            pii_crypto.clone(),
            config.default_user_tier,
        );
        let api_key_repo = notegate_db::ApiKeyRepo::with_lookup_key(
            pool.clone(),
            pii_crypto.lookup_key_id(),
            pii_crypto.version(),
        );
        let resolver = notegate_service::identity::Resolver::new(
            account_repo,
            api_key_repo,
            pii_crypto.clone(),
        );
        let jwt = Arc::new(auth::jwt::JwtAuthority::from_url(&config, jwks_url));
        let oidc = Arc::new(auth::oidc::OidcProvider::new(&config, http.clone()));
        let mut state = AppState::new_with_search_client(
            pool.clone(),
            config.clone(),
            jwt,
            oidc,
            Arc::new(resolver),
            http,
            pii_crypto.clone(),
            search,
        )
        .with_metrics(main_listener_metrics.clone())
        .with_search_metrics_runtime(main_search_metrics_runtime)
        .with_shutdown_token(application_shutdown_token.clone());
        if pools.has_separate_read_pool() {
            state = state.with_read_db_metrics(pools.read().clone(), pools.read_max_connections());
        }
        Some(state)
    } else {
        None
    };

    let mut main_http_runtime = HttpRuntime::new();
    let mut search_http_runtime = HttpRuntime::new();
    if plan.main_listener {
        let router = match &state {
            Some(state) => routes::app(state.clone()),
            None => routes::control_app(ControlPlaneState::primary(
                pool.clone(),
                pools.primary_max_connections(),
                main_listener_metrics.clone(),
            )),
        };
        let listener = TcpListener::bind(bind_addr).await?;
        info!(
            event = "server.listening",
            surface = "public",
            addr = %bind_addr,
            process_mode = process_mode.as_str(),
        );
        main_http_runtime.spawn("public HTTP server", listener, router);
    }
    if let Some(search_state) = search_state {
        let listener = TcpListener::bind(search_bind_addr).await?;
        info!(
            event = "server.listening",
            surface = "internal_search",
            addr = %search_bind_addr,
            process_mode = process_mode.as_str(),
        );
        search_http_runtime.spawn(
            "internal search HTTP server",
            listener,
            routes::search_app(search_state),
        );
    }

    let mut process_runtime =
        process_runtime::ProcessRuntime::new(metrics, application_shutdown_token);
    if plan.runs_worker {
        let link_graph = state
            .as_ref()
            .map(|state| state.link_graph.clone())
            .unwrap_or_else(|| {
                let files = notegate_db::FilesRepo::with_limits_and_crypto(
                    pool.clone(),
                    config.limits,
                    pii_crypto.clone(),
                )
                .with_metrics_enabled(config.metrics_enabled);
                notegate_service::link_graph::LinkGraphService::new(
                    notegate_db::LinkGraphRepo::new(pool.clone()),
                    files,
                    notegate_db::LinkGraphWorkRepo::new(pool.clone()),
                )
            });
        process_runtime.start_worker(
            pool.clone(),
            config.background_jobs,
            config.metrics_enabled,
            link_graph,
        )?;
    }
    if plan.runs_reconciler {
        let object_storage = state
            .as_ref()
            .map(|state| state.object_storage.clone())
            .unwrap_or_else(|| object_storage::ObjectStorage::new(&config.s3));
        process_runtime.start_reconciler(&pool, object_storage)?;
    }
    if plan.runs_api {
        let state = state
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("api mode requires application state"))?;
        process_runtime.start_metadata_writer(
            state.metadata_writes.clone(),
            pool.clone(),
            config.metrics_enabled,
        );
    }

    enum StopReason {
        Http(anyhow::Error),
        Signal,
        Runtime(anyhow::Error),
    }

    let stop_reason = tokio::select! {
        error = main_http_runtime.wait_for_exit() => StopReason::Http(error),
        error = search_http_runtime.wait_for_exit() => StopReason::Http(error),
        () = signals.wait() => StopReason::Signal,
        error = process_runtime.wait_for_critical_exit() => {
            tracing::error!(event = "process_runtime.critical_task_exited", %error);
            StopReason::Runtime(error)
        },
    };

    info!(event = "server.shutting_down");
    process_runtime.begin_shutdown();
    let http_result = shutdown_http_runtimes(main_http_runtime, search_http_runtime).await;

    let server_result = match stop_reason {
        StopReason::Http(error) => {
            if let Err(shutdown_error) = http_result {
                tracing::error!(event = "server.graceful_shutdown_failed", %shutdown_error);
            }
            Err(error)
        }
        StopReason::Signal => http_result,
        StopReason::Runtime(runtime_error) => {
            if let Err(error) = http_result {
                tracing::error!(event = "server.graceful_shutdown_failed", %error);
            }
            Err(runtime_error)
        }
    };

    // HTTP has drained, so the metadata writer can flush its final observations.
    process_runtime.join().await;

    // Background tasks record cancellation or finish their final flush before
    // returning. Close the pool only after every task has joined.
    pools.close().await;
    info!(event = "shutdown.complete");

    server_result
}

struct HttpRuntime {
    shutdown: CancellationToken,
    tasks: JoinSet<(&'static str, io::Result<()>)>,
}

impl HttpRuntime {
    fn new() -> Self {
        Self {
            shutdown: CancellationToken::new(),
            tasks: JoinSet::new(),
        }
    }

    fn spawn(&mut self, name: &'static str, listener: TcpListener, router: axum::Router) {
        let shutdown = self.shutdown.clone().cancelled_owned();
        self.tasks.spawn(async move {
            let result = axum::serve(listener, router)
                .with_graceful_shutdown(shutdown)
                .await;
            (name, result)
        });
    }

    async fn wait_for_exit(&mut self) -> anyhow::Error {
        match self.tasks.join_next().await {
            Some(Ok((name, Ok(())))) => anyhow::anyhow!("{name} stopped unexpectedly"),
            Some(Ok((name, Err(error)))) => anyhow::anyhow!("{name} failed: {error}"),
            Some(Err(error)) => anyhow::anyhow!("HTTP server task failed: {error}"),
            None => std::future::pending().await,
        }
    }

    fn begin_shutdown(&self) {
        self.shutdown.cancel();
    }

    async fn join(mut self) -> anyhow::Result<()> {
        self.shutdown.cancel();
        while let Some(result) = self.tasks.join_next().await {
            match result {
                Ok((_name, Ok(()))) => {}
                Ok((name, Err(error))) => return Err(anyhow::anyhow!("{name} failed: {error}")),
                Err(error) => return Err(anyhow::anyhow!("HTTP server task failed: {error}")),
            }
        }
        Ok(())
    }
}

async fn shutdown_http_runtimes(
    main_runtime: HttpRuntime,
    search_runtime: HttpRuntime,
) -> anyhow::Result<()> {
    // Search is a dependency of accepted main-listener requests, so keep it
    // available until those requests have drained.
    main_runtime.begin_shutdown();
    let main_result = main_runtime.join().await;
    search_runtime.begin_shutdown();
    let search_result = search_runtime.join().await;
    main_result.and(search_result)
}

#[cfg(test)]
mod tests;

struct ShutdownSignals {
    #[cfg(unix)]
    sigterm: tokio::signal::unix::Signal,
}

impl ShutdownSignals {
    fn install() -> io::Result<Self> {
        #[cfg(unix)]
        let sigterm = signal(SignalKind::terminate())?;

        Ok(Self {
            #[cfg(unix)]
            sigterm,
        })
    }

    async fn wait(mut self) {
        let ctrl_c = async {
            if let Err(error) = tokio::signal::ctrl_c().await {
                tracing::error!(%error, "failed to wait for Ctrl+C");
            }
        };

        #[cfg(unix)]
        let terminate = async {
            self.sigterm.recv().await;
        };

        #[cfg(not(unix))]
        let terminate = std::future::pending::<()>();

        tokio::select! {
            () = ctrl_c => {}
            () = terminate => {}
        }
    }
}

fn init_tracing(metrics_enabled: bool) {
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_error| {
        EnvFilter::new("notegate_api=info,notegate_db=info,tower_http=info")
    });

    let result = if std::env::var("LOG_FORMAT").as_deref() == Ok("json") {
        let internal_metrics =
            observability::InternalMetricsLayer.with_filter(filter_fn(move |metadata| {
                internal_metrics_event(metrics_enabled, metadata)
            }));
        tracing_subscriber::registry()
            .with(tracing_subscriber::fmt::layer().json().with_filter(filter))
            .with(internal_metrics)
            .try_init()
    } else {
        let internal_metrics =
            observability::InternalMetricsLayer.with_filter(filter_fn(move |metadata| {
                internal_metrics_event(metrics_enabled, metadata)
            }));
        tracing_subscriber::registry()
            .with(tracing_subscriber::fmt::layer().with_filter(filter))
            .with(internal_metrics)
            .try_init()
    };

    if let Err(error) = result {
        eprintln!("failed to initialize tracing: {error}");
    }
}

fn internal_metrics_event(metrics_enabled: bool, metadata: &tracing::Metadata<'_>) -> bool {
    metrics_enabled
        && matches!(
            metadata.target(),
            "sqlx::pool::acquire" | "notegate_db::pool" | "notegate_db::crypto"
        )
}
