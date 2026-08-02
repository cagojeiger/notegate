//! Asynchronous Markdown link projection and browser-facing relation reads.

mod markdown;

use std::collections::{BTreeSet, HashMap};
use std::time::Duration;

use notegate_db::{
    FilesRepo, LinkIndexClaim, LinkIndexRepo, LinkReferenceRecord, NewLinkReference,
    NodeLinkRecords, QueuedLinkIndexEvent, SourceLinkSet,
};
use notegate_model::{
    LinkIndexFreshness, LinkIndexStatus, LinkReference, LinkReferenceStatus, NodeLinkSummary,
    Permission, SpaceLinkIndexState,
};
use uuid::Uuid;

use crate::error::{ServiceError, ServiceResult};

const PARSER_VERSION: i32 = 1;
const CLAIM_LEASE: Duration = Duration::from_secs(120);
const EVENT_BATCH_SIZE: i64 = 200;
const REBUILD_SOURCE_BATCH_SIZE: i64 = 64;
pub const NODE_LINK_PREVIEW_LIMIT: i64 = 8;

#[derive(Debug, Clone)]
pub struct LinkIndexService {
    index: LinkIndexRepo,
    files: FilesRepo,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LinkIndexRun {
    Idle,
    Incremental { space_id: Uuid, events: usize },
    Rebuilt { space_id: Uuid },
    RebuildQueued { space_id: Uuid },
}

impl LinkIndexService {
    pub fn new(index: LinkIndexRepo, files: FilesRepo) -> Self {
        Self { index, files }
    }

    pub async fn state(
        &self,
        caller_account_id: Uuid,
        space_id: Uuid,
    ) -> ServiceResult<SpaceLinkIndexState> {
        self.authorize(caller_account_id, space_id, false).await?;
        self.index
            .state(space_id)
            .await?
            .ok_or_else(|| ServiceError::Internal("link index state is missing".to_owned()))
    }

    pub async fn ensure_worker_compatible(&self) -> ServiceResult<()> {
        let newest_version = self.index.newest_parser_version().await?.unwrap_or(0);
        if newest_version > PARSER_VERSION {
            return Err(ServiceError::Internal(format!(
                "link index parser version {newest_version} is newer than this binary ({PARSER_VERSION}); roll forward"
            )));
        }
        Ok(())
    }

    pub async fn request_rebuild(
        &self,
        caller_account_id: Uuid,
        space_id: Uuid,
    ) -> ServiceResult<SpaceLinkIndexState> {
        self.authorize(caller_account_id, space_id, true).await?;
        Ok(self.index.request_rebuild(space_id).await?)
    }

    pub async fn node_links(
        &self,
        caller_account_id: Uuid,
        space_id: Uuid,
        node_id: Uuid,
    ) -> ServiceResult<NodeLinkSummary> {
        self.authorize(caller_account_id, space_id, false).await?;
        if self.files.find_node(space_id, node_id).await?.is_none() {
            return Err(ServiceError::NotFound("node not found".to_owned()));
        }
        let records = self
            .index
            .node_links(space_id, node_id, NODE_LINK_PREVIEW_LIMIT)
            .await?
            .ok_or_else(|| ServiceError::Internal("link index state is missing".to_owned()))?;
        self.hydrate_records(records).await
    }

    pub async fn process_next(&self) -> ServiceResult<LinkIndexRun> {
        let Some(claim) = self.index.claim_next(CLAIM_LEASE, PARSER_VERSION).await? else {
            return Ok(LinkIndexRun::Idle);
        };
        let result = self.process_claim(&claim).await;
        if let Err(error) = &result {
            self.index
                .fail_claim(&claim, &error.to_string())
                .await
                .map_err(|failure_error| {
                    ServiceError::Internal(format!(
                        "link indexing failed ({error}); releasing the claim also failed: {failure_error}"
                    ))
                })?;
        }
        result
    }

