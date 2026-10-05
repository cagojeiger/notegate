//! CI-only paired benchmark: service + PostgreSQL, not HTTP or production capacity.
#![allow(
    clippy::unwrap_in_result,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing
)]
mod common;
use common::{TestDb, insert_user_account, setup_space};
use futures_util::future::join_all;
use notegate_db::{FilesRepo, SpaceRepo};
use notegate_service::files::{FilesService, WriteTarget, WriteText, WriteTextBody};
use std::{sync::Arc, time::Instant};
use tokio::sync::Barrier;
use uuid::Uuid;

type TestResult = Result<(), Box<dyn std::error::Error>>;
const SAMPLES: usize = 20;

#[derive(Clone, Copy)]
struct Writer {
    actor: Uuid,
    space: Uuid,
    node: Uuid,
}

async fn write(
    files: &FilesService,
    writer: Writer,
    value: &str,
) -> Result<(), notegate_service::ServiceError> {
    files
        .write_text(
            writer.actor,
            writer.space,
            WriteText {
                target: WriteTarget::Existing {
                    node_id: writer.node,
                },
                body: WriteTextBody::Plain(value.to_owned()),
                expected_sha256: None,
            },
        )
        .await?;
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "CI performance workflow runs the same harness against baseline and candidate"]
async fn compare_text_writes() -> TestResult {
    let db = TestDb::setup()
        .await?
        .ok_or("NOTEGATE_TEST_DATABASE_URL is required")?;
    let files = FilesService::new(FilesRepo::new(db.pool.clone()));
    let spaces = SpaceRepo::new(db.pool.clone());
    let mut case = 0;
    for bytes in [10 * 1024, 100 * 1024, 1024 * 1024] {
        for encrypted in [false, true] {
            for shape in [
                "sequential",
                "same_space",
                "same_owner_spaces",
                "different_owners",
            ] {
                case += 1;
                let workers = if shape == "sequential" { 1 } else { 4 };
                let owner = insert_user_account(
                    &db.pool,
                    &format!("bench-{case}-owner"),
                    &format!("bench-{case}@example.com"),
                )
                .await?;
                sqlx::query("UPDATE users SET tier='system_max' WHERE id=$1")
                    .bind(owner)
                    .execute(&db.pool)
                    .await?;
                let mut writers = Vec::new();
                let mut shared = None;
                let a = Arc::new("a".repeat(bytes));
                let b = Arc::new("b".repeat(bytes));
                for worker in 0..workers {
                    let actor = if shape == "different_owners" && worker > 0 {
                        let id = insert_user_account(
                            &db.pool,
                            &format!("bench-{case}-{worker}"),
                            &format!("bench-{case}-{worker}@example.com"),
                        )
                        .await?;
                        sqlx::query("UPDATE users SET tier='system_max' WHERE id=$1")
                            .bind(id)
                            .execute(&db.pool)
                            .await?;
                        id
                    } else {
                        owner
                    };
                    let (space, root) = if shape == "same_space" && worker > 0 {
                        shared.unwrap()
                    } else {
                        let pair =
                            setup_space(&spaces, actor, &format!("bench-{case}-{worker}")).await;
                        sqlx::query(
                            "UPDATE spaces SET default_text_encryption_enabled=$2 WHERE id=$1",
                        )
                        .bind(pair.0)
                        .bind(encrypted)
                        .execute(&db.pool)
                        .await?;
                        shared = Some(pair);
                        pair
                    };
                    let created = files
                        .write_text(
                            actor,
                            space,
                            WriteText {
                                target: WriteTarget::Create {
                                    parent_node_id: root,
                                    name: format!("{worker}.md"),
                                },
                                body: WriteTextBody::Plain(a.as_ref().clone()),
                                expected_sha256: None,
                            },
                        )
                        .await?;
                    let writer = Writer {
                        actor,
                        space,
                        node: created.node.node.id,
                    };
                    // Exercise connections, statements and TOAST before timing changed saves.
                    write(&files, writer, &b).await?;
                    write(&files, writer, &a).await?;
                    writers.push(writer);
                }
                // Start both revisions at the same checkpoint phase. Earlier cases
                // generate enough WAL to otherwise trigger background checkpoints
                // during unrelated samples. Durability remains enabled. CI only.
                sqlx::query("CHECKPOINT").execute(&db.pool).await?;
                let barrier = Arc::new(Barrier::new(workers));
                let started = Instant::now();
                let results = join_all(writers.iter().map(|writer| {
                    let barrier = barrier.clone();
                    let files = files.clone();
                    let a = a.clone();
                    let b = b.clone();
                    let writer = *writer;
                    tokio::spawn(async move {
                        barrier.wait().await;
                        let mut times = Vec::new();
                        for index in 0..SAMPLES {
                            let started = Instant::now();
                            write(&files, writer, if index % 2 == 0 { &b } else { &a }).await?;
                            times.push(started.elapsed().as_secs_f64() * 1000.0);
                        }
                        Ok::<_, notegate_service::ServiceError>(times)
                    })
                }))
                .await;
                let elapsed = started.elapsed().as_secs_f64();
                let mut times = Vec::new();
                for result in results {
                    times.extend(result??);
                }
                times.sort_by(f64::total_cmp);
                let n = times.len();
                // Verify that all timed operations were actual changed saves with recoverable bodies.
                for writer in &writers {
                    let count: i64 =
                        sqlx::query_scalar("SELECT count(*) FROM text_revisions WHERE node_id=$1")
                            .bind(writer.node)
                            .fetch_one(&db.pool)
                            .await?;
                    assert_eq!(count, (SAMPLES + 2) as i64);
                    let current = files
                        .text_revisions(writer.actor, writer.space, writer.node, 1, None)
                        .await?;
                    let previous = files
                        .text_revision(
                            writer.actor,
                            writer.space,
                            writer.node,
                            current.revisions[0].id,
                        )
                        .await?;
                    assert_eq!(previous.content, b.as_str());
                    let usage: (i64, i64) = sqlx::query_as("SELECT stored_bytes,(SELECT SUM(stored_bytes)::bigint FROM text_revisions WHERE space_id=$1) FROM text_revision_usage WHERE space_id=$1")
                        .bind(writer.space).fetch_one(&db.pool).await?;
                    assert_eq!(usage.0, usage.1);
                }
                println!(
                    "NOTE_TEXT_WRITE_BENCH={}",
                    serde_json::json!({
                        "bytes":bytes, "encrypted":encrypted, "shape":shape, "workers":workers,
                        "samples":n, "p50_ms":times[(n - 1) / 2], "p95_ms":times[(n * 95).div_ceil(100) - 1],
                        "writes_per_second":n as f64 / elapsed,
                    })
                );
                let mut ids = writers.iter().map(|w| w.space).collect::<Vec<_>>();
                ids.sort_unstable();
                ids.dedup();
                sqlx::query("DELETE FROM spaces WHERE id = ANY($1)")
                    .bind(ids)
                    .execute(&db.pool)
                    .await?;
            }
        }
    }
    db.cleanup().await;
    Ok(())
}
