#![allow(
    clippy::unwrap_in_result,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic
)]
mod common;
use common::{TestDb, space_with_root};
use notegate_core::{Error, security::PiiCrypto};
use notegate_db::{FilesRepo, TextMutationKind, files::revisions};
use notegate_model::files::{StoredContent, WriteTextBody};
use uuid::Uuid;

type TestResult = Result<(), Box<dyn std::error::Error>>;
fn body(value: &str) -> StoredContent {
    StoredContent {
        body: WriteTextBody::Plain(value.to_owned()),
        content_sha256: format!("{value:0<64}"),
        byte_len: value.len() as i64,
        line_count: 1,
    }
}
async fn save(
    repo: &FilesRepo,
    space: Uuid,
    node: Uuid,
    actor: Uuid,
    value: &str,
) -> Result<(), Error> {
    repo.save_text_content(
        space,
        node,
        &body(value),
        None,
        actor,
        TextMutationKind::Write,
    )
    .await?;
    Ok(())
}

#[tokio::test]
async fn snapshots_are_atomic_encrypted_and_guarded() -> TestResult {
    let Some(db) = TestDb::setup().await? else {
        return Ok(());
    };
    let (actor, space, root) = space_with_root(&db.pool, "revision-atomic").await?;
    let repo = FilesRepo::new(db.pool.clone());
    let (node, _) = repo
        .insert_text(space, root, "note.md", &body("a"), actor)
        .await?;
    save(&repo, space, node.id, actor, "b").await?;
    let page = repo.list_text_revisions(space, node.id, 10, None).await?;
    assert_eq!(page.revisions.len(), 1);
    assert_eq!(
        repo.read_text_revision(space, node.id, page.revisions[0].id)
            .await?
            .content,
        "a"
    );
    let ciphertext: Vec<u8> = sqlx::query_scalar("SELECT ciphertext FROM text_revisions")
        .fetch_one(&db.pool)
        .await?;
    assert_ne!(ciphertext, b"a");
    save(&repo, space, node.id, actor, "b").await?;
    assert!(matches!(
        repo.save_text_content(
            space,
            node.id,
            &body("c"),
            Some(&body("a").content_sha256),
            actor,
            TextMutationKind::Write
        )
        .await,
        Err(Error::Conflict(_))
    ));
    sqlx::raw_sql("CREATE FUNCTION reject_revision_write() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN IF NEW.content_text = 'c' THEN RAISE EXCEPTION 'injected write failure'; END IF; RETURN NEW; END $$; CREATE TRIGGER reject_revision_write BEFORE UPDATE ON text_objects FOR EACH ROW EXECUTE FUNCTION reject_revision_write();").execute(&db.pool).await?;
    assert!(save(&repo, space, node.id, actor, "c").await.is_err());
    assert_eq!(
        repo.list_text_revisions(space, node.id, 10, None)
            .await?
            .revisions
            .len(),
        1
    );
    assert_eq!(
        repo.find_text(space, node.id)
            .await?
            .unwrap()
            .1
            .content
            .as_deref(),
        Some("b")
    );
    let usage: i64 =
        sqlx::query_scalar("SELECT stored_bytes FROM text_revision_usage WHERE space_id=$1")
            .bind(space)
            .fetch_one(&db.pool)
            .await?;
    let actual: i64 = sqlx::query_scalar(
        "SELECT SUM(stored_bytes)::bigint FROM text_revisions WHERE space_id=$1",
    )
    .bind(space)
    .fetch_one(&db.pool)
    .await?;
    assert_eq!(usage, actual);
    db.cleanup().await;
    Ok(())
}

