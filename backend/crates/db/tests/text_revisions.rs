#![allow(
    clippy::unwrap_in_result,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic
)]
mod common;
use chrono::{DateTime, Duration, Utc};
use common::{TestDb, legacy_space_with_root, space_with_root};
use notegate_core::{Error, security::PiiCrypto};
use notegate_db::{FilesRepo, SpaceRepo, TextMutationKind, files::revisions};
use notegate_model::files::{CreateFolder, StoredContent, WriteTextBody};
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

fn policy_time() -> DateTime<Utc> {
    DateTime::parse_from_rfc3339("2026-01-01T00:00:00Z")
        .unwrap()
        .with_timezone(&Utc)
}

type RevisionHead = (Uuid, DateTime<Utc>, Uuid, DateTime<Utc>);
async fn revision_head(pool: &sqlx::PgPool, node: Uuid) -> Result<RevisionHead, sqlx::Error> {
    sqlx::query_as(
        "SELECT revision_id, revision_written_at, revision_group_id, revision_group_started_at \
         FROM text_objects WHERE node_id=$1",
    )
    .bind(node)
    .fetch_one(pool)
    .await
}

async fn assert_history_usage(pool: &sqlx::PgPool, space: Uuid) -> TestResult {
    let (recorded, actual): (i64, i64) = sqlx::query_as(
        "SELECT u.stored_bytes, (SELECT COALESCE(SUM(r.stored_bytes),0)::bigint \
         FROM text_revisions r WHERE r.space_id=u.space_id) \
         FROM text_revision_usage u WHERE u.space_id=$1",
    )
    .bind(space)
    .fetch_one(pool)
    .await?;
    assert_eq!(recorded, actual);
    Ok(())
}

#[tokio::test]
async fn editing_time_limits_are_strict_at_microsecond_boundaries() -> TestResult {
    let Some(db) = TestDb::setup().await? else {
        return Ok(());
    };
    let (actor, space, root) = space_with_root(&db.pool, "revision-time-boundaries").await?;
    let start = policy_time() + Duration::seconds(1);
    for (limit, seconds) in [("idle", 120), ("group", 600)] {
        for offset in [-1, 0, 1] {
            let repo = FilesRepo::new(db.pool.clone()).with_revision_time(policy_time());
            let (node, _) = repo
                .insert_text(
                    space,
                    root,
                    &format!("{limit}-{offset}.md"),
                    &body("a"),
                    actor,
                )
                .await?;
            assert_eq!(revision_head(&db.pool, node.id).await?.1, policy_time());
            let editing = repo.with_revision_context("browser", Some(Uuid::new_v4()));
            save(
                &editing.clone().with_revision_time(start),
                space,
                node.id,
                actor,
                "b",
            )
            .await?;
            let first = revision_head(&db.pool, node.id).await?;
            if limit == "group" {
                // Keep idle time under two minutes while the group approaches ten minutes.
                for elapsed in [90, 180, 270, 360, 450, 540] {
                    save(
                        &editing
                            .clone()
                            .with_revision_time(start + Duration::seconds(elapsed)),
                        space,
                        node.id,
                        actor,
                        &format!("edit-{elapsed}"),
                    )
                    .await?;
                    assert_eq!(revision_head(&db.pool, node.id).await?.3, start);
                }
            }
            let previous = revision_head(&db.pool, node.id).await?;
            let now = start + Duration::seconds(seconds) + Duration::microseconds(offset);
            save(
                &editing.with_revision_time(now),
                space,
                node.id,
                actor,
                "final",
            )
            .await?;
            let head = revision_head(&db.pool, node.id).await?;
            let continues = offset < 0;
            assert_eq!(head.1, now, "{limit}: offset {offset}");
            assert_eq!(head.2 == first.2, continues, "{limit}: offset {offset}");
            assert_eq!(head.3, if continues { start } else { now });
            let snapshot: (DateTime<Utc>, DateTime<Utc>, bool, DateTime<Utc>) = sqlx::query_as(
                "SELECT written_at, superseded_at, checkpoint, cleanup_at FROM text_revisions WHERE id=$1",
            ).bind(previous.0).fetch_one(&db.pool).await?;
            assert_eq!(snapshot.0, previous.1);
            assert_eq!(snapshot.1, now);
            assert_eq!(snapshot.2, !continues, "{limit}: offset {offset}");
            assert_eq!(
                snapshot.3,
                now + Duration::days(if continues { 1 } else { 30 })
            );
            assert_history_usage(&db.pool, space).await?;
        }
    }
    db.cleanup().await;
    Ok(())
}

