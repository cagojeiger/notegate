//! Dev-only bootstrap: create one user account and mint a user API key, then
//! print the plaintext token. Runs migrations + ensures crypto key epochs first
//! so it works against a freshly-wiped database. Reuses the real PiiCrypto so the
//! token hash matches the running API. Requires the same NOTEGATE_* env as the API.

use notegate_core::Config;
use notegate_core::security::PiiCrypto;
use notegate_db::{AccountRepo, ApiKeyRepo, CryptoKeyEpochRepo, connect, run_migrations};
use notegate_model::account::AccountKind;
use notegate_model::{CreateApiKey, ResolveAttrs};
use notegate_service::accounts::AccountService;

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

    let account_repo = AccountRepo::with_crypto(pool.clone(), crypto.clone());
    let (account, _user) = account_repo
        .upsert_user_by_sub(&ResolveAttrs {
            sub: "dev|seed-user-1".to_owned(),
            email: "tester@example.com".to_owned(),
            name: "Seed Tester".to_owned(),
        })
        .await?;

    let api_key_repo =
        ApiKeyRepo::with_lookup_key(pool.clone(), crypto.lookup_key_id(), crypto.version());
    let svc = AccountService::with_api_keys(account_repo, api_key_repo, crypto);
    let minted = svc
        .create_key(
            AccountKind::User,
            account.id,
            CreateApiKey {
                name: "fe-test-key".to_owned(),
                scopes: Vec::new(),
                expires_at: Some(chrono::Utc::now() + chrono::Duration::days(30)),
            },
        )
        .await?;

    println!("ACCOUNT_ID={}", account.id);
    println!("TOKEN={}", minted.token);
    Ok(())
}
