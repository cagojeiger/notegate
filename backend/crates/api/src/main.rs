use std::io;

use notegate_core::Config;
use notegate_core::security::PiiCrypto;
use tokio::net::TcpListener;
#[cfg(unix)]
use tokio::signal::unix::{SignalKind, signal};
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
mod mcp;
mod metadata_write_behind;
mod object_storage;
mod object_upload_flow;
mod observability;
mod openapi;
mod page;
mod periodic_worker;
mod process_runtime;
mod public_v2;
mod reconciliations;
mod rest;
mod routes;
mod state;
mod usage_bootstrap;

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
    let http = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        .redirect(reqwest::redirect::Policy::none())
        .build()?;
    let jwks_url = format!("{}/keys", config.authgate_url);
    // The db-backed identity resolver: account_repo resolves users, while
    // api_key_repo resolves API-key ownership and agent callers in one query.
    notegate_service::cursor::configure_signing_key(pii_crypto.session_signing_key())?;
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
    let resolver =
        notegate_service::identity::Resolver::new(account_repo, api_key_repo, pii_crypto.clone());
    let config = std::sync::Arc::new(config);
    let jwt = std::sync::Arc::new(auth::jwt::JwtAuthority::from_url(&config, jwks_url));
    let oidc = std::sync::Arc::new(auth::oidc::OidcProvider::new(&config, http.clone()));
    let application_shutdown_token = CancellationToken::new();
    let state = AppState::new(
        pool.clone(),
        config.clone(),
        jwt,
        oidc,
        std::sync::Arc::new(resolver),
        http,
        pii_crypto,
    )
    .with_metrics(metrics)
    .with_shutdown_token(application_shutdown_token.clone());
    let router = if process_mode.runs_api() {
        routes::app(state.clone())
    } else {
        routes::worker_app(state.clone())
    };

    let listener = TcpListener::bind(bind_addr).await?;
    info!(
        event = "server.listening",
        addr = %bind_addr,
        process_mode = process_mode.as_str(),
    );

    let mut process_runtime =
        process_runtime::ProcessRuntime::start(&state, application_shutdown_token)?;

    let http_shutdown_token = CancellationToken::new();
    let http_shutdown = http_shutdown_token.clone().cancelled_owned();
    let server = async move {
        axum::serve(listener, router)
            .with_graceful_shutdown(http_shutdown)
            .await
    };
    tokio::pin!(server);

    enum StopReason {
        Http(io::Result<()>),
        Signal,
        Runtime(anyhow::Error),
    }

    let stop_reason = tokio::select! {
        result = &mut server => StopReason::Http(result),
        () = signals.wait() => StopReason::Signal,
        error = process_runtime.wait_for_critical_exit() => {
            tracing::error!(event = "process_runtime.critical_task_exited", %error);
            StopReason::Runtime(error)
        },
    };

    info!(event = "server.shutting_down");
    http_shutdown_token.cancel();
    process_runtime.begin_shutdown();

    let server_result = match stop_reason {
        StopReason::Http(result) => result.map_err(anyhow::Error::from),
        StopReason::Signal => server.await.map_err(anyhow::Error::from),
        StopReason::Runtime(runtime_error) => {
            if let Err(error) = server.await {
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
