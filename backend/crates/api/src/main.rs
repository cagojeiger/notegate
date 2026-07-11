use std::io;

use notegate_core::Config;
use notegate_core::security::PiiCrypto;
use tokio::net::TcpListener;
#[cfg(unix)]
use tokio::signal::unix::{SignalKind, signal};
use tokio_util::sync::CancellationToken;
use tracing::info;
use tracing_subscriber::EnvFilter;

mod auth;
mod error;
mod identity;
mod mcp;
mod openapi;
mod page;
mod periodic_worker;
mod purge_worker;
mod rest;
mod routes;
mod state;
mod usage_reconcile_worker;

use state::AppState;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    if std::env::args().any(|arg| arg == "--print-openapi") {
        println!("{}", openapi::json_pretty()?);
        return Ok(());
    }

    // Load `.env` for local development; absence is fine in production.
    let _ = dotenvy::dotenv();
    init_tracing();

    let config = Config::load()?;

    // fail-fast: install the SIGTERM handler during boot so a failure here
    // aborts startup instead of leaving us without graceful shutdown.
    let signals = ShutdownSignals::install()?;

    let pool = notegate_db::connect(&config).await?;
    notegate_db::run_migrations(&pool).await?;
    info!(
        event = "db.ready",
        max_connections = config.db_max_connections
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
    // The db-backed identity resolver: account_repo resolves users, api_key_repo
    // resolves key ownership, and agent_repo resolves agent callers.
    notegate_service::cursor::configure_signing_key(pii_crypto.session_signing_key())?;
    let account_repo = notegate_db::AccountRepo::with_crypto_and_default_user_tier(
        pool.clone(),
        pii_crypto.clone(),
        config.default_user_tier,
    );
    let agent_repo = notegate_db::AgentRepo::new(pool.clone());
    let api_key_repo = notegate_db::ApiKeyRepo::with_lookup_key(
        pool.clone(),
        pii_crypto.lookup_key_id(),
        pii_crypto.version(),
    );
    let resolver = notegate_service::identity::Resolver::new(
        account_repo,
        agent_repo,
        api_key_repo,
        pii_crypto.clone(),
    );
    let config = std::sync::Arc::new(config);
    let jwt = std::sync::Arc::new(auth::jwt::JwtAuthority::from_url(&config, jwks_url));
    let oidc = std::sync::Arc::new(auth::oidc::OidcProvider::new(&config, http.clone()));
    let state = AppState::new(
        pool.clone(),
        config.clone(),
        jwt,
        oidc,
        std::sync::Arc::new(resolver),
        http,
        pii_crypto,
    );

    let listener = TcpListener::bind(bind_addr).await?;
    info!(event = "server.listening", addr = %bind_addr);

    let background_shutdown_token = CancellationToken::new();
    let purge_worker = purge_worker::spawn(pool.clone(), background_shutdown_token.clone());
    let usage_reconcile_worker =
        usage_reconcile_worker::spawn(pool.clone(), background_shutdown_token.clone());

    let http_shutdown_token = CancellationToken::new();
    let http_shutdown = http_shutdown_token.clone().cancelled_owned();
    let server = async move {
        axum::serve(listener, routes::app(state))
            .with_graceful_shutdown(http_shutdown)
            .await
    };
    tokio::pin!(server);

    let server_result: Option<io::Result<()>> = tokio::select! {
        result = &mut server => Some(result),
        () = signals.wait() => None,
    };

    info!(event = "server.shutting_down");
    http_shutdown_token.cancel();
    background_shutdown_token.cancel();

    let server_result = match server_result {
        Some(result) => result,
        None => server.await,
    };

    if let Err(error) = purge_worker.await {
        tracing::error!(event = "purge_worker.join_failed", %error);
    }
    if let Err(error) = usage_reconcile_worker.await {
        tracing::error!(event = "usage_reconcile_worker.join_failed", %error);
    }

    // Workers drain their current transaction before returning. Close the pool
    // only after every worker has joined so no background query is interrupted.
    pool.close().await;
    info!(event = "shutdown.complete");

    server_result.map_err(anyhow::Error::from)
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

fn init_tracing() {
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_error| {
        EnvFilter::new("notegate_api=info,notegate_db=info,tower_http=info")
    });

    let result = if std::env::var("LOG_FORMAT").as_deref() == Ok("json") {
        tracing_subscriber::fmt()
            .json()
            .with_env_filter(filter)
            .try_init()
    } else {
        tracing_subscriber::fmt().with_env_filter(filter).try_init()
    };

    if let Err(error) = result {
        eprintln!("failed to initialize tracing: {error}");
    }
}