#[tokio::test]
async fn retention_expires_at_the_exact_replacement_based_deadline() -> TestResult {
    let Some(db) = TestDb::setup().await? else {
        return Ok(());
    };
    for (checkpoint, days) in [(false, 1), (true, 30)] {
        for offset in [-1, 0, 1] {
            // Separate Spaces keep the global cleanup selector independent for each case.
            let (actor, space, root) =
                space_with_root(&db.pool, &format!("revision-retention-{days}-{offset}")).await?;
            let repo = FilesRepo::new(db.pool.clone()).with_revision_time(policy_time());
            let (node, _) = repo
                .insert_text(space, root, "note.md", &body("a"), actor)
                .await?;
            // The initial body is old; retention must still start when it is replaced.
            let replacement = policy_time() + Duration::days(90);
            let editing = repo.with_revision_context("browser", Some(Uuid::new_v4()));
            save(
                &editing
                    .clone()
                    .with_revision_time(replacement - Duration::seconds(1)),
                space,
                node.id,
                actor,
                "b",
            )
            .await?;
            let target = revision_head(&db.pool, node.id).await?.0;
            if !checkpoint {
                save(
                    &editing.clone().with_revision_time(replacement),
                    space,
                    node.id,
                    actor,
                    "c",
                )
                .await?;
            }
            let (id, written_at, superseded_at, cleanup_at): (
                Uuid,
                DateTime<Utc>,
                DateTime<Utc>,
                DateTime<Utc>,
            ) = sqlx::query_as(
                "SELECT id, written_at, superseded_at, cleanup_at FROM text_revisions \
                 WHERE space_id=$1 AND checkpoint=$2",
            )
            .bind(space)
            .bind(checkpoint)
            .fetch_one(&db.pool)
            .await?;
            let expected_replacement = if checkpoint {
                replacement - Duration::seconds(1)
            } else {
                replacement
            };
            let deadline = expected_replacement + Duration::days(days);
            assert_eq!(
                written_at,
                if checkpoint {
                    policy_time()
                } else {
                    replacement - Duration::seconds(1)
                }
            );
            assert_eq!(superseded_at, expected_replacement);
            assert_eq!(cleanup_at, deadline);
            if !checkpoint {
                assert_eq!(id, target);
            }
            let before = revision_head(&db.pool, node.id).await?;
            let cutoff = deadline + Duration::microseconds(offset);
            assert_eq!(
                revisions::cleanup_at(&db.pool, cutoff).await?,
                u64::from(offset >= 0),
                "{days} days: offset {offset}"
            );
            assert_eq!(revisions::cleanup_at(&db.pool, cutoff).await?, 0);
            let retained: bool =
                sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM text_revisions WHERE id=$1)")
                    .bind(id)
                    .fetch_one(&db.pool)
                    .await?;
            assert_eq!(retained, offset < 0, "{days} days: offset {offset}");
            assert_eq!(revision_head(&db.pool, node.id).await?, before);
            assert_eq!(
                repo_body(&editing, space, node.id).await?,
                if checkpoint { "b" } else { "c" }
            );
            assert_history_usage(&db.pool, space).await?;
            if !checkpoint {
                let initial: i64 = sqlx::query_scalar(
                    "SELECT count(*) FROM text_revisions WHERE space_id=$1 AND checkpoint",
                )
                .bind(space)
                .fetch_one(&db.pool)
                .await?;
                assert_eq!(initial, 1);
            }
            sqlx::query("DELETE FROM spaces WHERE id=$1")
                .bind(space)
                .execute(&db.pool)
                .await?;
        }
    }
    db.cleanup().await;
    Ok(())
}

