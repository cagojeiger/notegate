//! Atomic snapshots and metadata-only retention. No history operation mutates current content.
use chrono::{DateTime, Utc};
use notegate_core::security::{EncryptedField, PiiCrypto};
use notegate_core::{Error, Result};
use notegate_model::text_revision::{
    CurrentTextRevision, TextRevision, TextRevisionContent, TextRevisionCursor, TextRevisionPage,
};
use sqlx::{PgConnection, PgPool, Row, postgres::PgRow};
use uuid::Uuid;

use super::{commands::checks, rows::TextRow};
use crate::map_sqlx_error;

const DAY_SECONDS: i32 = 24 * 60 * 60;
pub const RECENT_SECONDS: i32 = DAY_SECONDS;
pub const RETENTION_SECONDS: i32 = 30 * DAY_SECONDS;
pub const IDLE_SECONDS: i32 = 120;
pub const GROUP_SECONDS: i32 = 600;
pub const SPACE_HISTORY_BYTES: i64 = 1024 * 1024 * 1024;
const CLEANUP_BATCH: i64 = 100;
const META: &str = "r.id, r.node_id, r.content_sha256, r.byte_len, r.line_count, r.written_at, r.author_id, r.group_id, r.source, r.purpose, r.superseded_at";
const VISIBLE: &str = "r.space_id = $1 AND r.node_id = $2 AND EXISTS (SELECT 1 FROM text_objects t JOIN nodes n ON n.id = t.node_id AND n.space_id = t.space_id JOIN spaces s ON s.id = t.space_id WHERE t.node_id = r.node_id AND t.space_id = r.space_id AND t.storage_format = 'plain' AND n.deleted_at IS NULL AND s.deleted_at IS NULL)";

/// Called only while the normal write transaction owns the Space and text locks.
#[allow(clippy::too_many_arguments)]
pub(crate) async fn capture(
    tx: &mut PgConnection,
    crypto: &PiiCrypto,
    current: &TextRow,
    actor: Uuid,
    source: &str,
    session: Option<Uuid>,
    purpose: Option<&str>,
    next_plain: bool,
    now: Option<DateTime<Utc>>,
) -> Result<()> {
    // Sample wall time once, after the write transaction acquired its locks.
    let head = sqlx::query(
        "WITH clock AS MATERIALIZED (SELECT COALESCE($6::timestamptz, clock_timestamp()) AS saved_at) \
         SELECT revision_id, revision_written_at, revision_author_id, revision_group_id, \
         revision_group_started_at, revision_source, revision_purpose, clock.saved_at, \
         ($3::uuid IS NOT NULL AND revision_session_id = $3 AND revision_author_id = $4 \
          AND revision_source = $5 AND revision_written_at > clock.saved_at - make_interval(secs => $7) \
          AND revision_group_started_at > clock.saved_at - make_interval(secs => $8)) AS same_group \
         FROM text_objects CROSS JOIN clock WHERE space_id = $1 AND node_id = $2",
    ).bind(current.space_id).bind(current.node_id).bind(session).bind(actor).bind(source)
        .bind(now).bind(IDLE_SECONDS).bind(GROUP_SECONDS).fetch_one(&mut *tx).await.map_err(map_sqlx_error)?;
    let id: Uuid = head.try_get("revision_id").map_err(map_sqlx_error)?;
    let saved_at: DateTime<Utc> = head.try_get("saved_at").map_err(map_sqlx_error)?;
    let same_group = next_plain
        && current.storage_format == "plain"
        && head
            .try_get::<Option<bool>, _>("same_group")
            .map_err(map_sqlx_error)?
            .unwrap_or(false);
    let previous_group: Uuid = head.try_get("revision_group_id").map_err(map_sqlx_error)?;
    let started_at: DateTime<Utc> = head
        .try_get("revision_group_started_at")
        .map_err(map_sqlx_error)?;

    // A plain -> opaque transition still preserves the last supported plain body.
    // No client-encrypted payload is interpreted or included in revision APIs.
    if current.storage_format == "plain" {
        let text = current.clone().into_text(crypto)?;
        let plaintext = text
            .content
            .ok_or_else(|| Error::internal("plain text has no body"))?;
        let encrypted = crypto.encrypt_text_content(
            &current.space_id.to_string(),
            &revision_binding(current.node_id, id),
            &plaintext,
        )?;
        let stored_bytes = i64::try_from(encrypted.ciphertext.len() + encrypted.nonce.len())
            .map_err(|_| Error::internal("revision size overflow"))?;
        sqlx::query(
            "INSERT INTO text_revision_usage (space_id) VALUES ($1) ON CONFLICT DO NOTHING",
        )
        .bind(current.space_id)
        .execute(&mut *tx)
        .await
        .map_err(map_sqlx_error)?;
        let reserved = sqlx::query(
            "UPDATE text_revision_usage SET stored_bytes = stored_bytes + $2 \
             WHERE space_id = $1 AND stored_bytes + $2 <= $3",
        )
        .bind(current.space_id)
        .bind(stored_bytes)
        .bind(SPACE_HISTORY_BYTES)
        .execute(&mut *tx)
        .await
        .map_err(map_sqlx_error)?;
        if reserved.rows_affected() == 0 {
            return Err(Error::TextRevisionStorageFull);
        }
        sqlx::query(
            "INSERT INTO text_revisions (id, node_id, space_id, content_sha256, byte_len, line_count, \
             written_at, author_id, group_id, source, checkpoint, superseded_at, cleanup_at, \
             ciphertext, nonce, enc_key_id, enc_version, purpose) \
             VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$12 + make_interval(secs => $13),$14,$15,$16,$17,$18)",
        ).bind(id).bind(current.node_id).bind(current.space_id).bind(&current.content_sha256)
            .bind(current.byte_len).bind(current.line_count)
            .bind(head.try_get::<DateTime<Utc>, _>("revision_written_at").map_err(map_sqlx_error)?)
            .bind(head.try_get::<Uuid, _>("revision_author_id").map_err(map_sqlx_error)?)
            .bind(previous_group).bind(head.try_get::<String, _>("revision_source").map_err(map_sqlx_error)?)
            .bind(!same_group).bind(saved_at).bind(if same_group { RECENT_SECONDS } else { RETENTION_SECONDS })
            .bind(encrypted.ciphertext).bind(encrypted.nonce).bind(crypto.enc_key_id()).bind(crypto.version())
            .bind(head.try_get::<Option<String>, _>("revision_purpose").map_err(map_sqlx_error)?)
            .execute(&mut *tx).await.map_err(map_sqlx_error)?;
    }
    sqlx::query(
        "UPDATE text_objects SET revision_id = $3, revision_written_at = $4, revision_author_id = $5, \
         revision_group_id = $6, revision_group_started_at = $7, revision_session_id = $8, revision_source = $9, revision_purpose = $10 \
         WHERE space_id = $1 AND node_id = $2",
    ).bind(current.space_id).bind(current.node_id).bind(Uuid::new_v4()).bind(saved_at).bind(actor)
        .bind(if same_group { previous_group } else { Uuid::new_v4() })
        .bind(if same_group { started_at } else { saved_at }).bind(session).bind(source).bind(purpose)
        .execute(&mut *tx).await.map_err(map_sqlx_error)?;
    Ok(())
}