    async fn process_claim(&self, claim: &LinkIndexClaim) -> ServiceResult<LinkIndexRun> {
        if claim.rebuild_requested
            || claim.parser_version < PARSER_VERSION
            || claim.status == LinkIndexStatus::Rebuilding
            || claim.rebuild_base_generation.is_some()
        {
            return self
                .rebuild(
                    claim,
                    claim.rebuild_requested || claim.parser_version < PARSER_VERSION,
                )
                .await;
        }

        let batch = self.index.events_after(claim, EVENT_BATCH_SIZE).await?;
        if !batch.cursor_valid
            || (batch.events.is_empty() && claim.applied_generation < claim.desired_generation)
        {
            self.index.request_claim_rebuild(claim).await?;
            return Ok(LinkIndexRun::RebuildQueued {
                space_id: claim.space_id,
            });
        }
        if batch.events.is_empty() {
            self.index
                .commit_incremental(claim, &[], &[], false, claim.applied_generation)
                .await?;
            return Ok(LinkIndexRun::Incremental {
                space_id: claim.space_id,
                events: 0,
            });
        }

        let impact = EventImpact::from_events(&batch.events);
        if impact.rebuild {
            return self.rebuild(claim, true).await;
        }
        let source_ids = impact.dirty_sources.into_iter().collect::<Vec<_>>();
        let sources = self.source_link_sets(claim.space_id, &source_ids).await?;
        let rebind_targets = self
            .live_target_paths(
                claim.space_id,
                &impact.created_targets.into_iter().collect::<Vec<_>>(),
            )
            .await?;
        let last_generation = batch
            .events
            .last()
            .map(|event| event.generation)
            .ok_or_else(|| ServiceError::Internal("link index event batch is empty".to_owned()))?;
        let event_count = batch.events.len();
        self.index
            .commit_incremental(
                claim,
                &sources,
                &rebind_targets,
                impact.cleanup_deleted,
                last_generation,
            )
            .await?;
        Ok(LinkIndexRun::Incremental {
            space_id: claim.space_id,
            events: event_count,
        })
    }

    async fn rebuild(&self, claim: &LinkIndexClaim, restart: bool) -> ServiceResult<LinkIndexRun> {
        let (base_generation, mut cursor) = if restart || claim.rebuild_base_generation.is_none() {
            (
                self.index
                    .begin_rebuild(claim, PARSER_VERSION, CLAIM_LEASE)
                    .await?,
                None,
            )
        } else {
            (
                claim.rebuild_base_generation.ok_or_else(|| {
                    ServiceError::Internal("link rebuild base is missing".to_owned())
                })?,
                claim.rebuild_after_node_id,
            )
        };

        loop {
            let (source_ids, has_more) = self
                .index
                .rebuild_source_ids(claim.space_id, cursor, REBUILD_SOURCE_BATCH_SIZE)
                .await?;
            if source_ids.is_empty() {
                self.index.finish_rebuild(claim, base_generation).await?;
                return Ok(LinkIndexRun::Rebuilt {
                    space_id: claim.space_id,
                });
            }
            let last_node_id = source_ids
                .last()
                .copied()
                .ok_or_else(|| ServiceError::Internal("link rebuild batch is empty".to_owned()))?;
            let sources = self.source_link_sets(claim.space_id, &source_ids).await?;
            self.index
                .commit_rebuild_batch(claim, &sources, last_node_id, CLAIM_LEASE)
                .await?;
            cursor = Some(last_node_id);
            if !has_more {
                self.index.finish_rebuild(claim, base_generation).await?;
                return Ok(LinkIndexRun::Rebuilt {
                    space_id: claim.space_id,
                });
            }
        }
    }

