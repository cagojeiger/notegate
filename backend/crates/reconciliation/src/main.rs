use notegate_core::limits::Limits;
use notegate_core::security::PiiCrypto;
use notegate_db::{CryptoKeyEpochRepo, FilesRepo, LinkIndexRepo};
use notegate_service::link_index::LinkIndexService;
use secrecy::{ExposeSecret, SecretString};
use tokio_util::sync::CancellationToken;
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let _ = dotenvy::dotenv();
    let config = WorkerConfig::load()?;
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .json()
        .init();

    let pool =
        notegate_db::connect_with(&config.database_url, config.db_max_connections, false).await?;
    notegate_db::check_readiness(&pool).await?;
    let crypto = PiiCrypto::from_root_secrets(
        config.enc_root_key_id.clone(),
        &config.enc_root_secret,
        config.lookup_root_key_id.clone(),
        &config.lookup_root_secret,
    )?;
    CryptoKeyEpochRepo::new(pool.clone())
        .verify_active(&crypto)
        .await?;
    let files = FilesRepo::with_limits_and_crypto(pool.clone(), Limits::default(), crypto);
    let links = LinkIndexService::new(LinkIndexRepo::new(pool.clone()), files);
    let shutdown = CancellationToken::new();
    let shutdown_signal = spawn_shutdown_signal(shutdown.clone());

    tracing::info!(event = "reconciliation_worker.started");
    notegate_reconciliation::run(&pool, &links, shutdown).await?;
    shutdown_signal.abort();
    pool.close().await;
    tracing::info!(event = "reconciliation_worker.stopped");
    Ok(())
}

struct WorkerConfig {
    database_url: String,
    db_max_connections: u32,
    enc_root_key_id: String,
    enc_root_secret: SecretString,
    lookup_root_key_id: String,
    lookup_root_secret: SecretString,
}

impl WorkerConfig {
    fn load() -> anyhow::Result<Self> {
        let database_url = required_env("NOTEGATE_DATABASE_URL")?;
        let db_max_connections = match std::env::var("NOTEGATE_DB_MAX_CONNECTIONS") {
            Ok(value) => value.parse::<u32>()?,
            Err(std::env::VarError::NotPresent) => 10,
            Err(error) => return Err(error.into()),
        };
        let enc_root_key_id = required_env("NOTEGATE_ENC_ROOT_KEY_ID")?;
        let enc_root_secret = SecretString::from(required_env("NOTEGATE_ENC_ROOT_SECRET")?);
        let lookup_root_key_id = required_env("NOTEGATE_LOOKUP_ROOT_KEY_ID")?;
        let lookup_root_secret = SecretString::from(required_env("NOTEGATE_LOOKUP_ROOT_SECRET")?);

        if !(2..=256).contains(&db_max_connections) {
            anyhow::bail!("NOTEGATE_DB_MAX_CONNECTIONS must be between 2 and 256");
        }
        if enc_root_secret.expose_secret().len() < 32
            || lookup_root_secret.expose_secret().len() < 32
        {
            anyhow::bail!("reconciliation worker root secrets must be at least 32 bytes");
        }
        if enc_root_key_id == lookup_root_key_id
            || enc_root_secret.expose_secret() == lookup_root_secret.expose_secret()
        {
            anyhow::bail!("reconciliation worker ENC and LOOKUP roots must be distinct");
        }

        Ok(Self {
            database_url,
            db_max_connections,
            enc_root_key_id,
            enc_root_secret,
            lookup_root_key_id,
            lookup_root_secret,
        })
    }
}

fn required_env(name: &str) -> anyhow::Result<String> {
    let value = std::env::var(name).map_err(|_| anyhow::anyhow!("{name} is required"))?;
    if value.is_empty() {
        anyhow::bail!("{name} must not be empty");
    }
    Ok(value)
}

fn spawn_shutdown_signal(shutdown: CancellationToken) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        #[cfg(unix)]
        {
            use tokio::signal::unix::{SignalKind, signal};

            let terminate = signal(SignalKind::terminate());
            match terminate {
                Ok(mut terminate) => {
                    tokio::select! {
                        _ = tokio::signal::ctrl_c() => {}
                        _ = terminate.recv() => {}
                    }
                }
                Err(error) => {
                    tracing::error!(event = "reconciliation.signal_install_failed", %error);
                    let _ = tokio::signal::ctrl_c().await;
                }
            }
        }
        #[cfg(not(unix))]
        {
            let _ = tokio::signal::ctrl_c().await;
        }
        shutdown.cancel();
    })
}
