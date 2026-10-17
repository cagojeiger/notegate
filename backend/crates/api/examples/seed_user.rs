//! Dev-only bootstrap: create one user browser session and one Agent API key.
//! Runs migrations + ensures crypto key epochs first so it works against a
//! freshly-wiped database. Requires the same NOTEGATE_* env as the API.

use chrono::Utc;
use notegate_core::Config;
use notegate_core::security::PiiCrypto;
use notegate_db::browser_session_repo::{InsertBrowserSession, format_token, token_prefix};
use notegate_db::{
    AccountRepo, AgentRepo, ApiKeyRepo, BrowserSessionRepo, CryptoKeyEpochRepo, connect,
    run_migrations,
};
use notegate_model::account::AccountKind;
use notegate_model::{CreateAgent, CreateAgentApiKey, ResolveAttrs};
use notegate_service::agents::AgentService;
use uuid::Uuid;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let config = Config::load()?;
    let pool = connect(&config).await?;
    run_migrations(&pool).await?;

    let crypto = PiiCrypto::from_root_secrets(
        config.enc_root_key_id.clone(),
        &config.enc_root_secret,
        config.lookup_root_key_id.clone(),
        &config.lookup_root_secret,
    )?;
    CryptoKeyEpochRepo::new(pool.clone())
        .ensure_active(&crypto)
        .await?;

    let account_repo = AccountRepo::with_crypto_and_default_user_tier(
        pool.clone(),
        crypto.clone(),
        config.default_user_tier,
    );
    if !config.authgate_url.is_empty() {
        account_repo
            .ensure_legacy_identity_issuer(&config.authgate_url)
            .await?;
    }
    // Optional CI suffix creates independent identities without changing existing seeds.
    let suffix = std::env::args().nth(1);
    let (sub, email, name) = match suffix {
        Some(suffix) => {
            if suffix.is_empty()
                || suffix.len() > 64
                || !suffix
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
            {
                return Err(
                    "seed suffix must contain 1-64 ASCII letters, digits, or hyphens".into(),
                );
            }
            (
                format!("dev|seed-user-1-{suffix}"),
                format!("tester+{suffix}@example.com"),
                format!("Seed Tester {suffix}"),
            )
        }
        None => (
            "dev|seed-user-1".to_owned(),
            "tester@example.com".to_owned(),
            "Seed Tester".to_owned(),
        ),
    };
    let (account, _user) = account_repo
        .upsert_user_by_identity(&ResolveAttrs {
            issuer: config
                .oidc
                .as_ref()
                .map_or_else(|| config.authgate_url.clone(), |o| o.issuer.clone()),
            sub,
            email,
            name,
        })
        .await?;

    let browser_session = create_browser_session(&config, &pool, &crypto, account.id).await?;
    let agent_service = AgentService::with_crypto(
        AgentRepo::new(pool.clone()),
        ApiKeyRepo::with_lookup_key(pool.clone(), crypto.lookup_key_id(), crypto.version()),
        crypto.clone(),
    );
    let agent = agent_service
        .create_agent(
            AccountKind::User,
            account.id,
            CreateAgent {
                name: "e2e-agent".to_owned(),
            },
        )
        .await?;
    let agent_key = agent_service
        .create_key(
            AccountKind::User,
            account.id,
            CreateAgentApiKey {
                agent_id: agent.id,
                name: "e2e-agent-key".to_owned(),
                scopes: Vec::new(),
                expires_at: Some(Utc::now() + chrono::Duration::days(1)),
            },
        )
        .await?;

    println!("ACCOUNT_ID={}", account.id);
    println!("AGENT_API_KEY={}", agent_key.token);
    println!("BROWSER_SESSION={browser_session}");
    Ok(())
}

async fn create_browser_session(
    config: &Config,
    pool: &notegate_db::PgPool,
    crypto: &PiiCrypto,
    user_id: Uuid,
) -> Result<String, Box<dyn std::error::Error>> {
    let session_id = Uuid::new_v4();
    let secret = format!("{}{}", Uuid::new_v4().simple(), Uuid::new_v4().simple());
    let token_hash = crypto.browser_session_hash(&session_id.to_string(), &secret)?;
    let refresh_token =
        crypto.encrypt_browser_refresh_token(&session_id.to_string(), "e2e-refresh-token")?;
    let now = Utc::now();
    let validated_until = now + chrono::Duration::from_std(config.browser_session_ttl)?;
    let expires_at = now + chrono::Duration::from_std(config.browser_session_max_ttl)?;
    let token_prefix = token_prefix(session_id);

    let repo =
        BrowserSessionRepo::with_lookup_key(pool.clone(), crypto.lookup_key_id(), crypto.version());
    let args = InsertBrowserSession {
        session_id,
        user_id,
        token_prefix: &token_prefix,
        token_hash: &token_hash,
        refresh_token: &refresh_token,
        refresh_token_enc_key_id: crypto.enc_key_id(),
        refresh_token_enc_version: crypto.version(),
        validated_until,
        expires_at,
    };
    if let Some(oidc) = &config.oidc {
        repo.insert_local_session(args, &oidc.issuer).await?;
    } else {
        repo.insert_session(args).await?;
    }

    Ok(format_token(session_id, &secret))
}