async fn repo_body(repo: &FilesRepo, space: Uuid, node: Uuid) -> Result<String, Error> {
    Ok(repo
        .find_text(space, node)
        .await?
        .unwrap()
        .1
        .content
        .unwrap())
}

#[tokio::test]
async fn unsuccessful_writes_do_not_refresh_the_editing_clock() -> TestResult {
    let Some(db) = TestDb::setup().await? else {
        return Ok(());
    };
    let (actor, space, root) = space_with_root(&db.pool, "revision-attempt-clock").await?;
    let repo = FilesRepo::new(db.pool.clone()).with_revision_time(policy_time());
    let (node, _) = repo
        .insert_text(space, root, "note.md", &body("a"), actor)
        .await?;
    let start = policy_time() + Duration::seconds(1);
    let editing = repo
        .with_revision_context("browser", Some(Uuid::new_v4()))
        .with_revision_time(start);
    save(&editing, space, node.id, actor, "b").await?;
    let head = revision_head(&db.pool, node.id).await?;
    let snapshots: Vec<Uuid> = sqlx::query_scalar("SELECT id FROM text_revisions ORDER BY id")
        .fetch_all(&db.pool)
        .await?;
    let attempt = editing
        .clone()
        .with_revision_time(start + Duration::seconds(119));
    sqlx::raw_sql("CREATE FUNCTION reject_revision_write() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN IF NEW.content_text = 'c' THEN RAISE EXCEPTION 'injected write failure'; END IF; RETURN NEW; END $$; CREATE TRIGGER reject_revision_write BEFORE UPDATE ON text_objects FOR EACH ROW EXECUTE FUNCTION reject_revision_write();").execute(&db.pool).await?;
    for kind in ["unchanged", "conflict", "rollback"] {
        match kind {
            "unchanged" => save(&attempt, space, node.id, actor, "b").await?,
            "conflict" => assert!(matches!(
                attempt
                    .save_text_content(
                        space,
                        node.id,
                        &body("c"),
                        Some(&body("a").content_sha256),
                        actor,
                        TextMutationKind::Write
                    )
                    .await,
                Err(Error::Conflict(_))
            )),
            _ => assert!(save(&attempt, space, node.id, actor, "c").await.is_err()),
        }
        assert_eq!(revision_head(&db.pool, node.id).await?, head, "{kind}");
        let actual: Vec<Uuid> = sqlx::query_scalar("SELECT id FROM text_revisions ORDER BY id")
            .fetch_all(&db.pool)
            .await?;
        assert_eq!(actual, snapshots, "{kind}");
        assert_eq!(repo_body(&editing, space, node.id).await?, "b");
        assert_history_usage(&db.pool, space).await?;
    }
    sqlx::query("DROP TRIGGER reject_revision_write ON text_objects")
        .execute(&db.pool)
        .await?;
    let now = start + Duration::seconds(120);
    save(&editing.with_revision_time(now), space, node.id, actor, "c").await?;
    let next = revision_head(&db.pool, node.id).await?;
    assert_ne!(next.2, head.2);
    assert_eq!((next.1, next.3), (now, now));
    let checkpoint: bool = sqlx::query_scalar("SELECT checkpoint FROM text_revisions WHERE id=$1")
        .bind(head.0)
        .fetch_one(&db.pool)
        .await?;
    assert!(checkpoint);
    db.cleanup().await;
    Ok(())
}