fn revision_binding(node: Uuid, revision: Uuid) -> String {
    format!("{node}/revisions/{revision}")
}

fn metadata(row: &PgRow) -> Result<TextRevision> {
    Ok(TextRevision {
        id: row.try_get("id").map_err(map_sqlx_error)?,
        node_id: row.try_get("node_id").map_err(map_sqlx_error)?,
        content_sha256: row.try_get("content_sha256").map_err(map_sqlx_error)?,
        byte_len: row.try_get("byte_len").map_err(map_sqlx_error)?,
        line_count: row.try_get("line_count").map_err(map_sqlx_error)?,
        written_at: row.try_get("written_at").map_err(map_sqlx_error)?,
        author_id: row.try_get("author_id").map_err(map_sqlx_error)?,
        group_id: row.try_get("group_id").map_err(map_sqlx_error)?,
        source: row.try_get("source").map_err(map_sqlx_error)?,
        purpose: row.try_get("purpose").map_err(map_sqlx_error)?,
        superseded_at: row.try_get("superseded_at").map_err(map_sqlx_error)?,
    })
}

pub async fn list(
    pool: &PgPool,
    space: Uuid,
    node: Uuid,
    limit: i64,
    cursor: Option<&TextRevisionCursor>,
    external_only: bool,
) -> Result<TextRevisionPage> {
    if !(1..=100).contains(&limit) {
        return Err(Error::validation(
            "revision limit must be between 1 and 100",
        ));
    }
    let rows = sqlx::query(sqlx::AssertSqlSafe(format!(
        "SELECT {META} FROM text_revisions r WHERE {VISIBLE} \
         AND (NOT $3 OR node_external_access_allowed($1,$2)) \
         AND ($4::timestamptz IS NULL OR (r.superseded_at, r.id) < ($4,$5)) \
         ORDER BY r.superseded_at DESC, r.id DESC LIMIT $6"
    )))
    .bind(space)
    .bind(node)
    .bind(external_only)
    .bind(cursor.map(|c| c.superseded_at))
    .bind(cursor.map(|c| c.id))
    .bind(limit + 1)
    .fetch_all(pool)
    .await
    .map_err(map_sqlx_error)?;
    let mut revisions = rows.iter().map(metadata).collect::<Result<Vec<_>>>()?;
    let has_more = revisions.len() > limit as usize;
    revisions.truncate(limit as usize);
    let next_cursor = if has_more {
        revisions.last().map(|r| TextRevisionCursor {
            superseded_at: r.superseded_at,
            id: r.id,
        })
    } else {
        None
    };
    let head = sqlx::query(
        "SELECT t.content_sha256, t.revision_purpose FROM text_objects t \
        JOIN nodes n ON n.id=t.node_id AND n.space_id=t.space_id JOIN spaces s ON s.id=t.space_id \
        WHERE t.space_id=$1 AND t.node_id=$2 AND t.storage_format='plain' \
        AND n.deleted_at IS NULL AND s.deleted_at IS NULL \
        AND (NOT $3 OR node_external_access_allowed($1,$2))",
    )
    .bind(space)
    .bind(node)
    .bind(external_only)
    .fetch_optional(pool)
    .await
    .map_err(map_sqlx_error)?;
    let current = head
        .map(|row| -> Result<CurrentTextRevision> {
            Ok(CurrentTextRevision {
                content_sha256: row.try_get("content_sha256").map_err(map_sqlx_error)?,
                purpose: row.try_get("revision_purpose").map_err(map_sqlx_error)?,
            })
        })
        .transpose()?;
    Ok(TextRevisionPage {
        current,
        revisions,
        next_cursor,
    })
}

