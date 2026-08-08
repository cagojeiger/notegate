use std::collections::{BTreeMap, HashMap};

use notegate_core::limits;
use notegate_db::{
    FilesRepo, LinkIndexRepo, ReconciliationClaim, SpaceLinkStatus, StoredLinkReference,
};
use notegate_model::{
    AccountKind, Caller, Channel, IncomingLinkCursor, LinkReferenceKind, LinkReferencePage,
    LinkReferenceView, LinkSyncStatus, ListLinkReferences, NodeLinkIndexView, OutgoingLinkCursor,
    Permission, SpaceLinkIndexView,
};
use percent_encoding::percent_decode_str;
use pulldown_cmark::{Event, Options, Parser, Tag};
use uuid::Uuid;

use crate::error::{ServiceError, ServiceResult};
use crate::pagination::paginate_keyset;

#[derive(Debug, Clone)]
pub struct LinkIndexService {
    store: LinkIndexRepo,
    files: FilesRepo,
}

impl LinkIndexService {
    pub fn new(store: LinkIndexRepo, files: FilesRepo) -> Self {
        Self { store, files }
    }

    pub async fn node(
        &self,
        caller_account_id: Uuid,
        space_id: Uuid,
        node_id: Uuid,
    ) -> ServiceResult<NodeLinkIndexView> {
        self.require_visible_node(caller_account_id, space_id, node_id)
            .await?;
        let space_state = self.store.space_status(space_id).await?;

        Ok(NodeLinkIndexView {
            status: sync_status(&space_state),
            last_synced_at: space_state.last_synced_at,
        })
    }

    pub async fn outgoing(
        &self,
        caller_account_id: Uuid,
        space_id: Uuid,
        node_id: Uuid,
        request: ListLinkReferences,
    ) -> ServiceResult<LinkReferencePage> {
        self.require_visible_node(caller_account_id, space_id, node_id)
            .await?;
        let (references, limit, has_more, next_cursor) = paginate_keyset(
            request.limit,
            limits::LINK_REFERENCES_DEFAULT_LIMIT,
            limits::LINK_REFERENCES_MAX_LIMIT,
            request.cursor.as_deref(),
            |limit, cursor: Option<OutgoingLinkCursor>| async move {
                if cursor.as_ref().is_some_and(|cursor| {
                    cursor.space_id != space_id || cursor.source_node_id != node_id
                }) {
                    return Err(ServiceError::InvalidInput(
                        "cursor does not match outgoing links query".to_owned(),
                    ));
                }
                Ok(self
                    .store
                    .outgoing(space_id, node_id, limit, cursor.as_ref())
                    .await?)
            },
            |reference| OutgoingLinkCursor {
                space_id,
                source_node_id: node_id,
                target_path: reference.target_path.clone(),
                kind: reference.kind,
            },
        )
        .await?;
        let items = references
            .into_iter()
            .map(|reference| LinkReferenceView {
                node_id: reference.target_node_id,
                path: reference.target_path,
                kind: reference.kind,
                occurrence_count: reference.occurrence_count,
            })
            .collect();
        Ok(LinkReferencePage {
            items,
            limit,
            has_more,
            next_cursor,
        })
    }

    pub async fn incoming(
        &self,
        caller_account_id: Uuid,
        space_id: Uuid,
        node_id: Uuid,
        request: ListLinkReferences,
    ) -> ServiceResult<LinkReferencePage> {
        self.require_visible_node(caller_account_id, space_id, node_id)
            .await?;
        let (incoming, limit, has_more, next_cursor) = paginate_keyset(
            request.limit,
            limits::LINK_REFERENCES_DEFAULT_LIMIT,
            limits::LINK_REFERENCES_MAX_LIMIT,
            request.cursor.as_deref(),
            |limit, cursor: Option<IncomingLinkCursor>| async move {
                if cursor.as_ref().is_some_and(|cursor| {
                    cursor.space_id != space_id || cursor.target_node_id != node_id
                }) {
                    return Err(ServiceError::InvalidInput(
                        "cursor does not match incoming links query".to_owned(),
                    ));
                }
                Ok(self
                    .store
                    .incoming(space_id, node_id, limit, cursor.as_ref())
                    .await?)
            },
            |reference| IncomingLinkCursor {
                space_id,
                target_node_id: node_id,
                source_node_id: reference.source_node_id,
                kind: reference.kind,
            },
        )
        .await?;
        let incoming_ids = incoming
            .iter()
            .map(|reference| reference.source_node_id)
            .collect::<Vec<_>>();
        let incoming_paths = self.files.node_paths_many(space_id, &incoming_ids).await?;
        let items = incoming
            .into_iter()
            .filter_map(|reference| {
                incoming_paths
                    .get(&reference.source_node_id)
                    .map(|path| LinkReferenceView {
                        node_id: Some(reference.source_node_id),
                        path: path.clone(),
                        kind: reference.kind,
                        occurrence_count: reference.occurrence_count,
                    })
            })
            .collect::<Vec<_>>();
        Ok(LinkReferencePage {
            items,
            limit,
            has_more,
            next_cursor,
        })
    }