#[tokio::test]
async fn purpose_follows_the_resulting_body_and_ignores_failed_or_unchanged_writes() -> TestResult {
    let Some(db) = TestDb::setup().await? else {
        return Ok(());
    };
    let (actor, space, root) = space_with_root(&db.pool, "revision-purpose").await?;
    let repo = FilesRepo::new(db.pool.clone()).with_revision_context("mcp", None);
    let (node, _) = repo
        .clone()
        .with_revision_purpose(Some("Create the original note".into()))
        .insert_text(space, root, "note.md", &body("a"), actor)
        .await?;
    let editing = repo
        .clone()
        .with_revision_purpose(Some("Correct the configuration".into()));
    save(&editing, space, node.id, actor, "b").await?;
    let clear_count: i64 = sqlx::query_scalar(
        "SELECT (SELECT count(*) FROM text_objects WHERE revision_purpose IS NOT NULL) + \
                (SELECT count(*) FROM text_revisions WHERE purpose IS NOT NULL)",
    )
    .fetch_one(&db.pool)
    .await?;
    assert_eq!(clear_count, 0);
    let page = repo.list_text_revisions(space, node.id, 10, None).await?;
    assert_eq!(
        page.current.unwrap().purpose.as_deref(),
        Some("Correct the configuration")
    );
    assert_eq!(
        page.revisions[0].purpose.as_deref(),
        Some("Create the original note")
    );
    assert_eq!(
        repo.read_text_revision(space, node.id, page.revisions[0].id)
            .await?
            .revision
            .purpose,
        page.revisions[0].purpose
    );
    let attempt = repo
        .clone()
        .with_revision_purpose(Some("Must not replace the saved reason".into()));
    save(&attempt, space, node.id, actor, "b").await?;
    assert!(
        attempt
            .save_text_content(
                space,
                node.id,
                &body("c"),
                Some(&body("a").content_sha256),
                actor,
                TextMutationKind::Write
            )
            .await
            .is_err()
    );
    let page = repo.list_text_revisions(space, node.id, 10, None).await?;
    assert_eq!(page.revisions.len(), 1);
    assert_eq!(
        page.current.unwrap().purpose.as_deref(),
        Some("Correct the configuration")
    );
    let browser = attempt.with_revision_context("browser", None);
    save(&browser, space, node.id, actor, "c").await?;
    let page = browser
        .list_text_revisions(space, node.id, 10, None)
        .await?;
    assert!(page.current.unwrap().purpose.is_none());
    assert_eq!(
        page.revisions[0].purpose.as_deref(),
        Some("Correct the configuration")
    );
    db.cleanup().await;
    Ok(())
}

