//! Background projection of file-change events into Markdown link relations.

use std::collections::{BTreeSet, HashMap};
use std::time::Duration;

use notegate_db::{
    FilesRepo, LinkIndexClaim, LinkIndexRepo, NewLinkReference, QueuedLinkIndexEvent, SourceLinkSet,
};
use notegate_model::LinkIndexStatus;
use uuid::Uuid;

use crate::error::{ServiceError, ServiceResult};

use super::markdown;

const PARSER_VERSION: i32 = 1;
const CLAIM_LEASE: Duration = Duration::from_secs(120);
const EVENT_BATCH_SIZE: i64 = 200;
const INCREMENTAL_SOURCE_LIMIT: usize = 8;
const REBUILD_SOURCE_BATCH_SIZE: i64 = 8;

#[derive(Debug, Clone)]
pub struct LinkIndexProjector {
    index: LinkIndexRepo,
    files: FilesRepo,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LinkIndexRun {
    Idle,
    Incremental { space_id: Uuid, events: usize },
    RebuildProgress { space_id: Uuid, sources: usize },
    Rebuilt { space_id: Uuid },
    RebuildQueued { space_id: Uuid },
}

impl LinkIndexProjector {
    pub fn new(index: LinkIndexRepo, files: FilesRepo) -> Self {
        Self { index, files }
    }

    pub async fn ensure_compatible(&self) -> ServiceResult<()> {
        let newest_version = self.index.newest_parser_version().await?.unwrap_or(0);
        if newest_version > PARSER_VERSION {
            return Err(ServiceError::Internal(format!(
                "link index parser version {newest_version} is newer than this binary ({PARSER_VERSION}); roll forward"
            )));
        }
        Ok(())
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
                .commit_incremental(claim, &[], false, claim.applied_generation)
                .await?;
            return Ok(LinkIndexRun::Incremental {
                space_id: claim.space_id,
                events: 0,
            });
        }

        let Some((impact, event_count)) = EventImpact::bounded_incremental(&batch.events) else {
            self.index.request_claim_rebuild(claim).await?;
            return Ok(LinkIndexRun::RebuildQueued {
                space_id: claim.space_id,
            });
        };
        let source_ids = impact.dirty_sources.into_iter().collect::<Vec<_>>();
        let sources = self.source_link_sets(claim.space_id, &source_ids).await?;
        let rebind_targets = self
            .live_target_paths(
                claim.space_id,
                &impact.created_targets.into_iter().collect::<Vec<_>>(),
            )
            .await?;
        let last_generation_index = event_count
            .checked_sub(1)
            .ok_or_else(|| ServiceError::Internal("link index event batch is empty".to_owned()))?;
        let last_generation = batch
            .events
            .get(last_generation_index)
            .map(|event| event.generation)
            .ok_or_else(|| ServiceError::Internal("link index event batch is empty".to_owned()))?;
        self.index
            .rewrite_sources(claim, &sources, CLAIM_LEASE)
            .await?;
        self.index
            .commit_incremental(
                claim,
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
        let (base_generation, cursor) = if restart || claim.rebuild_base_generation.is_none() {
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
            .rewrite_sources(claim, &sources, CLAIM_LEASE)
            .await?;
        self.index
            .commit_rebuild_batch(claim, last_node_id, has_more, base_generation)
            .await?;
        if has_more {
            Ok(LinkIndexRun::RebuildProgress {
                space_id: claim.space_id,
                sources: source_ids.len(),
            })
        } else {
            Ok(LinkIndexRun::Rebuilt {
                space_id: claim.space_id,
            })
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
}

#[derive(Default)]
struct EventImpact {
    dirty_sources: BTreeSet<Uuid>,
    created_targets: BTreeSet<Uuid>,
    cleanup_deleted: bool,
    rebuild: bool,
}

impl EventImpact {
    fn bounded_incremental(events: &[QueuedLinkIndexEvent]) -> Option<(Self, usize)> {
        let mut impact = Self::default();
        let mut event_count = 0;

        for event in events {
            let next = Self::from_event(event);
            if next.rebuild {
                return None;
            }
            let additional_sources = next.dirty_sources.difference(&impact.dirty_sources).count();
            if impact.dirty_sources.len() + additional_sources > INCREMENTAL_SOURCE_LIMIT {
                break;
            }
            impact.merge(next);
            event_count += 1;
        }

        Some((impact, event_count))
    }

    #[cfg(test)]
    fn from_events(events: &[QueuedLinkIndexEvent]) -> Self {
        let mut impact = Self::default();
        for queued in events {
            impact.merge(Self::from_event(queued));
        }
        impact
    }

    fn from_event(queued: &QueuedLinkIndexEvent) -> Self {
        let mut impact = Self::default();
        let event = &queued.event;
        let Some(node_id) = event.node_id else {
            impact.rebuild = true;
            return impact;
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
        impact
    }

    fn merge(&mut self, other: Self) {
        self.dirty_sources.extend(other.dirty_sources);
        self.created_targets.extend(other.created_targets);
        self.cleanup_deleted |= other.cleanup_deleted;
        self.rebuild |= other.rebuild;
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
    fn large_incremental_fanout_is_split_without_a_space_rebuild() {
        let events = (0..=INCREMENTAL_SOURCE_LIMIT)
            .map(|_| event(1, Uuid::new_v4(), "text.write", json!({})))
            .collect::<Vec<_>>();

        let (first, first_count) = EventImpact::bounded_incremental(&events).unwrap();
        assert_eq!(first_count, INCREMENTAL_SOURCE_LIMIT);
        assert_eq!(first.dirty_sources.len(), INCREMENTAL_SOURCE_LIMIT);
        assert!(!first.rebuild);

        let (_, remaining) = events.split_at(first_count);
        let (second, second_count) = EventImpact::bounded_incremental(remaining).unwrap();
        assert_eq!(second_count, 1);
        assert_eq!(second.dirty_sources.len(), 1);
        assert!(!second.rebuild);
    }

    #[test]
    fn delete_cleans_sources_without_discarding_target_refs() {
        let node_id = Uuid::new_v4();
        let impact = EventImpact::from_events(&[event(1, node_id, "item.delete", json!({}))]);
        assert!(impact.cleanup_deleted);
        assert!(!impact.rebuild);
    }
}