#[tokio::test]
async fn grouping_preserves_boundaries_and_cleanup_is_repeatable() -> TestResult {
    let Some(db) = TestDb::setup().await? else {
        return Ok(());
    };
    let (actor, space, root) = space_with_root(&db.pool, "revision-group").await?;
    let repo = FilesRepo::new(db.pool.clone());
    let (node, _) = repo
        .insert_text(space, root, "note.md", &body("a"), actor)
        .await?;
    let editing = repo
        .clone()
        .with_revision_context("browser", Some(Uuid::new_v4()));
    for value in ["b", "c", "d"] {
        save(&editing, space, node.id, actor, value).await?;
    }
    let ai = repo
        .clone()
        .with_revision_context("mcp", Some(Uuid::new_v4()));
    save(&ai, space, node.id, actor, "e").await?;
    save(&repo, space, node.id, actor, "f").await?;
    let flags: Vec<(String, bool)> = sqlx::query_as(
        "SELECT content_sha256,checkpoint FROM text_revisions ORDER BY superseded_at",
    )
    .fetch_all(&db.pool)
    .await?;
    assert_eq!(
        flags.iter().map(|v| v.1).collect::<Vec<_>>(),
        vec![true, false, false, true, true]
    );
    assert_eq!(revisions::cleanup(&db.pool).await?, 0);
    // Advance only stored timestamps; no sleeps or changes to the production clock.
    sqlx::query("UPDATE text_revisions SET superseded_at=superseded_at-interval '25 hours', cleanup_at=cleanup_at-interval '25 hours'").execute(&db.pool).await?;
    assert_eq!(revisions::cleanup(&db.pool).await?, 2);
    assert_eq!(revisions::cleanup(&db.pool).await?, 0);
    let left = repo.list_text_revisions(space, node.id, 10, None).await?;
    assert_eq!(
        left.revisions
            .iter()
            .map(|r| r.content_sha256.clone())
            .collect::<Vec<_>>(),
        vec![
            body("e").content_sha256,
            body("d").content_sha256,
            body("a").content_sha256
        ]
    );
    sqlx::query("UPDATE text_revisions SET cleanup_at=cleanup_at-interval '31 days'")
        .execute(&db.pool)
        .await?;
    assert_eq!(revisions::cleanup(&db.pool).await?, 3);
    assert_eq!(
        repo.find_text(space, node.id)
            .await?
            .unwrap()
            .1
            .content
            .as_deref(),
        Some("f")
    );
    let usage: i64 =
        sqlx::query_scalar("SELECT stored_bytes FROM text_revision_usage WHERE space_id=$1")
            .bind(space)
            .fetch_one(&db.pool)
            .await?;
    assert_eq!(usage, 0);
    db.cleanup().await;
    Ok(())
}

#[tokio::test]
async fn time_actor_and_channel_boundaries_cannot_coalesce() -> TestResult {
    let Some(db) = TestDb::setup().await? else {
        return Ok(());
    };
    let (actor, space, root) = space_with_root(&db.pool, "revision-time").await?;
    let other =
        common::insert_user_account(&db.pool, "revision-other", "other@example.com").await?;
    let session = Some(Uuid::new_v4());
    let repo = FilesRepo::new(db.pool.clone()).with_revision_context("browser", session);
    let (node, _) = repo
        .insert_text(space, root, "note.md", &body("a"), actor)
        .await?;
    save(&repo, space, node.id, actor, "b").await?;
    sqlx::query(
        "UPDATE text_objects SET revision_written_at=now()-interval '2 minutes' WHERE node_id=$1",
    )
    .bind(node.id)
    .execute(&db.pool)
    .await?;
    save(&repo, space, node.id, actor, "c").await?;
    sqlx::query("UPDATE text_objects SET revision_group_started_at=now()-interval '10 minutes' WHERE node_id=$1").bind(node.id).execute(&db.pool).await?;
    save(&repo, space, node.id, actor, "d").await?;
    save(&repo, space, node.id, other, "e").await?;
    save(
        &repo.clone().with_revision_context("mcp", session),
        space,
        node.id,
        other,
        "f",
    )
    .await?;
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM text_revisions WHERE NOT checkpoint")
        .fetch_one(&db.pool)
        .await?;
    assert_eq!(count, 0);
    db.cleanup().await;
    Ok(())
}

