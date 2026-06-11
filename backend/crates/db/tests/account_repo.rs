mod common;

use common::TestDb;
use notegate_db::AccountRepo;
use notegate_model::ResolveAttrs;

fn attrs(sub: &str, email: &str, name: &str) -> ResolveAttrs {
    ResolveAttrs {
        sub: sub.to_owned(),
        email: email.to_owned(),
        name: name.to_owned(),
    }
}

#[tokio::test]
async fn upsert_user_creates_and_updates_same_account() -> Result<(), Box<dyn std::error::Error>> {
    let Some(db) = TestDb::setup().await? else {
        return Ok(());
    };
    let repo = AccountRepo::new(db.pool.clone());

    let (first, first_user) = repo
        .upsert_user_by_sub(&attrs("sub-1", "first@example.test", "First"))
        .await?;
    let (second, second_user) = repo
        .upsert_user_by_sub(&attrs("sub-1", "second@example.test", "Second"))
        .await?;

    assert_eq!(first.id, second.id);
    assert_eq!(second.display_name, "Second");
    assert_eq!(first_user.id, second_user.id);
    assert_eq!(second_user.email.as_deref(), Some("second@example.test"));

    db.cleanup().await;
    Ok(())
}

#[tokio::test]
async fn soft_deleted_user_cannot_be_resurrected_before_purge()
-> Result<(), Box<dyn std::error::Error>> {
    let Some(db) = TestDb::setup().await? else {
        return Ok(());
    };
    let repo = AccountRepo::new(db.pool.clone());
    let (account, _) = repo
        .upsert_user_by_sub(&attrs("delete-sub", "delete@example.test", "Delete Me"))
        .await?;

    repo.soft_delete_user(account.id, account.id).await?;
    let deleted = repo
        .find_account(account.id)
        .await?
        .ok_or("account shell missing")?;
    assert!(!deleted.is_active);
    assert!(
        repo.upsert_user_by_sub(&attrs("delete-sub", "delete@example.test", "Return"))
            .await
            .is_err()
    );

    db.cleanup().await;
    Ok(())
}