    pub async fn space(
        &self,
        caller_account_id: Uuid,
        space_id: Uuid,
    ) -> ServiceResult<SpaceLinkIndexView> {
        self.require_permission(caller_account_id, space_id, Permission::Read)
            .await?;
        let state = self.store.space_status(space_id).await?;
        Ok(SpaceLinkIndexView {
            status: sync_status(&state),
            pending_documents: state.pending_documents,
            retrying_documents: state.retrying_documents,
            last_synced_at: state.last_synced_at,
        })
    }

    pub async fn request_node(
        &self,
        caller: &Caller,
        space_id: Uuid,
        node_id: Uuid,
    ) -> ServiceResult<()> {
        require_dashboard_user(caller)?;
        self.require_permission(caller.account_id(), space_id, Permission::Write)
            .await?;
        if !self.store.request_source(space_id, node_id).await? {
            return Err(ServiceError::NotFound("text not found".to_owned()));
        }
        Ok(())
    }

    pub async fn request_space(&self, caller: &Caller, space_id: Uuid) -> ServiceResult<()> {
        require_dashboard_user(caller)?;
        self.require_permission(caller.account_id(), space_id, Permission::Write)
            .await?;
        if !self.store.request_space(space_id).await? {
            return Err(ServiceError::NotFound("space not found".to_owned()));
        }
        Ok(())
    }

    pub async fn execute_next(&self) -> notegate_core::Result<LinkIndexExecution> {
        let Some(claim) = self.store.claim_work().await? else {
            return Ok(LinkIndexExecution::Idle);
        };
        if LinkIndexRepo::is_space_work(&claim) {
            return match self.store.expand_space(&claim).await {
                Ok(true) => Ok(LinkIndexExecution::SpaceExpanded {
                    space_id: claim.space_id,
                }),
                Ok(false) => Ok(LinkIndexExecution::ClaimLost),
                Err(error) => {
                    self.store.fail_work(&claim, &error.to_string()).await?;
                    Ok(LinkIndexExecution::Failed {
                        space_id: claim.space_id,
                        source_node_id: None,
                        error: error.to_string(),
                    })
                }
            };
        }
        if !LinkIndexRepo::is_source_work(&claim) {
            let error = format!("unsupported projection work kind: {}", claim.work_kind);
            self.store.fail_work(&claim, &error).await?;
            return Ok(LinkIndexExecution::Failed {
                space_id: claim.space_id,
                source_node_id: None,
                error,
            });
        }
        match self.execute_source(&claim).await {
            Ok(execution) => Ok(execution),
            Err(error) => {
                self.store.fail_work(&claim, &error.to_string()).await?;
                Ok(LinkIndexExecution::Failed {
                    space_id: claim.space_id,
                    source_node_id: Some(claim.target_id),
                    error: error.to_string(),
                })
            }
        }
    }

    async fn execute_source(
        &self,
        claim: &ReconciliationClaim,
    ) -> notegate_core::Result<LinkIndexExecution> {
        let Some(references) = self.references_for_source(claim).await? else {
            return Ok(match self.store.discard_source(claim).await? {
                true => LinkIndexExecution::SourceDiscarded {
                    space_id: claim.space_id,
                    source_node_id: claim.target_id,
                },
                false => LinkIndexExecution::ClaimLost,
            });
        };
        Ok(
            match self.store.complete_source(claim, &references).await? {
                true => LinkIndexExecution::SourceIndexed {
                    space_id: claim.space_id,
                    source_node_id: claim.target_id,
                    reference_count: references.len(),
                },
                false => LinkIndexExecution::ClaimLost,
            },
        )
    }