#[tokio::test]
async fn legacy_revision_reasons_migrate_and_survive_old_writer_archival() -> TestResult {
    let Some(db) = TestDb::setup().await? else {
        return Ok(());
    };
    let (actor, space, root) = space_with_root(&db.pool, "legacy-reason").await?;
    let crypto = PiiCrypto::test();
    let repo = FilesRepo::new(db.pool.clone());
    let (node, _) = repo
        .insert_text(space, root, "note.md", &body("a"), actor)
        .await?;
    save(&repo, space, node.id, actor, "b").await?;
    sqlx::query(
        "UPDATE text_objects SET revision_purpose='current legacy reason', revision_private_purpose=NULL WHERE node_id=$1",
    )
    .bind(node.id)
    .execute(&db.pool)
    .await?;
    sqlx::query("UPDATE text_revisions SET purpose='past legacy reason', private_purpose=NULL WHERE node_id=$1")
        .bind(node.id)
        .execute(&db.pool)
        .await?;
    let before = repo.list_text_revisions(space, node.id, 10, None).await?;
    assert_eq!(
        revisions::encrypt_legacy_purposes(&db.pool, &crypto).await?,
        2
    );
    assert_eq!(
        revisions::encrypt_legacy_purposes(&db.pool, &crypto).await?,
        0
    );
    let after = repo.list_text_revisions(space, node.id, 10, None).await?;
    assert_eq!(
        serde_json::to_value(&before.current)?,
        serde_json::to_value(&after.current)?
    );
    assert_eq!(
        serde_json::to_value(&before.revisions)?,
        serde_json::to_value(&after.revisions)?
    );

    let head = revision_head(&db.pool, node.id).await?;
    let envelope = crypto.encrypt_text_content(
        &space.to_string(),
        &format!("{}/revisions/{}", node.id, head.0),
        "b",
    )?;
    let bytes = (envelope.ciphertext.len() + envelope.nonce.len()) as i64;
    let mut tx = db.pool.begin().await?;
    sqlx::query("UPDATE text_revision_usage SET stored_bytes=stored_bytes+$2 WHERE space_id=$1")
        .bind(space)
        .bind(bytes)
        .execute(&mut *tx)
        .await?;
    // Exact old-writer shape: INSERT omits private_purpose and copies the now
    // empty plaintext column. The compatibility trigger preserves the envelope.
    sqlx::query("INSERT INTO text_revisions(id,node_id,space_id,content_sha256,byte_len,line_count,written_at,author_id,group_id,source,checkpoint,superseded_at,cleanup_at,ciphertext,nonce,enc_key_id,enc_version,purpose) \
        SELECT revision_id,node_id,space_id,content_sha256,byte_len,line_count,revision_written_at,revision_author_id,revision_group_id,revision_source,true,clock_timestamp(),clock_timestamp()+interval '30 days',$2,$3,$4,$5,revision_purpose FROM text_objects WHERE node_id=$1")
        .bind(node.id).bind(envelope.ciphertext).bind(envelope.nonce).bind(crypto.enc_key_id()).bind(crypto.version()).execute(&mut *tx).await?;
    sqlx::query("UPDATE text_objects SET revision_id=$2, revision_purpose='new legacy reason' WHERE node_id=$1")
        .bind(node.id).bind(Uuid::new_v4()).execute(&mut *tx).await?;
    tx.commit().await?;
    let archived = repo.read_text_revision(space, node.id, head.0).await?;
    assert_eq!(archived.content, "b");
    assert_eq!(
        archived.revision.purpose.as_deref(),
        Some("current legacy reason")
    );
    let current = repo
        .list_text_revisions(space, node.id, 10, None)
        .await?
        .current
        .unwrap();
    assert_eq!(current.purpose.as_deref(), Some("new legacy reason"));
    assert_eq!(
        revisions::encrypt_legacy_purposes(&db.pool, &crypto).await?,
        1
    );
    let private: serde_json::Value =
        sqlx::query_scalar("SELECT private_purpose FROM text_revisions WHERE id=$1")
            .bind(head.0)
            .fetch_one(&db.pool)
            .await?;
    assert!(!private.to_string().contains("legacy reason"));
    // A purpose copied to a different revision fails authentication.
    sqlx::query("UPDATE text_revisions SET private_purpose=$2 WHERE id=$1")
        .bind(after.revisions[0].id)
        .bind(private)
        .execute(&db.pool)
        .await?;
    assert!(
        repo.list_text_revisions(space, node.id, 10, None)
            .await
            .is_err()
    );
    db.cleanup().await;
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
    let repo = FilesRepo::new(db.pool.clone()).with_revision_time(policy_time());
    let (node, _) = repo
        .insert_text(space, root, "note.md", &body("a"), actor)
        .await?;
    let editing = repo
        .clone()
        .with_revision_context("browser", Some(Uuid::new_v4()));
    for (second, value) in [(1, "b"), (2, "c"), (3, "d")] {
        save(
            &editing
                .clone()
                .with_revision_time(policy_time() + Duration::seconds(second)),
            space,
            node.id,
            actor,
            value,
        )
        .await?;
    }
    let ai = repo
        .clone()
        .with_revision_context("mcp", Some(Uuid::new_v4()));
    save(
        &ai.with_revision_time(policy_time() + Duration::seconds(4)),
        space,
        node.id,
        actor,
        "e",
    )
    .await?;
    save(
        &repo
            .clone()
            .with_revision_time(policy_time() + Duration::seconds(5)),
        space,
        node.id,
        actor,
        "f",
    )
    .await?;
    let flags: Vec<(String, bool)> = sqlx::query_as(
        "SELECT content_sha256,checkpoint FROM text_revisions ORDER BY superseded_at",
    )
    .fetch_all(&db.pool)
    .await?;
    assert_eq!(
        flags.iter().map(|v| v.1).collect::<Vec<_>>(),
        vec![true, false, false, true, true]
    );
    assert_eq!(
        revisions::cleanup_at(&db.pool, policy_time() + Duration::seconds(5)).await?,
        0
    );
    let recent_cutoff = policy_time() + Duration::hours(25);
    assert_eq!(revisions::cleanup_at(&db.pool, recent_cutoff).await?, 2);
    assert_eq!(revisions::cleanup_at(&db.pool, recent_cutoff).await?, 0);
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
    assert_eq!(
        revisions::cleanup_at(&db.pool, policy_time() + Duration::days(31)).await?,
        3
    );
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
async fn actor_channel_and_session_boundaries_cannot_coalesce() -> TestResult {
    let Some(db) = TestDb::setup().await? else {
        return Ok(());
    };
    let (actor, space, root) = space_with_root(&db.pool, "revision-identity").await?;
    let other =
        common::insert_user_account(&db.pool, "revision-other", "other@example.com").await?;
    let session = Some(Uuid::new_v4());
    let repo = FilesRepo::new(db.pool.clone()).with_revision_time(policy_time());
    let (node, _) = repo
        .insert_text(space, root, "note.md", &body("a"), actor)
        .await?;
    for (second, source, id, author, value) in [
        (1, "browser", session, actor, "b"),
        (2, "browser", session, other, "c"),
        (3, "mcp", session, other, "d"),
        (4, "mcp", Some(Uuid::new_v4()), other, "e"),
        (5, "mcp", None, other, "f"),
        (6, "mcp", None, other, "g"),
    ] {
        save(
            &repo
                .clone()
                .with_revision_context(source, id)
                .with_revision_time(policy_time() + Duration::seconds(second)),
            space,
            node.id,
            author,
            value,
        )
        .await?;
    }
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
    let repo = FilesRepo::new(db.pool.clone()).with_revision_time(policy_time());
    let (node, _) = repo
        .insert_text(space, root, "note.md", &body("a"), actor)
        .await?;
    let editing = repo
        .clone()
        .with_revision_context("browser", Some(Uuid::new_v4()))
        .with_revision_time(policy_time() + Duration::seconds(1));
    save(&editing, space, node.id, actor, "b").await?;
    let head = revision_head(&db.pool, node.id).await?;
    sqlx::query("UPDATE text_revision_usage SET stored_bytes=$1 WHERE space_id=$2")
        .bind(revisions::SPACE_HISTORY_BYTES)
        .bind(space)
        .execute(&db.pool)
        .await?;
    assert!(matches!(
        save(
            &editing.with_revision_time(policy_time() + Duration::seconds(120)),
            space,
            node.id,
            actor,
            "c"
        )
        .await,
        Err(Error::TextRevisionStorageFull)
    ));
    assert_eq!(revision_head(&db.pool, node.id).await?, head);
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
    let repo = FilesRepo::new(db.pool.clone()).with_revision_time(policy_time());
    let (node, _) = repo
        .insert_text(space, root, "note.md", &body("a"), actor)
        .await?;
    let replacement = policy_time() + Duration::days(90);
    save(
        &repo.clone().with_revision_time(replacement),
        space,
        node.id,
        actor,
        "b",
    )
    .await?;
    assert_eq!(revisions::cleanup_at(&db.pool, replacement).await?, 0);
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
    let (actor, space, root) = legacy_space_with_root(&db.pool, "revision-migration").await?;
    let repo = FilesRepo::new(db.pool.clone());
    // Model the old binary's schema directly; the current writer requires the latest migration.
    let node_id: Uuid = sqlx::query_scalar("INSERT INTO nodes (space_id,parent_id,name,kind,created_by_account_id,updated_by_account_id) VALUES ($1,$2,'legacy.md','text',$3,$3) RETURNING id")
        .bind(space).bind(root).bind(actor).fetch_one(&db.pool).await?;
    sqlx::query("INSERT INTO text_objects (node_id,space_id,storage_format,content_text,content_sha256,byte_len,line_count,created_by_account_id,updated_by_account_id) VALUES ($1,$2,'plain','a',$3,1,1,$4,$4)")
        .bind(node_id).bind(space).bind(&body("a").content_sha256).bind(actor).execute(&db.pool).await?;
    sqlx::query("UPDATE text_objects SET updated_at=now()-interval '90 days' WHERE node_id=$1")
        .bind(node_id)
        .execute(&db.pool)
        .await?;
    db.apply_migration(42).await?;
    let backfilled: bool=sqlx::query_scalar("SELECT revision_author_id=updated_by_account_id AND revision_written_at=updated_at FROM text_objects WHERE node_id=$1").bind(node_id).fetch_one(&db.pool).await?;
    assert!(backfilled);
    db.apply_migration(43).await?;
    db.apply_migration(44).await?;
    db.apply_migration(45).await?;
    db.apply_migration(46).await?;
    db.apply_migration(47).await?;
    db.apply_migration(48).await?;
    db.apply_migration(49).await?;
    db.apply_migration(50).await?;
    db.apply_migration(51).await?;
    assert!(
        repo.list_text_revisions(space, node_id, 10, None)
            .await?
            .revisions
            .is_empty()
    );
    save(&repo, space, node_id, actor, "b").await?;
    assert_eq!(revisions::cleanup(&db.pool).await?, 0);
    let page = repo.list_text_revisions(space, node_id, 10, None).await?;
    assert!(page.current.unwrap().purpose.is_none());
    assert!(page.revisions[0].purpose.is_none());
    assert_eq!(
        repo.read_text_revision(space, node_id, page.revisions[0].id)
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
    let repo = FilesRepo::new(db.pool.clone()).with_revision_time(policy_time());
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
    let cutoff = policy_time() + Duration::days(31);
    assert_eq!(revisions::cleanup_at(&db.pool, cutoff).await?, 100);
    assert_eq!(revisions::cleanup_at(&db.pool, cutoff).await?, 5);
    assert_eq!(revisions::cleanup_at(&db.pool, cutoff).await?, 0);
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

#[tokio::test]
async fn changed_save_updates_body_and_revision_attribution_once() -> TestResult {
    let Some(db) = TestDb::setup().await? else {
        return Ok(());
    };
    let (actor, space, root) = space_with_root(&db.pool, "revision-single-update").await?;
    let repo = FilesRepo::new(db.pool.clone()).with_revision_time(policy_time());
    let (node, _) = repo
        .insert_text(space, root, "note.md", &body("a"), actor)
        .await?;
    let original = revision_head(&db.pool, node.id).await?;
    sqlx::raw_sql("CREATE TABLE text_update_observations (body_changed bool NOT NULL, revision_changed bool NOT NULL); CREATE FUNCTION observe_text_update() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN INSERT INTO text_update_observations VALUES (OLD.content_sha256 IS DISTINCT FROM NEW.content_sha256, OLD.revision_id IS DISTINCT FROM NEW.revision_id); RETURN NEW; END $$; CREATE TRIGGER observe_text_update AFTER UPDATE ON text_objects FOR EACH ROW EXECUTE FUNCTION observe_text_update();")
        .execute(&db.pool).await?;
    let now = policy_time() + Duration::seconds(1);
    save(
        &repo.clone().with_revision_time(now),
        space,
        node.id,
        actor,
        "b",
    )
    .await?;
    let observed: Vec<(bool, bool)> =
        sqlx::query_as("SELECT body_changed, revision_changed FROM text_update_observations")
            .fetch_all(&db.pool)
            .await?;
    assert_eq!(observed, vec![(true, true)]);
    let head = revision_head(&db.pool, node.id).await?;
    assert_eq!(head.1, now);
    assert_ne!(head.0, original.0);
    assert_eq!(
        repo.read_text_revision(space, node.id, original.0)
            .await?
            .content,
        "a"
    );
    save(&repo, space, node.id, actor, "b").await?;
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM text_update_observations")
        .fetch_one(&db.pool)
        .await?;
    assert_eq!(count, 1);
    assert_eq!(revision_head(&db.pool, node.id).await?, head);
    assert_history_usage(&db.pool, space).await?;
    db.cleanup().await;
    Ok(())
}

#[tokio::test]
async fn trash_restore_keeps_the_current_revision_but_does_not_freeze_revision_retention()
-> TestResult {
    let Some(db) = TestDb::setup().await? else {
        return Ok(());
    };
    for scope in ["document", "folder", "space"] {
        let (actor, space, root) =
            space_with_root(&db.pool, &format!("revision-trash-{scope}")).await?;
        let start = Utc::now() - Duration::seconds(3);
        let repo = FilesRepo::new(db.pool.clone()).with_revision_time(start);
        let parent = if scope == "folder" {
            repo.insert_folder(
                space,
                &CreateFolder {
                    parent_node_id: root,
                    name: "notes".into(),
                },
                actor,
            )
            .await?
            .id
        } else {
            root
        };
        let (node, _) = repo
            .insert_text(space, parent, "note.md", &body("old"), actor)
            .await?;
        let editing = repo
            .clone()
            .with_revision_context("browser", Some(Uuid::new_v4()));
        for (second, value) in [(1, "intermediate"), (2, "current")] {
            save(
                &editing
                    .clone()
                    .with_revision_time(start + Duration::seconds(second)),
                space,
                node.id,
                actor,
                value,
            )
            .await?;
        }
        let head = revision_head(&db.pool, node.id).await?;
        let target = match scope {
            "space" => {
                SpaceRepo::new(db.pool.clone())
                    .delete_space(space, actor, actor)
                    .await?;
                space
            }
            _ => {
                let target = if scope == "folder" { parent } else { node.id };
                repo.soft_delete_node(space, target, actor, scope == "folder")
                    .await?;
                target
            }
        };
        let selected = repo
            .list_trash(actor, 100, None)
            .await?
            .into_iter()
            .find(|item| item.id == target)
            .unwrap();
        let cleanup_at: DateTime<Utc> =
            sqlx::query_scalar("SELECT min(cleanup_at) FROM text_revisions WHERE node_id = $1")
                .bind(node.id)
                .fetch_one(&db.pool)
                .await?;
        assert!(cleanup_at > selected.deleted_at && cleanup_at < selected.purge_after);
        assert_eq!(
            revisions::cleanup_at(&db.pool, cleanup_at - Duration::microseconds(1)).await?,
            0
        );
        assert_eq!(
            revisions::cleanup_at(&db.pool, cleanup_at).await?,
            1,
            "{scope}"
        );
        assert_eq!(revisions::cleanup_at(&db.pool, cleanup_at).await?, 0);
        assert_eq!(revision_head(&db.pool, node.id).await?, head);
        assert_history_usage(&db.pool, space).await?;
        // Service reads authorize the Space first; find_text itself filters
        // node deletion, since a trashed Space preserves its node flags.
        let accessible = repo.permission_for(space, actor).await?.is_some()
            && repo.find_text(space, node.id).await?.is_some();
        assert!(!accessible, "retention must not reactivate {scope} trash");
        assert!(matches!(
            save(&repo, space, node.id, actor, "blocked").await,
            Err(Error::NotFound(_))
        ));
        let repo = repo.with_trash_time(cleanup_at);
        if scope == "space" {
            repo.restore_trashed_space(actor, space, (&selected).into())
                .await?;
        } else {
            repo.restore_trashed_node(actor, space, target, (&selected).into())
                .await?;
        }
        assert_eq!(revision_head(&db.pool, node.id).await?, head);
        let history = repo.list_text_revisions(space, node.id, 10, None).await?;
        assert_eq!(history.revisions.len(), 1);
        assert_eq!(
            repo.read_text_revision(space, node.id, history.revisions[0].id)
                .await?
                .content,
            "old"
        );
        assert_eq!(
            repo.find_text(space, node.id)
                .await?
                .unwrap()
                .1
                .content
                .as_deref(),
            Some("current")
        );
        let receipts: Vec<String> = sqlx::query_scalar(
            "SELECT metadata->>'reason' FROM audit_events WHERE op_type = 'text_revision.delete' AND metadata->>'node_id' = $1",
        ).bind(node.id.to_string()).fetch_all(&db.pool).await?;
        assert_eq!(receipts, ["intermediate_expired"]);
        assert_history_usage(&db.pool, space).await?;
    }
    db.cleanup().await;
    Ok(())
}
