//! Bounded relation reads assembled from one repeatable-read snapshot.

use std::collections::{BTreeSet, HashMap};

use notegate_core::{Error, Result};
use notegate_model::{LinkReferenceKind, SpaceLinkIndexState};
use sqlx::{FromRow, Postgres};
use uuid::Uuid;

use super::{LinkIndexRepo, LinkIndexStateRow};
use crate::files::queries::node::node_paths_many_with;
use crate::map_sqlx_error;

#[derive(Debug, Clone)]
pub struct LinkReferenceRecord {
    pub id: i64,
    pub kind: LinkReferenceKind,
    pub raw_href: String,
    pub normalized_target_path: Option<String>,
    pub occurrence_count: i32,
    pub source_node_id: Uuid,
    pub source_name: String,
    pub source_path: Option<String>,
    pub target_node_id: Option<Uuid>,
    pub target_name: Option<String>,
    pub target_path: Option<String>,
    pub target_deleted: bool,
}

#[derive(Debug)]
pub struct NodeLinkRecords {
    pub index: SpaceLinkIndexState,
    pub outgoing_count: i64,
    pub incoming_count: i64,
    pub broken_count: i64,
    pub outgoing: Vec<LinkReferenceRecord>,
    pub incoming: Vec<LinkReferenceRecord>,
    pub outgoing_truncated: bool,
    pub incoming_truncated: bool,
}

impl LinkIndexRepo {
    pub async fn node_links(
        &self,
        space_id: Uuid,
        node_id: Uuid,
        limit: i64,
    ) -> Result<Option<NodeLinkRecords>> {
        let mut tx = self.pool.begin().await.map_err(map_sqlx_error)?;
        sqlx::query("SET TRANSACTION ISOLATION LEVEL REPEATABLE READ READ ONLY")
            .execute(&mut *tx)
            .await
            .map_err(map_sqlx_error)?;
        let Some(state) = sqlx::query_as::<_, LinkIndexStateRow>(
            "SELECT state.space_id, state.desired_generation, state.applied_generation, \
                    state.status, state.last_indexed_at \
             FROM space_link_index_states state \
             JOIN nodes node ON node.space_id = state.space_id \
                            AND node.id = $2 \
                            AND node.deleted_at IS NULL \
             WHERE state.space_id = $1",
        )
        .bind(space_id)
        .bind(node_id)
        .fetch_optional(&mut *tx)
        .await
        .map_err(map_sqlx_error)?
        else {
            tx.commit().await.map_err(map_sqlx_error)?;
            return Ok(None);
        };

        let (outgoing_count, incoming_count, broken_count) =
            relation_counts(&mut tx, space_id, node_id).await?;
        let outgoing_rows = outgoing_rows(&mut tx, space_id, node_id, limit + 1).await?;
        let incoming_rows = incoming_rows(&mut tx, space_id, node_id, limit + 1).await?;
        let (mut outgoing, outgoing_truncated) = truncate_records(outgoing_rows, limit)?;
        let (mut incoming, incoming_truncated) = truncate_records(incoming_rows, limit)?;
        let node_ids = outgoing
            .iter()
            .chain(&incoming)
            .flat_map(|record| [Some(record.source_node_id), record.target_node_id])
            .flatten()
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect::<Vec<_>>();
        let paths = node_paths_many_with(&mut *tx, space_id, &node_ids).await?;
        attach_paths(&mut outgoing, &paths);
        attach_paths(&mut incoming, &paths);
        tx.commit().await.map_err(map_sqlx_error)?;

        Ok(Some(NodeLinkRecords {
            index: state.into_public()?,
            outgoing_count,
            incoming_count,
            broken_count,
            outgoing,
            incoming,
            outgoing_truncated,
            incoming_truncated,
        }))
    }
}

async fn relation_counts(
    tx: &mut sqlx::Transaction<'_, Postgres>,
    space_id: Uuid,
    node_id: Uuid,
) -> Result<(i64, i64, i64)> {
    sqlx::query_as(
        "SELECT \
            (SELECT count(*) FROM node_link_refs WHERE space_id = $1 AND source_node_id = $2), \
            (SELECT count(*) FROM node_link_refs reference \
             JOIN nodes source ON source.id = reference.source_node_id \
             WHERE reference.space_id = $1 AND reference.target_node_id = $2 \
               AND source.deleted_at IS NULL), \
            (SELECT count(*) FROM node_link_refs reference \
             LEFT JOIN nodes target ON target.id = reference.target_node_id \
             WHERE reference.space_id = $1 AND reference.source_node_id = $2 \
               AND (reference.normalized_target_path IS NULL \
                    OR reference.target_node_id IS NULL \
                    OR target.deleted_at IS NOT NULL))",
    )
    .bind(space_id)
    .bind(node_id)
    .fetch_one(&mut **tx)
    .await
    .map_err(map_sqlx_error)
}