#[tokio::test]
async fn quota_failure_keeps_current_and_cascade_releases_history() -> TestResult {
    let Some(db) = TestDb::setup().await? else {
        return Ok(());
    };
    let (actor, space, root) = space_with_root(&db.pool, "revision-quota").await?;
    let repo = FilesRepo::new(db.pool.clone());
    let (node, _) = repo
        .insert_text(space, root, "note.md", &body("a"), actor)
        .await?;
    save(&repo, space, node.id, actor, "b").await?;
    sqlx::query("UPDATE text_revision_usage SET stored_bytes=$1 WHERE space_id=$2")
        .bind(revisions::SPACE_HISTORY_BYTES)
        .bind(space)
        .execute(&db.pool)
        .await?;
    assert!(matches!(
        save(&repo, space, node.id, actor, "c").await,
        Err(Error::Conflict(_))
    ));
    assert_eq!(
        repo.find_text(space, node.id)
            .await?
            .unwrap()
            .1
            .content
            .as_deref(),
        Some("b")
    );
    sqlx::query("UPDATE text_revision_usage SET stored_bytes=(SELECT SUM(stored_bytes)::bigint FROM text_revisions WHERE space_id=$1) WHERE space_id=$1").bind(space).execute(&db.pool).await?;
    sqlx::query("DELETE FROM nodes WHERE id=$1")
        .bind(node.id)
        .execute(&db.pool)
        .await?;
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM text_revisions")
        .fetch_one(&db.pool)
        .await?;
    let usage: i64 =
        sqlx::query_scalar("SELECT stored_bytes FROM text_revision_usage WHERE space_id=$1")
            .bind(space)
            .fetch_one(&db.pool)
            .await?;
    assert_eq!((count, usage), (0, 0));
    db.cleanup().await;
    Ok(())
}

#[tokio::test]
async fn encryption_identity_tampering_and_old_current_protection() -> TestResult {
    let Some(db) = TestDb::setup().await? else {
        return Ok(());
    };
    let (actor, space, root) = space_with_root(&db.pool, "revision-crypto").await?;
    let repo = FilesRepo::new(db.pool.clone());
    let (node, _) = repo
        .insert_text(space, root, "note.md", &body("a"), actor)
        .await?;
    sqlx::query(
        "UPDATE text_objects SET revision_written_at=now()-interval '90 days' WHERE node_id=$1",
    )
    .bind(node.id)
    .execute(&db.pool)
    .await?;
    save(&repo, space, node.id, actor, "b").await?;
    assert_eq!(revisions::cleanup(&db.pool).await?, 0);
    let page = repo.list_text_revisions(space, node.id, 10, None).await?;
    let id = page.revisions[0].id;
    assert_eq!(
        repo.read_text_revision(space, node.id, id).await?.content,
        "a"
    );
    let raw: (Vec<u8>, Vec<u8>, String, i32) = sqlx::query_as(
        "SELECT ciphertext,nonce,enc_key_id,enc_version FROM text_revisions WHERE id=$1",
    )
    .bind(id)
    .fetch_one(&db.pool)
    .await?;
    assert!(
        PiiCrypto::test()
            .decrypt_text_content(
                &space.to_string(),
                &format!("{}/revisions/{id}", Uuid::new_v4()),
                &raw.2,
                raw.3,
                &notegate_core::security::EncryptedField {
                    ciphertext: raw.0,
                    nonce: raw.1
                }
            )
            .is_err()
    );
    let replacement = Uuid::new_v4();
    sqlx::query("UPDATE text_revisions SET id=$2 WHERE id=$1")
        .bind(id)
        .bind(replacement)
        .execute(&db.pool)
        .await?;
    assert!(
        repo.read_text_revision(space, node.id, replacement)
            .await
            .is_err()
    );
    db.cleanup().await;
    Ok(())
}

