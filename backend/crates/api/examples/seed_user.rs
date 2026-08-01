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
    let (account, _user) = account_repo
        .upsert_user_by_sub(&ResolveAttrs {
            sub: "dev|seed-user-1".to_owned(),
            email: "tester@example.com".to_owned(),
            name: "Seed Tester".to_owned(),
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

    BrowserSessionRepo::with_lookup_key(pool.clone(), crypto.lookup_key_id(), crypto.version())
        .insert_session(InsertBrowserSession {
            session_id,
            user_id,
            token_prefix: &token_prefix,
            token_hash: &token_hash,
            refresh_token: &refresh_token,
            refresh_token_enc_key_id: crypto.enc_key_id(),
            refresh_token_enc_version: crypto.version(),
            validated_until,
            expires_at,
        })
        .await?;

    Ok(format_token(session_id, &secret))
}