    async fn source_link_sets(
        &self,
        space_id: Uuid,
        source_ids: &[Uuid],
    ) -> ServiceResult<Vec<SourceLinkSet>> {
        if source_ids.is_empty() {
            return Ok(Vec::new());
        }
        let texts = self.files.find_texts(space_id, source_ids).await?;
        let paths = self.files.node_paths_many(space_id, source_ids).await?;
        let mut parsed = HashMap::new();
        let mut target_paths = BTreeSet::new();

        for source_id in source_ids {
            let references = match (texts.get(source_id), paths.get(source_id)) {
                (Some(text), Some(path)) => text
                    .content
                    .as_deref()
                    .map(|content| markdown::parse_references(path, content))
                    .unwrap_or_default(),
                _ => Vec::new(),
            };
            for reference in &references {
                if let Some(path) = &reference.normalized_target_path {
                    target_paths.insert(path.clone());
                }
            }
            parsed.insert(*source_id, references);
        }

        let target_paths = target_paths.into_iter().collect::<Vec<_>>();
        let targets = self
            .files
            .resolve_nodes_by_paths(space_id, &target_paths)
            .await?
            .into_iter()
            .map(|(_index, path, node)| (path, node.id))
            .collect::<HashMap<_, _>>();

        Ok(source_ids
            .iter()
            .map(|source_id| {
                let references = parsed
                    .remove(source_id)
                    .unwrap_or_default()
                    .into_iter()
                    .map(|reference| NewLinkReference {
                        target_node_id: reference
                            .normalized_target_path
                            .as_ref()
                            .and_then(|path| targets.get(path).copied()),
                        kind: reference.kind,
                        raw_href: reference.raw_href,
                        normalized_target_path: reference.normalized_target_path,
                        occurrence_count: reference.occurrence_count,
                    })
                    .collect();
                SourceLinkSet {
                    source_node_id: *source_id,
                    references,
                }
            })
            .collect())
    }

    async fn live_target_paths(
        &self,
        space_id: Uuid,
        node_ids: &[Uuid],
    ) -> ServiceResult<Vec<(String, Uuid)>> {
        let paths = self.files.node_paths_many(space_id, node_ids).await?;
        Ok(node_ids
            .iter()
            .filter_map(|node_id| paths.get(node_id).cloned().map(|path| (path, *node_id)))
            .collect())
    }

    async fn hydrate_records(&self, records: NodeLinkRecords) -> ServiceResult<NodeLinkSummary> {
        if matches!(
            records.index.freshness(),
            LinkIndexFreshness::Rebuilding | LinkIndexFreshness::Failed
        ) {
            return Ok(NodeLinkSummary {
                index: records.index,
                outgoing_count: 0,
                incoming_count: 0,
                broken_count: 0,
                outgoing: Vec::new(),
                incoming: Vec::new(),
                outgoing_truncated: false,
                incoming_truncated: false,
            });
        }

        let node_ids = records
            .outgoing
            .iter()
            .chain(&records.incoming)
            .flat_map(|record| [Some(record.source_node_id), record.target_node_id])
            .flatten()
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect::<Vec<_>>();
        let paths = self
            .files
            .node_paths_many(records.index.space_id, &node_ids)
            .await?;
        Ok(NodeLinkSummary {
            index: records.index,
            outgoing_count: records.outgoing_count,
            incoming_count: records.incoming_count,
            broken_count: records.broken_count,
            outgoing: records
                .outgoing
                .into_iter()
                .map(|record| hydrate_reference(record, &paths))
                .collect(),
            incoming: records
                .incoming
                .into_iter()
                .map(|record| hydrate_reference(record, &paths))
                .collect(),
            outgoing_truncated: records.outgoing_truncated,
            incoming_truncated: records.incoming_truncated,
        })
    }