async fn outgoing_rows(
    tx: &mut sqlx::Transaction<'_, Postgres>,
    space_id: Uuid,
    node_id: Uuid,
    limit: i64,
) -> Result<Vec<LinkReferenceRow>> {
    sqlx::query_as::<_, LinkReferenceRow>(sqlx::AssertSqlSafe(format!(
        "SELECT {LINK_REFERENCE_COLUMNS} \
         FROM node_link_refs reference \
         JOIN nodes source ON source.id = reference.source_node_id \
         LEFT JOIN nodes target ON target.id = reference.target_node_id \
         WHERE reference.space_id = $1 AND reference.source_node_id = $2 \
           AND source.deleted_at IS NULL \
         ORDER BY reference.id LIMIT $3"
    )))
    .bind(space_id)
    .bind(node_id)
    .bind(limit)
    .fetch_all(&mut **tx)
    .await
    .map_err(map_sqlx_error)
}

async fn incoming_rows(
    tx: &mut sqlx::Transaction<'_, Postgres>,
    space_id: Uuid,
    node_id: Uuid,
    limit: i64,
) -> Result<Vec<LinkReferenceRow>> {
    sqlx::query_as::<_, LinkReferenceRow>(sqlx::AssertSqlSafe(format!(
        "SELECT {LINK_REFERENCE_COLUMNS} \
         FROM node_link_refs reference \
         JOIN nodes source ON source.id = reference.source_node_id \
         LEFT JOIN nodes target ON target.id = reference.target_node_id \
         WHERE reference.space_id = $1 AND reference.target_node_id = $2 \
           AND source.deleted_at IS NULL \
         ORDER BY reference.id LIMIT $3"
    )))
    .bind(space_id)
    .bind(node_id)
    .bind(limit)
    .fetch_all(&mut **tx)
    .await
    .map_err(map_sqlx_error)
}

fn truncate_records(
    mut rows: Vec<LinkReferenceRow>,
    limit: i64,
) -> Result<(Vec<LinkReferenceRecord>, bool)> {
    let limit =
        usize::try_from(limit).map_err(|_| Error::internal("invalid link relation limit"))?;
    let truncated = rows.len() > limit;
    if truncated {
        rows.pop();
    }
    let records = rows
        .into_iter()
        .map(LinkReferenceRow::into_record)
        .collect::<Result<Vec<_>>>()?;
    Ok((records, truncated))
}

fn attach_paths(records: &mut [LinkReferenceRecord], paths: &HashMap<Uuid, String>) {
    for record in records {
        record.source_path = paths.get(&record.source_node_id).cloned();
        record.target_path = record
            .target_node_id
            .and_then(|target_node_id| paths.get(&target_node_id).cloned());
    }
}

#[derive(Debug, FromRow)]
struct LinkReferenceRow {
    id: i64,
    reference_kind: String,
    raw_href: String,
    normalized_target_path: Option<String>,
    occurrence_count: i32,
    source_node_id: Uuid,
    source_name: String,
    target_node_id: Option<Uuid>,
    target_name: Option<String>,
    target_deleted: bool,
}

impl LinkReferenceRow {
    fn into_record(self) -> Result<LinkReferenceRecord> {
        let kind = LinkReferenceKind::parse(&self.reference_kind).ok_or_else(|| {
            Error::internal(format!(
                "unknown link reference kind: {}",
                self.reference_kind
            ))
        })?;
        Ok(LinkReferenceRecord {
            id: self.id,
            kind,
            raw_href: self.raw_href,
            normalized_target_path: self.normalized_target_path,
            occurrence_count: self.occurrence_count,
            source_node_id: self.source_node_id,
            source_name: self.source_name,
            source_path: None,
            target_node_id: self.target_node_id,
            target_name: self.target_name,
            target_path: None,
            target_deleted: self.target_deleted,
        })
    }
}

const LINK_REFERENCE_COLUMNS: &str = "reference.id, reference.reference_kind, reference.raw_href, \
     reference.normalized_target_path, reference.occurrence_count, \
     reference.source_node_id, source.name AS source_name, \
     reference.target_node_id, target.name AS target_name, \
     COALESCE(target.deleted_at IS NOT NULL, false) AS target_deleted";