    async fn references_for_source(
        &self,
        claim: &ReconciliationClaim,
    ) -> notegate_core::Result<Option<Vec<StoredLinkReference>>> {
        let Some((_, text)) = self
            .files
            .find_text(claim.space_id, claim.target_id)
            .await?
        else {
            return Ok(None);
        };
        let source_path = self
            .files
            .node_path(claim.space_id, claim.target_id)
            .await?
            .ok_or_else(|| notegate_core::Error::not_found("text not found"))?;
        let parsed = parse_internal_references(&source_path, text.content.as_deref().unwrap_or(""));
        let paths = parsed
            .iter()
            .map(|reference| reference.target_path.clone())
            .collect::<Vec<_>>();
        let resolved = self
            .files
            .resolve_nodes_by_paths(claim.space_id, &paths)
            .await?
            .into_iter()
            .map(|(_, path, node)| (path, node.id))
            .collect::<HashMap<_, _>>();
        Ok(Some(
            parsed
                .into_iter()
                .map(|reference| StoredLinkReference {
                    target_node_id: resolved.get(&reference.target_path).copied(),
                    target_path: reference.target_path,
                    kind: reference.kind,
                    occurrence_count: reference.occurrence_count,
                })
                .collect(),
        ))
    }

    async fn require_permission(
        &self,
        caller_account_id: Uuid,
        space_id: Uuid,
        required: Permission,
    ) -> ServiceResult<()> {
        let permission = self
            .files
            .permission_for(space_id, caller_account_id)
            .await?
            .ok_or_else(|| ServiceError::NotFound("space not found".to_owned()))?;
        if required == Permission::Write && permission != Permission::Write {
            return Err(ServiceError::Forbidden(
                "write permission is required".to_owned(),
            ));
        }
        Ok(())
    }

    async fn require_visible_node(
        &self,
        caller_account_id: Uuid,
        space_id: Uuid,
        node_id: Uuid,
    ) -> ServiceResult<()> {
        self.require_permission(caller_account_id, space_id, Permission::Read)
            .await?;
        self.files
            .find_node(space_id, node_id)
            .await?
            .ok_or_else(|| ServiceError::NotFound("node not found".to_owned()))?;
        Ok(())
    }
}

fn require_dashboard_user(caller: &Caller) -> ServiceResult<()> {
    if caller.account.kind != AccountKind::User || caller.channel != Channel::Browser {
        return Err(ServiceError::Forbidden(
            "link indexing can only be requested from the dashboard".to_owned(),
        ));
    }
    Ok(())
}

