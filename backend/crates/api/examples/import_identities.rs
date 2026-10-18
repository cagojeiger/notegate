//! Offline, operator-only account migration. Input contains sensitive external subjects.
//! Supply the normal NoteGate config and a mode-0600 JSON manifest; never expose as an API.
use notegate_core::{Config, security::PiiCrypto};
use notegate_db::{AccountRepo, account_repo::IdentityLink};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let path = std::env::args()
        .nth(1)
        .ok_or_else(|| anyhow::anyhow!("usage: import_identities <verified-mapping.json>"))?;
    let config = Config::load()?;
    let bytes = std::fs::read(path)?;
    let links: Vec<IdentityLink> = serde_json::from_slice(&bytes)?;
    if links.iter().any(|l| l.issuer != config.oidc.issuer) {
        anyhow::bail!("all mappings must use the configured standalone issuer");
    }
    let pool = sqlx::postgres::PgPoolOptions::new()
        .max_connections(2)
        .connect(&config.database_url)
        .await?;
    let crypto = PiiCrypto::from_root_secrets(
        config.enc_root_key_id.clone(),
        &config.enc_root_secret,
        config.lookup_root_key_id.clone(),
        &config.lookup_root_secret,
    )?;
    AccountRepo::with_crypto(pool, crypto)
        .import_identity_links(&links)
        .await?;
    println!(
        "Linked {} verified identities; existing account IDs and ownership are unchanged.",
        links.len()
    );
    Ok(())
}
