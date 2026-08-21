use std::io;

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

mod admission;
mod agent_text;
mod auth;
mod background_jobs;
mod error;
mod file_change;
mod file_preview;
mod identity;
mod internal_search;
mod mcp;
mod metadata_write_behind;
mod object_storage;
mod object_upload_flow;
mod observability;
mod openapi;
mod page;
mod path_node_summary;
mod periodic_worker;
mod process_runtime;
mod public_v2;
mod reconciliations;
mod rest;
mod routes;
mod search_topology;
mod state;
mod usage_bootstrap;

use search_topology::{SearchClientTarget, SearchMetricsOwner, SearchTopology};
use state::AppState;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    if std::env::args().any(|arg| arg == "--print-openapi") {
        println!("{}", openapi::json_pretty()?);
        return Ok(());
    }
    let recalculate_usage = std::env::args().any(|arg| arg == "--recalculate-usage");

    // Load `.env` for local development; absence is fine in production.
    let _ = dotenvy::dotenv();

    let config = Config::load()?;
    let process_mode = config.process_mode;
    init_tracing(config.metrics_enabled);
    let metrics = observability::install(config.metrics_enabled)?;

    // fail-fast: install the SIGTERM handler during boot so a failure here
    // aborts startup instead of leaving us without graceful shutdown.
    let signals = ShutdownSignals::install()?;

    let pool = notegate_db::connect(&config).await?;
    notegate_db::run_migrations(&pool).await?;
    if recalculate_usage {
        let spaces_recalculated = usage_bootstrap::recalculate_all(&pool).await?;
        println!("recalculated {spaces_recalculated} spaces");
        pool.close().await;
        return Ok(());
    }
    usage_bootstrap::ensure(&pool).await?;
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
    key_epochs.ensure_active(&pii_crypto).await?;
    info!(event = "crypto_key_epochs.ensured");

    let bind_addr = config.bind_addr;
    let search_bind_addr = config.search_bind_addr;
    notegate_core::cursor::configure_signing_key(pii_crypto.session_signing_key())?;
    let config = std::sync::Arc::new(config);
    let application_shutdown_token = CancellationToken::new();
    let internal_signing_key = pii_crypto.internal_search_signing_key();
    let topology = SearchTopology::plan(process_mode, config.search_service_url.as_deref());
    let search_listener_metrics = if topology.metrics_owner == SearchMetricsOwner::SearchListener {
        metrics.clone()
    } else {
        None
    };
    let search_runtime = topology.search_listener.then(|| {
        let store = notegate_db::FilesRepo::with_limits_and_crypto(
            pool.clone(),
            config.limits,
            pii_crypto.clone(),
        )
        .with_metrics_enabled(config.metrics_enabled);
        notegate_search::SearchRuntime::new(store, config.search_body_cache, config.metrics_enabled)
    });
    let search_state = search_runtime.clone().map(|runtime| {
        internal_search::SearchServerState::new(
            pool.clone(),
            config.db_max_connections,
            runtime,
            internal_signing_key,
            search_listener_metrics,
        )
    });
    let public_search_metrics_runtime =
        if topology.metrics_owner == SearchMetricsOwner::PublicListener {
            search_runtime.clone()
        } else {
            None
        };

    let state = if topology.public_listener {
        let http = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(10))
            .redirect(reqwest::redirect::Policy::none())
            .build()?;
        let search = match topology.client {
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
        let jwt = std::sync::Arc::new(auth::jwt::JwtAuthority::from_url(&config, jwks_url));
        let oidc = std::sync::Arc::new(auth::oidc::OidcProvider::new(&config, http.clone()));
        Some(
            AppState::new_with_search_client(
                pool.clone(),
                config.clone(),
                jwt,
                oidc,
                std::sync::Arc::new(resolver),
                http,
                pii_crypto,
                search,
            )
            .with_metrics(metrics.clone())
            .with_search_metrics_runtime(public_search_metrics_runtime)
            .with_shutdown_token(application_shutdown_token.clone()),
        )
    } else {
        None
    };

    let mut http_runtime = HttpRuntime::new();
    if let Some(state) = &state {
        let router = if process_mode.runs_api() {
            routes::app(state.clone())
        } else {
            routes::worker_app(state.clone())
        };
        let listener = TcpListener::bind(bind_addr).await?;
        info!(
            event = "server.listening",
            surface = "public",
            addr = %bind_addr,
            process_mode = process_mode.as_str(),
        );
        http_runtime.spawn("public HTTP server", listener, router);
    }
    if let Some(search_state) = search_state {
        let listener = TcpListener::bind(search_bind_addr).await?;
        info!(
            event = "server.listening",
            surface = "internal_search",
            addr = %search_bind_addr,
            process_mode = process_mode.as_str(),
        );
        http_runtime.spawn(
            "internal search HTTP server",
            listener,
            routes::search_app(search_state),
        );
    }

    let mut process_runtime = if let Some(state) = &state {
        process_runtime::ProcessRuntime::start(state, application_shutdown_token)?
    } else {
        process_runtime::ProcessRuntime::search_only(metrics, application_shutdown_token)
    };

    enum StopReason {
        Http(anyhow::Error),
        Signal,
        Runtime(anyhow::Error),
    }

    let stop_reason = tokio::select! {
        error = http_runtime.wait_for_exit() => StopReason::Http(error),
        () = signals.wait() => StopReason::Signal,
        error = process_runtime.wait_for_critical_exit() => {
            tracing::error!(event = "process_runtime.critical_task_exited", %error);
            StopReason::Runtime(error)
        },
    };

    info!(event = "server.shutting_down");
    http_runtime.begin_shutdown();
    process_runtime.begin_shutdown();
    let http_result = http_runtime.join().await;

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
    pool.close().await;
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