pub async fn read(
    pool: &PgPool,
    crypto: &PiiCrypto,
    space: Uuid,
    node: Uuid,
    id: Uuid,
    external_only: bool,
) -> Result<TextRevisionContent> {
    let row = sqlx::query(sqlx::AssertSqlSafe(format!(
        "SELECT {META}, r.ciphertext, r.nonce, r.enc_key_id, r.enc_version \
         FROM text_revisions r WHERE {VISIBLE} AND r.id = $3 \
         AND (NOT $4 OR node_external_access_allowed($1,$2))"
    )))
    .bind(space)
    .bind(node)
    .bind(id)
    .bind(external_only)
    .fetch_optional(pool)
    .await
    .map_err(map_sqlx_error)?
    .ok_or_else(|| Error::not_found("text revision not found"))?;
    let content = crypto.decrypt_text_content(
        &space.to_string(),
        &revision_binding(node, id),
        &row.try_get::<String, _>("enc_key_id")
            .map_err(map_sqlx_error)?,
        row.try_get("enc_version").map_err(map_sqlx_error)?,
        &EncryptedField {
            ciphertext: row.try_get("ciphertext").map_err(map_sqlx_error)?,
            nonce: row.try_get("nonce").map_err(map_sqlx_error)?,
        },
    )?;
    Ok(TextRevisionContent {
        revision: metadata(&row)?,
        content,
    })
}

/// One Space, at most 100 rows, one transaction. Uses the normal mutation lock order.
/// Space deletion owns its cascade; skip deleted Spaces rather than obstructing purge.
pub async fn cleanup(pool: &PgPool) -> Result<u64> {
    cleanup_with_time(pool, None).await
}

/// Exercise the same selection and deletion queries against an exact policy cutoff.
#[cfg(any(test, feature = "test-util"))]
pub async fn cleanup_at(pool: &PgPool, now: DateTime<Utc>) -> Result<u64> {
    cleanup_with_time(pool, Some(now)).await
}

async fn cleanup_with_time(pool: &PgPool, now: Option<DateTime<Utc>>) -> Result<u64> {
    // Keep selection and deletion on one cutoff, even if lock acquisition takes time.
    let (cutoff, space): (DateTime<Utc>, Option<Uuid>) = sqlx::query_as(
        "WITH clock AS MATERIALIZED (SELECT COALESCE($1::timestamptz, clock_timestamp()) AS cutoff) \
         SELECT clock.cutoff, (SELECT r.space_id FROM text_revisions r JOIN spaces s ON s.id = r.space_id \
         WHERE r.cleanup_at <= clock.cutoff AND s.deleted_at IS NULL ORDER BY r.cleanup_at, r.id LIMIT 1) \
         FROM clock",
    )
    .bind(now)
    .fetch_one(pool)
    .await
    .map_err(map_sqlx_error)?;
    let Some(space) = space else { return Ok(0) };
    let mut tx = pool.begin().await.map_err(map_sqlx_error)?;
    sqlx::query("SET LOCAL lock_timeout = '2s'")
        .execute(&mut *tx)
        .await
        .map_err(map_sqlx_error)?;
    checks::lock_space(&mut tx, space).await?;
    let deleted = sqlx::query(
        "DELETE FROM text_revisions WHERE id IN (SELECT id FROM text_revisions \
         WHERE space_id = $1 AND cleanup_at <= $3 ORDER BY cleanup_at, id LIMIT $2)",
    )
    .bind(space)
    .bind(CLEANUP_BATCH)
    .bind(cutoff)
    .execute(&mut *tx)
    .await
    .map_err(map_sqlx_error)?
    .rows_affected();
    tx.commit().await.map_err(map_sqlx_error)?;
    Ok(deleted)
}