    async fn authorize(
        &self,
        account_id: Uuid,
        space_id: Uuid,
        write: bool,
    ) -> ServiceResult<Permission> {
        let permission = self
            .files
            .permission_for(space_id, account_id)
            .await?
            .ok_or_else(|| ServiceError::NotFound("space not found".to_owned()))?;
        if write && !permission.allows_write() {
            return Err(ServiceError::Forbidden(
                "link reindexing requires write permission".to_owned(),
            ));
        }
        Ok(permission)
    }
}

fn hydrate_reference(record: LinkReferenceRecord, paths: &HashMap<Uuid, String>) -> LinkReference {
    let status = if record.normalized_target_path.is_none() {
        LinkReferenceStatus::Invalid
    } else if record.target_node_id.is_none() {
        LinkReferenceStatus::Missing
    } else if record.target_deleted {
        LinkReferenceStatus::Deleted
    } else {
        LinkReferenceStatus::Resolved
    };
    LinkReference {
        id: record.id,
        kind: record.kind,
        status,
        raw_href: record.raw_href,
        normalized_target_path: record.normalized_target_path,
        occurrence_count: record.occurrence_count,
        source_node_id: record.source_node_id,
        source_name: record.source_name,
        source_path: paths.get(&record.source_node_id).cloned(),
        target_node_id: record.target_node_id,
        target_name: record.target_name,
        target_path: record
            .target_node_id
            .and_then(|target_node_id| paths.get(&target_node_id).cloned()),
    }
}

#[derive(Default)]
struct EventImpact {
    dirty_sources: BTreeSet<Uuid>,
    created_targets: BTreeSet<Uuid>,
    cleanup_deleted: bool,
    rebuild: bool,
}

impl EventImpact {
    fn from_events(events: &[QueuedLinkIndexEvent]) -> Self {
        let mut impact = Self::default();
        for queued in events {
            let event = &queued.event;
            let Some(node_id) = event.node_id else {
                impact.rebuild = true;
                continue;
            };
            match event.op_type.as_str() {
                "text.create" => {
                    impact.dirty_sources.insert(node_id);
                    impact.created_targets.insert(node_id);
                }
                "text.write" | "text.append" | "text.patch" | "text.edit" => {
                    impact.dirty_sources.insert(node_id);
                }
                "folder.create" | "file.create" => {
                    impact.created_targets.insert(node_id);
                }
                "metadata.replace" | "metadata.patch" => {}
                "item.update" => {
                    if event
                        .metadata
                        .get("name_changed")
                        .and_then(serde_json::Value::as_bool)
                        .unwrap_or(false)
                    {
                        impact.rebuild = true;
                    } else if event
                        .metadata
                        .get("text_encryption_changed")
                        .and_then(serde_json::Value::as_bool)
                        .unwrap_or(false)
                    {
                        impact.dirty_sources.insert(node_id);
                    }
                }
                "item.delete" => impact.cleanup_deleted = true,
                "item.move" | "item.copy" => impact.rebuild = true,
                _ => impact.rebuild = true,
            }
        }
        impact
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use chrono::Utc;
    use notegate_model::FileChangeEvent;
    use serde_json::json;

    use super::*;

    fn event(
        id: i64,
        node_id: Uuid,
        op_type: &str,
        metadata: serde_json::Value,
    ) -> QueuedLinkIndexEvent {
        QueuedLinkIndexEvent {
            generation: id,
            event: FileChangeEvent {
                id,
                created_at: Utc::now(),
                space_id: Uuid::new_v4(),
                node_id: Some(node_id),
                actor_account_id: None,
                op_type: op_type.to_owned(),
                metadata,
            },
        }
    }

    #[test]
    fn coalesces_text_changes_to_one_dirty_source() {
        let node_id = Uuid::new_v4();
        let impact = EventImpact::from_events(&[
            event(1, node_id, "text.write", json!({})),
            event(2, node_id, "text.patch", json!({})),
        ]);
        assert_eq!(impact.dirty_sources, BTreeSet::from([node_id]));
        assert!(!impact.rebuild);
    }

    #[test]
    fn topology_changes_require_a_space_rebuild() {
        let node_id = Uuid::new_v4();
        for (op_type, metadata) in [
            ("item.move", json!({})),
            ("item.copy", json!({})),
            ("item.update", json!({ "name_changed": true })),
        ] {
            assert!(EventImpact::from_events(&[event(1, node_id, op_type, metadata)]).rebuild);
        }
    }

    #[test]
    fn delete_cleans_sources_without_discarding_target_refs() {
        let node_id = Uuid::new_v4();
        let impact = EventImpact::from_events(&[event(1, node_id, "item.delete", json!({}))]);
        assert!(impact.cleanup_deleted);
        assert!(!impact.rebuild);
    }
}