fn sync_status(state: &SpaceLinkStatus) -> LinkSyncStatus {
    if state.space_syncing || state.syncing_documents > 0 {
        LinkSyncStatus::Syncing
    } else if state.space_error.is_some() || state.retrying_documents > 0 {
        LinkSyncStatus::Retrying
    } else if state.space_pending || state.pending_documents > 0 {
        LinkSyncStatus::Pending
    } else {
        LinkSyncStatus::UpToDate
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LinkIndexExecution {
    Idle,
    SpaceExpanded {
        space_id: Uuid,
    },
    SourceIndexed {
        space_id: Uuid,
        source_node_id: Uuid,
        reference_count: usize,
    },
    SourceDiscarded {
        space_id: Uuid,
        source_node_id: Uuid,
    },
    ClaimLost,
    Failed {
        space_id: Uuid,
        source_node_id: Option<Uuid>,
        error: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ParsedReference {
    target_path: String,
    kind: LinkReferenceKind,
    occurrence_count: i32,
}

fn parse_internal_references(source_path: &str, content: &str) -> Vec<ParsedReference> {
    let mut references = BTreeMap::<(String, bool), i32>::new();
    for event in Parser::new_ext(content, Options::all()) {
        let (href, image) = match event {
            Event::Start(Tag::Link { dest_url, .. }) => (dest_url, false),
            Event::Start(Tag::Image { dest_url, .. }) => (dest_url, true),
            _ => continue,
        };
        let Some(target_path) = internal_target_path(source_path, href.as_ref()) else {
            continue;
        };
        references
            .entry((target_path, image))
            .and_modify(|count| *count += 1)
            .or_insert(1);
    }
    references
        .into_iter()
        .map(|((target_path, image), occurrence_count)| ParsedReference {
            target_path,
            kind: if image {
                LinkReferenceKind::Image
            } else {
                LinkReferenceKind::Link
            },
            occurrence_count,
        })
        .collect()
}

fn internal_target_path(source_path: &str, href: &str) -> Option<String> {
    let value = href.trim();
    if value.is_empty()
        || value.starts_with('#')
        || value.starts_with("//")
        || has_url_scheme(value)
    {
        return None;
    }
    let path = value.split_once('#').map_or(value, |(path, _)| path);
    if path.is_empty() || path.contains('?') {
        return None;
    }
    let decoded = decode_path_segments(path)?;
    let absolute = if decoded.starts_with('/') {
        decoded
    } else {
        let parent = source_path
            .rfind('/')
            .filter(|index| *index > 0)
            .map_or("/", |index| &source_path[..index]);
        format!("{}/{decoded}", parent.trim_end_matches('/'))
    };
    let normalized = normalize_absolute_path(&absolute)?;
    notegate_core::validation::normalize_path(&normalized).ok()
}

fn has_url_scheme(value: &str) -> bool {
    let Some(colon) = value.find(':') else {
        return false;
    };
    if value
        .find(['/', '?', '#'])
        .is_some_and(|separator| separator < colon)
    {
        return false;
    }
    let scheme = &value[..colon];
    let mut chars = scheme.chars();
    chars
        .next()
        .is_some_and(|first| first.is_ascii_alphabetic())
        && chars.all(|character| {
            character.is_ascii_alphanumeric() || matches!(character, '+' | '-' | '.')
        })
}

fn decode_path_segments(path: &str) -> Option<String> {
    path.split('/')
        .map(|segment| {
            if !has_valid_percent_encoding(segment) {
                return None;
            }
            let decoded = percent_decode_str(segment).decode_utf8().ok()?;
            (!decoded.contains('/') && !decoded.chars().any(char::is_control))
                .then(|| decoded.into_owned())
        })
        .collect::<Option<Vec<_>>>()
        .map(|segments| segments.join("/"))
}

fn has_valid_percent_encoding(value: &str) -> bool {
    let mut bytes = value.bytes();
    while let Some(byte) = bytes.next() {
        if byte != b'%' {
            continue;
        }
        let (Some(first), Some(second)) = (bytes.next(), bytes.next()) else {
            return false;
        };
        if !first.is_ascii_hexdigit() || !second.is_ascii_hexdigit() {
            return false;
        }
    }
    true
}

fn normalize_absolute_path(path: &str) -> Option<String> {
    let mut segments = Vec::new();
    for segment in path.split('/') {
        match segment {
            "" | "." => {}
            ".." => {
                segments.pop()?;
            }
            value => segments.push(value),
        }
    }
    Some(format!("/{}", segments.join("/")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parser_indexes_standard_internal_links_and_images() {
        let references = parse_internal_references(
            "/docs/current.md",
            "[one](../README.md) [again](../README.md#top) ![asset](./a%20b.png) \
             [web](https://example.com) [anchor](#local)",
        );
        assert_eq!(
            references,
            vec![
                ParsedReference {
                    target_path: "/README.md".to_owned(),
                    kind: LinkReferenceKind::Link,
                    occurrence_count: 2,
                },
                ParsedReference {
                    target_path: "/docs/a b.png".to_owned(),
                    kind: LinkReferenceKind::Image,
                    occurrence_count: 1,
                },
            ]
        );
    }

    #[test]
    fn path_rules_reject_escape_query_encoded_slash_and_protocols() {
        for href in [
            "../../outside.md",
            "note.md?view=1",
            "bad%path.md",
            "hidden%2Fchild.md",
            "mailto:test@example.com",
            "//example.com/note.md",
        ] {
            assert_eq!(
                internal_target_path("/docs/current.md", href),
                None,
                "{href}"
            );
        }
        assert_eq!(
            internal_target_path(
                "/docs/current.md",
                &format!("{}.md", "a".repeat(notegate_core::limits::MAX_PATH_LEN))
            ),
            None
        );
    }

    #[test]
    fn sync_status_prioritizes_active_and_failed_work() {
        assert_eq!(
            sync_status(&SpaceLinkStatus::default()),
            LinkSyncStatus::UpToDate
        );
        assert_eq!(
            sync_status(&SpaceLinkStatus {
                pending_documents: 1,
                ..SpaceLinkStatus::default()
            }),
            LinkSyncStatus::Pending
        );
        assert_eq!(
            sync_status(&SpaceLinkStatus {
                pending_documents: 1,
                retrying_documents: 1,
                ..SpaceLinkStatus::default()
            }),
            LinkSyncStatus::Retrying
        );
        assert_eq!(
            sync_status(&SpaceLinkStatus {
                pending_documents: 1,
                retrying_documents: 1,
                syncing_documents: 1,
                ..SpaceLinkStatus::default()
            }),
            LinkSyncStatus::Syncing
        );
    }
}