#[tokio::test]
async fn simultaneous_guarded_writes_only_record_the_winner() -> TestResult {
    let Some(db) = TestDb::setup().await? else {
        return Ok(());
    };
    let (actor, space, root) = space_with_root(&db.pool, "revision-race").await?;
    let repo = FilesRepo::new(db.pool.clone());
    let original = body("a");
    let (node, _) = repo
        .insert_text(space, root, "note.md", &original, actor)
        .await?;
    let b = body("b");
    let c = body("c");
    let (left, right) = tokio::join!(
        repo.save_text_content(
            space,
            node.id,
            &b,
            Some(&original.content_sha256),
            actor,
            TextMutationKind::Write
        ),
        repo.save_text_content(
            space,
            node.id,
            &c,
            Some(&original.content_sha256),
            actor,
            TextMutationKind::Write
        )
    );
    assert_eq!(usize::from(left.is_ok()) + usize::from(right.is_ok()), 1);
    assert_eq!(
        repo.list_text_revisions(space, node.id, 10, None)
            .await?
            .revisions
            .len(),
        1
    );
    db.cleanup().await;
    Ok(())
}

#[tokio::test]
async fn migration_backfills_existing_documents_without_inventing_history() -> TestResult {
    let Some(db) = TestDb::setup_before(42).await? else {
        return Ok(());
    };
    let (actor, space, root) = space_with_root(&db.pool, "revision-migration").await?;
    let repo = FilesRepo::new(db.pool.clone());
    let (node, _) = repo
        .insert_text(space, root, "legacy.md", &body("a"), actor)
        .await?;
    sqlx::query("UPDATE text_objects SET updated_at=now()-interval '90 days' WHERE node_id=$1")
        .bind(node.id)
        .execute(&db.pool)
        .await?;
    db.apply_migration(42).await?;
    let backfilled: bool=sqlx::query_scalar("SELECT revision_author_id=updated_by_account_id AND revision_written_at=updated_at FROM text_objects WHERE node_id=$1").bind(node.id).fetch_one(&db.pool).await?;
    assert!(backfilled);
    assert!(
        repo.list_text_revisions(space, node.id, 10, None)
            .await?
            .revisions
            .is_empty()
    );
    save(&repo, space, node.id, actor, "b").await?;
    assert_eq!(revisions::cleanup(&db.pool).await?, 0);
    let page = repo.list_text_revisions(space, node.id, 10, None).await?;
    assert_eq!(
        repo.read_text_revision(space, node.id, page.revisions[0].id)
            .await?
            .content,
        "a"
    );
    db.cleanup().await;
    Ok(())
}

#[tokio::test]
async fn cleanup_is_bounded_and_space_cascade_removes_usage() -> TestResult {
    let Some(db) = TestDb::setup().await? else {
        return Ok(());
    };
    let (actor, space, root) = space_with_root(&db.pool, "revision-batch").await?;
    let repo = FilesRepo::new(db.pool.clone());
    let (node, _) = repo
        .insert_text(space, root, "note.md", &body("a"), actor)
        .await?;
    for index in 0..105 {
        save(
            &repo,
            space,
            node.id,
            actor,
            if index % 2 == 0 { "b" } else { "a" },
        )
        .await?;
    }
    sqlx::query("UPDATE text_revisions SET cleanup_at=now()-interval '1 second'")
        .execute(&db.pool)
        .await?;
    assert_eq!(revisions::cleanup(&db.pool).await?, 100);
    assert_eq!(revisions::cleanup(&db.pool).await?, 5);
    assert_eq!(revisions::cleanup(&db.pool).await?, 0);
    save(&repo, space, node.id, actor, "c").await?;
    sqlx::query("DELETE FROM spaces WHERE id=$1")
        .bind(space)
        .execute(&db.pool)
        .await?;
    let remaining: (i64, i64) = sqlx::query_as(
        "SELECT (SELECT count(*) FROM text_revisions),(SELECT count(*) FROM text_revision_usage)",
    )
    .fetch_one(&db.pool)
    .await?;
    assert_eq!(remaining, (0, 0));
    db.cleanup().await;
    Ok(())
}
