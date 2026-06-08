#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic,
    clippy::unwrap_in_result
)]
use super::*;
use crate::files::StoredContent;
use chrono::Utc;
use notegate_core::Result as CoreResult;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use crate::files::types::Edit;

/// An in-memory file tree for exercising the service end-to-end without a DB.
#[derive(Clone)]
struct MemStore {
    role: Option<Role>,
    workspace_id: Uuid,
    root_id: Uuid,
    state: Arc<Mutex<State>>,
}

#[derive(Default)]
struct State {
    nodes: HashMap<Uuid, Node>,
    documents: HashMap<Uuid, Document>,
    mutate_before_save: Option<String>,
}

fn actor() -> Uuid {
    Uuid::nil()
}

impl MemStore {
    fn new(role: Option<Role>) -> Self {
        let workspace_id = Uuid::new_v4();
        let root_id = Uuid::new_v4();
        let mut nodes = HashMap::new();
        nodes.insert(
            root_id,
            raw_node(root_id, workspace_id, None, "/", NodeKind::Folder),
        );
        Self {
            role,
            workspace_id,
            root_id,
            state: Arc::new(Mutex::new(State {
                nodes,
                documents: HashMap::new(),
                mutate_before_save: None,
            })),
        }
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, State> {
        self.state.lock().expect("state lock")
    }

    /// Insert a folder directly (test setup).
    fn add_folder(&self, parent: Uuid, name: &str) -> Uuid {
        let id = Uuid::new_v4();
        self.lock().nodes.insert(
            id,
            raw_node(id, self.workspace_id, Some(parent), name, NodeKind::Folder),
        );
        id
    }

    /// Insert a document directly with content (test setup).
    fn add_document(&self, parent: Uuid, name: &str, content: &str) -> Uuid {
        let id = Uuid::new_v4();
        let metrics = content::compute(content);
        let mut state = self.lock();
        state.nodes.insert(
            id,
            raw_node(
                id,
                self.workspace_id,
                Some(parent),
                name,
                NodeKind::Document,
            ),
        );
        state.documents.insert(
            id,
            Document {
                node_id: id,
                workspace_id: self.workspace_id,
                content_md: content.to_owned(),
                content_sha256: metrics.content_sha256,
                byte_len: metrics.byte_len as i32,
                line_count: metrics.line_count as i32,
                created_by: actor(),
                updated_by: actor(),
                created_at: Utc::now(),
                updated_at: Utc::now(),
            },
        );
        id
    }

    fn mutate_before_next_save(&self, content: &str) {
        self.lock().mutate_before_save = Some(content.to_owned());
    }

    fn derive_path(state: &State, id: Uuid) -> Option<String> {
        let node = state.nodes.get(&id)?;
        match node.parent_id {
            None => Some("/".to_owned()),
            Some(parent) => {
                let parent_path = Self::derive_path(state, parent)?;
                Some(super::join_path(&parent_path, &node.name))
            }
        }
    }

    fn live_children(state: &State, parent: Uuid) -> Vec<Node> {
        let mut children: Vec<Node> = state
            .nodes
            .values()
            .filter(|n| n.parent_id == Some(parent) && n.deleted_at.is_none())
            .cloned()
            .collect();
        children.sort_by(|a, b| (a.sort_order, &a.name, a.id).cmp(&(b.sort_order, &b.name, b.id)));
        children
    }

    fn relative_depth(state: &State, id: Uuid) -> usize {
        Self::live_children(state, id)
            .into_iter()
            .map(|child| 1 + Self::relative_depth(state, child.id))
            .max()
            .unwrap_or(0)
    }

    fn subtree_count(state: &State, id: Uuid) -> usize {
        1 + Self::live_children(state, id)
            .into_iter()
            .map(|child| Self::subtree_count(state, child.id))
            .sum::<usize>()
    }

    fn is_descendant(state: &State, ancestor: Uuid, candidate: Uuid) -> bool {
        if ancestor == candidate {
            return true;
        }
        Self::live_children(state, ancestor)
            .into_iter()
            .any(|child| Self::is_descendant(state, child.id, candidate))
    }
}

fn raw_node(
    id: Uuid,
    workspace_id: Uuid,
    parent_id: Option<Uuid>,
    name: &str,
    kind: NodeKind,
) -> Node {
    Node {
        id,
        workspace_id,
        parent_id,
        name: name.to_owned(),
        kind,
        sort_order: 0,
        created_by: actor(),
        updated_by: actor(),
        deleted_by: None,
        purge_after: None,
        created_at: Utc::now(),
        updated_at: Utc::now(),
        deleted_at: None,
    }
}

impl FilesStore for MemStore {
    async fn role_for(&self, _ws: Uuid, _account: Uuid) -> CoreResult<Option<Role>> {
        Ok(self.role)
    }

    async fn root_node(&self, _ws: Uuid) -> CoreResult<Node> {
        Ok(self.lock().nodes.get(&self.root_id).cloned().expect("root"))
    }

    async fn find_node(&self, _ws: Uuid, id: Uuid) -> CoreResult<Option<Node>> {
        Ok(self
            .lock()
            .nodes
            .get(&id)
            .filter(|n| n.deleted_at.is_none())
            .cloned())
    }

    async fn node_path(&self, _ws: Uuid, id: Uuid) -> CoreResult<Option<String>> {
        let state = self.lock();
        Ok(Self::derive_path(&state, id))
    }

    async fn resolve_path(&self, _ws: Uuid, path: &str) -> CoreResult<Option<Uuid>> {
        let state = self.lock();
        let trimmed = path.trim();
        let mut current = Some(self.root_id);
        if trimmed.is_empty() || trimmed == "/" {
            return Ok(current);
        }
        for segment in trimmed.split('/').filter(|s| !s.is_empty()) {
            let Some(parent) = current else {
                return Ok(None);
            };
            current = Self::live_children(&state, parent)
                .into_iter()
                .find(|n| n.name == segment)
                .map(|n| n.id);
        }
        Ok(current)
    }

    async fn has_children(&self, _ws: Uuid, id: Uuid) -> CoreResult<bool> {
        Ok(!Self::live_children(&self.lock(), id).is_empty())
    }

    async fn count_live_children(&self, _ws: Uuid, parent: Uuid) -> CoreResult<usize> {
        Ok(Self::live_children(&self.lock(), parent).len())
    }

    async fn find_live_child_by_name(
        &self,
        _ws: Uuid,
        parent: Uuid,
        name: &str,
    ) -> CoreResult<Option<Node>> {
        Ok(Self::live_children(&self.lock(), parent)
            .into_iter()
            .find(|n| n.name == name))
    }

    async fn count_live_nodes(&self, _ws: Uuid) -> CoreResult<usize> {
        Ok(self
            .lock()
            .nodes
            .values()
            .filter(|n| n.deleted_at.is_none())
            .count())
    }

    async fn count_live_documents(&self, _ws: Uuid) -> CoreResult<usize> {
        let state = self.lock();
        Ok(state
            .nodes
            .values()
            .filter(|n| n.deleted_at.is_none() && n.kind == NodeKind::Document)
            .count())
    }

    async fn sum_live_document_bytes(&self, _ws: Uuid) -> CoreResult<usize> {
        let state = self.lock();
        Ok(state
            .documents
            .values()
            .filter(|d| {
                state
                    .nodes
                    .get(&d.node_id)
                    .map(|n| n.deleted_at.is_none())
                    .unwrap_or(false)
            })
            .map(|d| d.byte_len.max(0) as usize)
            .sum())
    }

    async fn document_stats(
        &self,
        _ws: Uuid,
        id: Uuid,
    ) -> CoreResult<Option<crate::files::DocumentStats>> {
        let state = self.lock();
        let Some(node) = state.nodes.get(&id).filter(|n| n.deleted_at.is_none()) else {
            return Ok(None);
        };
        if node.kind != NodeKind::Document {
            return Ok(None);
        }
        Ok(state
            .documents
            .get(&id)
            .map(|doc| crate::files::DocumentStats {
                content_sha256: doc.content_sha256.clone(),
                byte_len: doc.byte_len,
                line_count: doc.line_count,
            }))
    }

    async fn find_document(&self, _ws: Uuid, id: Uuid) -> CoreResult<Option<(Node, Document)>> {
        let state = self.lock();
        let Some(node) = state
            .nodes
            .get(&id)
            .filter(|n| n.deleted_at.is_none())
            .cloned()
        else {
            return Ok(None);
        };
        if node.kind != NodeKind::Document {
            return Ok(None);
        }
        Ok(state.documents.get(&id).cloned().map(|doc| (node, doc)))
    }

    async fn paged_children(
        &self,
        _ws: Uuid,
        parent: Uuid,
        limit: i64,
        cursor: Option<&ChildrenCursor>,
    ) -> CoreResult<(Vec<Node>, bool)> {
        let all = Self::live_children(&self.lock(), parent);
        let start = match cursor {
            None => 0,
            Some(c) => all
                .iter()
                .position(|n| {
                    (n.sort_order, n.name.as_str(), n.id) > (c.sort_order, c.name.as_str(), c.id)
                })
                .unwrap_or(all.len()),
        };
        let window: Vec<Node> = all
            .iter()
            .skip(start)
            .take(limit as usize)
            .cloned()
            .collect();
        let has_more = start + window.len() < all.len();
        Ok((window, has_more))
    }

    async fn subtree_relative_depth(&self, _ws: Uuid, id: Uuid) -> CoreResult<usize> {
        Ok(Self::relative_depth(&self.lock(), id))
    }

    async fn subtree_live_count(&self, _ws: Uuid, id: Uuid) -> CoreResult<usize> {
        Ok(Self::subtree_count(&self.lock(), id))
    }

    async fn is_self_or_descendant(
        &self,
        _ws: Uuid,
        node_id: Uuid,
        candidate: Uuid,
    ) -> CoreResult<bool> {
        Ok(Self::is_descendant(&self.lock(), node_id, candidate))
    }

    async fn insert_folder(
        &self,
        _ws: Uuid,
        command: &CreateFolder,
        created_by: Uuid,
    ) -> CoreResult<Node> {
        let id = Uuid::new_v4();
        let mut node = raw_node(
            id,
            self.workspace_id,
            Some(command.parent_node_id),
            &command.name,
            NodeKind::Folder,
        );
        node.created_by = created_by;
        node.updated_by = created_by;
        self.lock().nodes.insert(id, node.clone());
        Ok(node)
    }

    async fn insert_document(
        &self,
        _ws: Uuid,
        parent_node_id: Uuid,
        name: &str,
        content: &StoredContent,
        created_by: Uuid,
    ) -> CoreResult<(Node, Document)> {
        let id = Uuid::new_v4();
        let mut node = raw_node(
            id,
            self.workspace_id,
            Some(parent_node_id),
            name,
            NodeKind::Document,
        );
        node.created_by = created_by;
        node.updated_by = created_by;
        let doc = Document {
            node_id: id,
            workspace_id: self.workspace_id,
            content_md: content.content_md.clone(),
            content_sha256: content.content_sha256.clone(),
            byte_len: content.byte_len,
            line_count: content.line_count,
            created_by,
            updated_by: created_by,
            created_at: Utc::now(),
            updated_at: Utc::now(),
        };
        let mut state = self.lock();
        state.nodes.insert(id, node.clone());
        state.documents.insert(id, doc.clone());
        Ok((node, doc))
    }

    async fn save_document_content(
        &self,
        _ws: Uuid,
        node_id: Uuid,
        content: &StoredContent,
        expected_sha256: Option<&str>,
        updated_by: Uuid,
    ) -> CoreResult<(Node, Document)> {
        let mut state = self.lock();
        if let Some(content_md) = state.mutate_before_save.take() {
            let metrics = content::compute(&content_md);
            let doc = state.documents.get_mut(&node_id).expect("doc");
            doc.content_md = content_md;
            doc.content_sha256 = metrics.content_sha256;
            doc.byte_len = metrics.byte_len as i32;
            doc.line_count = metrics.line_count as i32;
        }
        let current_sha256 = state
            .documents
            .get(&node_id)
            .expect("doc")
            .content_sha256
            .clone();
        if let Some(expected) = expected_sha256
            && expected != current_sha256
        {
            return Err(notegate_core::Error::conflict(
                "expected_sha256 does not match the current document; read it again",
            ));
        }
        let node = state.nodes.get_mut(&node_id).expect("node");
        node.updated_by = updated_by;
        node.updated_at = Utc::now();
        let node = node.clone();
        let doc = state.documents.get_mut(&node_id).expect("doc");
        doc.content_md = content.content_md.clone();
        doc.content_sha256 = content.content_sha256.clone();
        doc.byte_len = content.byte_len;
        doc.line_count = content.line_count;
        doc.updated_by = updated_by;
        doc.updated_at = Utc::now();
        Ok((node, doc.clone()))
    }

    async fn move_node(&self, _ws: Uuid, command: &MoveNode, updated_by: Uuid) -> CoreResult<Node> {
        let mut state = self.lock();
        let node = state.nodes.get_mut(&command.node_id).expect("node");
        if let Some(expected_parent_id) = command.expected_parent_id
            && node.parent_id != Some(expected_parent_id)
        {
            return Err(notegate_core::Error::conflict(
                "expected_parent_id does not match the node's current parent; refresh and retry",
            ));
        }
        node.parent_id = Some(command.new_parent_node_id);
        if let Some(ref name) = command.new_name {
            node.name = name.clone();
        }
        node.updated_by = updated_by;
        node.updated_at = Utc::now();
        Ok(node.clone())
    }

    async fn update_node_metadata(
        &self,
        _ws: Uuid,
        node_id: Uuid,
        new_name: Option<&str>,
        new_sort_order: Option<i32>,
        updated_by: Uuid,
    ) -> CoreResult<Node> {
        let mut state = self.lock();
        let node = state.nodes.get_mut(&node_id).expect("node");
        if let Some(name) = new_name {
            node.name = name.to_owned();
        }
        if let Some(order) = new_sort_order {
            node.sort_order = order;
        }
        node.updated_by = updated_by;
        node.updated_at = Utc::now();
        Ok(node.clone())
    }

    async fn soft_delete_node(
        &self,
        _ws: Uuid,
        node_id: Uuid,
        deleted_by: Uuid,
    ) -> CoreResult<chrono::DateTime<Utc>> {
        // Soft-delete the node and its live subtree.
        let ids = {
            let state = self.lock();
            let mut stack = vec![node_id];
            let mut all = Vec::new();
            while let Some(id) = stack.pop() {
                all.push(id);
                for child in Self::live_children(&state, id) {
                    stack.push(child.id);
                }
            }
            all
        };
        let deleted_at = Utc::now();
        let purge_after = deleted_at + chrono::Duration::days(limits::DELETED_NODE_RETENTION_DAYS);
        let mut state = self.lock();
        for id in ids {
            if let Some(node) = state.nodes.get_mut(&id) {
                node.deleted_at = Some(deleted_at);
                node.deleted_by = Some(deleted_by);
                node.purge_after = Some(purge_after);
            }
        }
        Ok(purge_after)
    }
}

fn service(role: Option<Role>) -> (FilesService<MemStore>, MemStore) {
    service_with_limits(role, Limits::default())
}

fn service_with_limits(role: Option<Role>, limits: Limits) -> (FilesService<MemStore>, MemStore) {
    let store = MemStore::new(role);
    (FilesService::with_limits(store.clone(), limits), store)
}

// --- authorization wiring ---

#[tokio::test]
async fn no_role_is_not_found() {
    let (svc, store) = service(None);
    let err = svc.root(actor(), store.workspace_id).await.unwrap_err();
    assert!(matches!(err, ServiceError::NotFound(_)));
}

#[tokio::test]
async fn viewer_cannot_mkdir() {
    let (svc, store) = service(Some(Role::Viewer));
    let err = svc
        .create_folder(
            actor(),
            store.workspace_id,
            CreateFolder {
                parent_node_id: store.root_id,
                name: "notes".to_owned(),
            },
        )
        .await
        .unwrap_err();
    assert!(matches!(err, ServiceError::Forbidden(_)));
}

// --- mkdir / touch ---

#[tokio::test]
async fn editor_can_mkdir_and_path_is_derived() {
    let (svc, store) = service(Some(Role::Editor));
    let view = svc
        .create_folder(
            actor(),
            store.workspace_id,
            CreateFolder {
                parent_node_id: store.root_id,
                name: "notes".to_owned(),
            },
        )
        .await
        .unwrap();
    assert_eq!(view.path, "/notes");
    assert_eq!(view.node.kind, NodeKind::Folder);
}

#[tokio::test]
async fn mkdir_name_conflict_is_conflict() {
    let (svc, store) = service(Some(Role::Editor));
    store.add_folder(store.root_id, "notes");
    let err = svc
        .create_folder(
            actor(),
            store.workspace_id,
            CreateFolder {
                parent_node_id: store.root_id,
                name: "notes".to_owned(),
            },
        )
        .await
        .unwrap_err();
    assert!(matches!(err, ServiceError::Conflict(_)));
}

#[tokio::test]
async fn mkdir_rejects_depth_over_five() {
    let (svc, store) = service(Some(Role::Editor));
    // root/a/b/c/d/e is depth 5; creating under e would be depth 6.
    let mut parent = store.root_id;
    for name in ["a", "b", "c", "d", "e"] {
        parent = store.add_folder(parent, name);
    }
    let err = svc
        .create_folder(
            actor(),
            store.workspace_id,
            CreateFolder {
                parent_node_id: parent,
                name: "f".to_owned(),
            },
        )
        .await
        .unwrap_err();
    assert!(matches!(err, ServiceError::InvalidInput(_)));
}

#[tokio::test]
async fn touch_creates_empty_document() {
    let (svc, store) = service(Some(Role::Editor));
    let view = svc
        .create_document(
            actor(),
            store.workspace_id,
            CreateDocument {
                parent_node_id: store.root_id,
                name: "note.md".to_owned(),
            },
        )
        .await
        .unwrap();
    assert_eq!(view.node.path, "/note.md");
    assert_eq!(view.document.byte_len, 0);
    assert_eq!(view.document.line_count, 0);
}

// --- read ---

#[tokio::test]
async fn stat_document_includes_metrics_but_ls_omits_them() {
    let (svc, store) = service(Some(Role::Viewer));
    let doc = store.add_document(store.root_id, "note.md", "hello\n");

    let stat = svc.stat(actor(), store.workspace_id, doc).await.unwrap();
    let metrics = stat.document.expect("document stat includes metrics");
    assert_eq!(metrics.byte_len, 6);
    assert_eq!(metrics.line_count, 1);

    let page = svc
        .children(
            actor(),
            store.workspace_id,
            store.root_id,
            ChildrenRequest {
                limit: None,
                cursor: None,
            },
        )
        .await
        .unwrap();
    assert_eq!(page.items.len(), 1);
    assert!(
        page.items[0].document.is_none(),
        "ls keeps entries lightweight"
    );
}

#[tokio::test]
async fn read_unchanged_when_hash_matches() {
    let (svc, store) = service(Some(Role::Viewer));
    let id = store.add_document(store.root_id, "n.md", "# Note\n");
    let (_, doc) = store
        .find_document(store.workspace_id, id)
        .await
        .unwrap()
        .unwrap();
    let result = svc
        .read_document(
            actor(),
            store.workspace_id,
            ReadDocument {
                node_id: id,
                start_line: None,
                max_lines: None,
                max_bytes: None,
                if_none_match_sha256: Some(doc.content_sha256.clone()),
            },
        )
        .await
        .unwrap();
    assert!(result.unchanged());
    assert!(result.content.is_none());
}

#[tokio::test]
async fn read_truncates_by_max_lines() {
    let (svc, store) = service(Some(Role::Viewer));
    let id = store.add_document(store.root_id, "n.md", "l1\nl2\nl3\nl4\n");
    let result = svc
        .read_document(
            actor(),
            store.workspace_id,
            ReadDocument {
                node_id: id,
                start_line: Some(1),
                max_lines: Some(2),
                max_bytes: None,
                if_none_match_sha256: None,
            },
        )
        .await
        .unwrap();
    let content = result.content.unwrap();
    assert_eq!(content.returned_lines, 2);
    assert!(content.truncated);
    assert_eq!(content.next_start_line, Some(3));
}

#[tokio::test]
async fn read_folder_reports_folder_not_missing_document() {
    let (svc, store) = service(Some(Role::Viewer));
    let folder = store.add_folder(store.root_id, "notes");
    let err = svc
        .read_document(
            actor(),
            store.workspace_id,
            ReadDocument {
                node_id: folder,
                start_line: None,
                max_lines: None,
                max_bytes: None,
                if_none_match_sha256: None,
            },
        )
        .await
        .unwrap_err();
    assert!(
        matches!(err, ServiceError::InvalidInput(message) if message == "target is a folder, not a document")
    );
}

// --- write ---

#[tokio::test]
async fn write_existing_replaces_content() {
    let (svc, store) = service(Some(Role::Editor));
    let id = store.add_document(store.root_id, "n.md", "old");
    let view = svc
        .write_document(
            actor(),
            store.workspace_id,
            WriteDocument {
                target: WriteTarget::Existing { node_id: id },
                content_md: "new content\n".to_owned(),
                expected_sha256: None,
            },
        )
        .await
        .unwrap();
    assert_eq!(view.document.content_md, "new content\n");
    assert_eq!(view.document.line_count, 1);
}

#[tokio::test]
async fn write_create_makes_missing_document() {
    let (svc, store) = service(Some(Role::Editor));
    let view = svc
        .write_document(
            actor(),
            store.workspace_id,
            WriteDocument {
                target: WriteTarget::Create {
                    parent_node_id: store.root_id,
                    name: "fresh.md".to_owned(),
                },
                content_md: "# Fresh\n".to_owned(),
                expected_sha256: None,
            },
        )
        .await
        .unwrap();
    assert_eq!(view.node.path, "/fresh.md");
    assert_eq!(view.document.content_md, "# Fresh\n");
}

#[tokio::test]
async fn write_existing_missing_is_not_found() {
    let (svc, store) = service(Some(Role::Editor));
    let err = svc
        .write_document(
            actor(),
            store.workspace_id,
            WriteDocument {
                target: WriteTarget::Existing {
                    node_id: Uuid::new_v4(),
                },
                content_md: "x".to_owned(),
                expected_sha256: None,
            },
        )
        .await
        .unwrap_err();
    assert!(matches!(err, ServiceError::NotFound(_)));
}

#[tokio::test]
async fn write_expected_sha_mismatch_is_conflict() {
    let (svc, store) = service(Some(Role::Editor));
    let id = store.add_document(store.root_id, "n.md", "old");
    let err = svc
        .write_document(
            actor(),
            store.workspace_id,
            WriteDocument {
                target: WriteTarget::Existing { node_id: id },
                content_md: "new".to_owned(),
                expected_sha256: Some("deadbeef".to_owned()),
            },
        )
        .await
        .unwrap_err();
    assert!(matches!(err, ServiceError::Conflict(_)));
}

#[tokio::test]
async fn write_rejects_too_many_lines_as_invalid_input() {
    let (svc, store) = service(Some(Role::Editor));
    let id = store.add_document(store.root_id, "n.md", "x");
    let big = "a\n".repeat(limits::DOCUMENT_MAX_LINES + 1);
    let err = svc
        .write_document(
            actor(),
            store.workspace_id,
            WriteDocument {
                target: WriteTarget::Existing { node_id: id },
                content_md: big,
                expected_sha256: None,
            },
        )
        .await
        .unwrap_err();
    assert!(matches!(err, ServiceError::InvalidInput(_)));
}

// --- patch (integration through the service) ---

#[tokio::test]
async fn patch_applies_and_returns_previous_sha() {
    let (svc, store) = service(Some(Role::Editor));
    let id = store.add_document(store.root_id, "n.md", "hello world\n");
    let (_, before) = store
        .find_document(store.workspace_id, id)
        .await
        .unwrap()
        .unwrap();
    let result = svc
        .patch_document(
            actor(),
            store.workspace_id,
            PatchDocument {
                node_id: id,
                edits: vec![Edit {
                    old_text: "world".to_owned(),
                    new_text: "there".to_owned(),
                }],
                expected_sha256: None,
            },
        )
        .await
        .unwrap();
    assert_eq!(result.document.content_md, "hello there\n");
    assert_eq!(result.previous_sha256, before.content_sha256);
    assert_eq!(result.edits_applied, 1);
}

#[tokio::test]
async fn patch_no_match_is_conflict() {
    let (svc, store) = service(Some(Role::Editor));
    let id = store.add_document(store.root_id, "n.md", "hello\n");
    let err = svc
        .patch_document(
            actor(),
            store.workspace_id,
            PatchDocument {
                node_id: id,
                edits: vec![Edit {
                    old_text: "missing".to_owned(),
                    new_text: "x".to_owned(),
                }],
                expected_sha256: None,
            },
        )
        .await
        .unwrap_err();
    assert!(matches!(err, ServiceError::Conflict(_)));
}

#[tokio::test]
async fn patch_empty_edits_is_invalid_input() {
    let (svc, store) = service(Some(Role::Editor));
    let id = store.add_document(store.root_id, "n.md", "hello\n");
    let err = svc
        .patch_document(
            actor(),
            store.workspace_id,
            PatchDocument {
                node_id: id,
                edits: Vec::new(),
                expected_sha256: None,
            },
        )
        .await
        .unwrap_err();
    assert!(matches!(err, ServiceError::InvalidInput(_)));
}

#[tokio::test]
async fn patch_without_user_sha_still_guards_loaded_version() {
    let (svc, store) = service(Some(Role::Editor));
    let id = store.add_document(store.root_id, "n.md", "hello world\n");
    store.mutate_before_next_save("concurrent change\n");

    let err = svc
        .patch_document(
            actor(),
            store.workspace_id,
            PatchDocument {
                node_id: id,
                edits: vec![Edit {
                    old_text: "world".to_owned(),
                    new_text: "there".to_owned(),
                }],
                expected_sha256: None,
            },
        )
        .await
        .unwrap_err();

    assert!(matches!(err, ServiceError::Conflict(_)));
}

#[tokio::test]
async fn patch_expected_sha_checked_before_matching() {
    let (svc, store) = service(Some(Role::Editor));
    let id = store.add_document(store.root_id, "n.md", "hello\n");
    let err = svc
        .patch_document(
            actor(),
            store.workspace_id,
            PatchDocument {
                node_id: id,
                edits: vec![Edit {
                    old_text: "hello".to_owned(),
                    new_text: "hi".to_owned(),
                }],
                expected_sha256: Some("stale".to_owned()),
            },
        )
        .await
        .unwrap_err();
    assert!(matches!(err, ServiceError::Conflict(_)));
}

// --- mv ---

#[tokio::test]
async fn mv_root_is_forbidden() {
    let (svc, store) = service(Some(Role::Editor));
    let dest = store.add_folder(store.root_id, "dest");
    let err = svc
        .move_node(
            actor(),
            store.workspace_id,
            MoveNode {
                node_id: store.root_id,
                new_parent_node_id: dest,
                new_name: None,
                expected_parent_id: None,
            },
        )
        .await
        .unwrap_err();
    assert!(matches!(err, ServiceError::Conflict(_)));
}

#[tokio::test]
async fn mv_into_self_is_conflict() {
    let (svc, store) = service(Some(Role::Editor));
    let folder = store.add_folder(store.root_id, "folder");
    let err = svc
        .move_node(
            actor(),
            store.workspace_id,
            MoveNode {
                node_id: folder,
                new_parent_node_id: folder,
                new_name: None,
                expected_parent_id: None,
            },
        )
        .await
        .unwrap_err();
    assert!(matches!(err, ServiceError::Conflict(_)));
}

#[tokio::test]
async fn mv_into_descendant_is_conflict() {
    let (svc, store) = service(Some(Role::Editor));
    let parent = store.add_folder(store.root_id, "parent");
    let child = store.add_folder(parent, "child");
    let err = svc
        .move_node(
            actor(),
            store.workspace_id,
            MoveNode {
                node_id: parent,
                new_parent_node_id: child,
                new_name: None,
                expected_parent_id: None,
            },
        )
        .await
        .unwrap_err();
    assert!(matches!(err, ServiceError::Conflict(_)));
}

#[tokio::test]
async fn mv_destination_name_conflict_is_conflict() {
    let (svc, store) = service(Some(Role::Editor));
    let src_parent = store.add_folder(store.root_id, "src");
    let moving = store.add_document(src_parent, "note.md", "x");
    let dest = store.add_folder(store.root_id, "dest");
    store.add_document(dest, "note.md", "y");
    let err = svc
        .move_node(
            actor(),
            store.workspace_id,
            MoveNode {
                node_id: moving,
                new_parent_node_id: dest,
                new_name: None,
                expected_parent_id: None,
            },
        )
        .await
        .unwrap_err();
    assert!(matches!(err, ServiceError::Conflict(_)));
}

#[tokio::test]
async fn mv_expected_parent_mismatch_is_conflict() {
    let (svc, store) = service(Some(Role::Editor));
    let old_parent = store.add_folder(store.root_id, "old");
    let actual_parent = store.add_folder(store.root_id, "actual");
    let dest = store.add_folder(store.root_id, "dest");
    let doc = store.add_document(actual_parent, "doc.md", "x\n");

    let err = svc
        .move_node(
            actor(),
            store.workspace_id,
            MoveNode {
                node_id: doc,
                new_parent_node_id: dest,
                new_name: None,
                expected_parent_id: Some(old_parent),
            },
        )
        .await
        .unwrap_err();

    assert!(matches!(err, ServiceError::Conflict(_)));
}

#[tokio::test]
async fn mv_same_path_is_noop_success() {
    let (svc, store) = service(Some(Role::Editor));
    let folder = store.add_folder(store.root_id, "folder");
    let doc = store.add_document(folder, "note.md", "x");
    let view = svc
        .move_node(
            actor(),
            store.workspace_id,
            MoveNode {
                node_id: doc,
                new_parent_node_id: folder,
                new_name: Some("note.md".to_owned()),
                expected_parent_id: None,
            },
        )
        .await
        .unwrap();
    assert_eq!(view.path, "/folder/note.md");
}

#[tokio::test]
async fn mv_into_full_destination_is_conflict() {
    let limits = Limits {
        folder_max_children: 3,
        ..Limits::default()
    };
    let (svc, store) = service_with_limits(Some(Role::Editor), limits);
    let src_parent = store.add_folder(store.root_id, "src");
    let moving = store.add_document(src_parent, "note.md", "x");
    let dest = store.add_folder(store.root_id, "dest");
    // Fill the destination to the fanout cap.
    for i in 0..limits.folder_max_children {
        store.add_document(dest, &format!("f{i}.md"), "y");
    }
    let err = svc
        .move_node(
            actor(),
            store.workspace_id,
            MoveNode {
                node_id: moving,
                new_parent_node_id: dest,
                new_name: None,
                expected_parent_id: None,
            },
        )
        .await
        .unwrap_err();
    assert!(matches!(err, ServiceError::Conflict(_)));
}

#[tokio::test]
async fn mv_succeeds_and_derives_new_path() {
    let (svc, store) = service(Some(Role::Editor));
    let src = store.add_folder(store.root_id, "src");
    let doc = store.add_document(src, "note.md", "x");
    let dest = store.add_folder(store.root_id, "dest");
    let view = svc
        .move_node(
            actor(),
            store.workspace_id,
            MoveNode {
                node_id: doc,
                new_parent_node_id: dest,
                new_name: None,
                expected_parent_id: None,
            },
        )
        .await
        .unwrap();
    assert_eq!(view.path, "/dest/note.md");
}

// --- rm ---

#[tokio::test]
async fn rm_root_is_forbidden() {
    let (svc, store) = service(Some(Role::Editor));
    let err = svc
        .delete_node(
            actor(),
            store.workspace_id,
            DeleteNode {
                node_id: store.root_id,
                recursive: false,
            },
        )
        .await
        .unwrap_err();
    assert!(matches!(err, ServiceError::Conflict(_)));
}

#[tokio::test]
async fn rm_folder_without_recursive_is_conflict() {
    let (svc, store) = service(Some(Role::Editor));
    let folder = store.add_folder(store.root_id, "folder");
    let err = svc
        .delete_node(
            actor(),
            store.workspace_id,
            DeleteNode {
                node_id: folder,
                recursive: false,
            },
        )
        .await
        .unwrap_err();
    assert!(matches!(err, ServiceError::Conflict(_)));
}

#[tokio::test]
async fn rm_document_succeeds_without_recursive() {
    let (svc, store) = service(Some(Role::Editor));
    let doc = store.add_document(store.root_id, "n.md", "x");
    let result = svc
        .delete_node(
            actor(),
            store.workspace_id,
            DeleteNode {
                node_id: doc,
                recursive: false,
            },
        )
        .await
        .unwrap();
    assert_eq!(result.node_id, doc);
    assert_eq!(result.path, "/n.md");
    assert!(result.purge_after > Utc::now());
    assert!(
        store
            .find_node(store.workspace_id, doc)
            .await
            .unwrap()
            .is_none()
    );
}

// --- ls pagination ---

#[tokio::test]
async fn children_paginate_with_cursor() {
    let (svc, store) = service(Some(Role::Viewer));
    for i in 0..5 {
        store.add_document(store.root_id, &format!("f{i}.md"), "x");
    }
    let page1 = svc
        .children(
            actor(),
            store.workspace_id,
            store.root_id,
            ChildrenRequest {
                limit: Some(2),
                cursor: None,
            },
        )
        .await
        .unwrap();
    assert_eq!(page1.items.len(), 2);
    assert!(page1.has_more);
    let cursor = page1.next_cursor.clone();
    assert!(cursor.is_some());

    let page2 = svc
        .children(
            actor(),
            store.workspace_id,
            store.root_id,
            ChildrenRequest {
                limit: Some(2),
                cursor,
            },
        )
        .await
        .unwrap();
    assert_eq!(page2.items.len(), 2);
    // No overlap between page 1 and page 2.
    let ids1: Vec<Uuid> = page1.items.iter().map(|n| n.node.id).collect();
    let ids2: Vec<Uuid> = page2.items.iter().map(|n| n.node.id).collect();
    assert!(ids1.iter().all(|id| !ids2.contains(id)));
}

// --- resolve_path ---

#[tokio::test]
async fn resolve_path_root_returns_root() {
    let (svc, store) = service(Some(Role::Viewer));
    let view = svc
        .resolve_path(actor(), store.workspace_id, "/")
        .await
        .unwrap();
    assert_eq!(view.node.id, store.root_id);
    assert_eq!(view.path, "/");
}

#[tokio::test]
async fn resolve_path_nested_returns_node() {
    let (svc, store) = service(Some(Role::Viewer));
    let folder = store.add_folder(store.root_id, "projects");
    let doc = store.add_document(folder, "note.md", "x");
    let view = svc
        .resolve_path(actor(), store.workspace_id, "/projects/note.md")
        .await
        .unwrap();
    assert_eq!(view.node.id, doc);
    assert_eq!(view.path, "/projects/note.md");
}

#[tokio::test]
async fn resolve_path_missing_is_not_found() {
    let (svc, store) = service(Some(Role::Viewer));
    let err = svc
        .resolve_path(actor(), store.workspace_id, "/nope/missing.md")
        .await
        .unwrap_err();
    assert!(matches!(err, ServiceError::NotFound(_)));
}

#[tokio::test]
async fn resolve_path_requires_role() {
    let (svc, store) = service(None);
    let err = svc
        .resolve_path(actor(), store.workspace_id, "/")
        .await
        .unwrap_err();
    assert!(matches!(err, ServiceError::NotFound(_)));
}

// --- update_node (rename / reorder) ---

#[tokio::test]
async fn update_node_renames_and_derives_path() {
    let (svc, store) = service(Some(Role::Editor));
    let folder = store.add_folder(store.root_id, "old");
    let view = svc
        .update_node(
            actor(),
            store.workspace_id,
            folder,
            Some("new".to_owned()),
            None,
        )
        .await
        .unwrap();
    assert_eq!(view.node.name, "new");
    assert_eq!(view.path, "/new");
}

#[tokio::test]
async fn update_node_sets_sort_order() {
    let (svc, store) = service(Some(Role::Editor));
    let folder = store.add_folder(store.root_id, "f");
    let view = svc
        .update_node(actor(), store.workspace_id, folder, None, Some(10))
        .await
        .unwrap();
    assert_eq!(view.node.sort_order, 10);
}

#[tokio::test]
async fn update_node_rejects_root_rename() {
    let (svc, store) = service(Some(Role::Editor));
    let err = svc
        .update_node(
            actor(),
            store.workspace_id,
            store.root_id,
            Some("renamed".to_owned()),
            None,
        )
        .await
        .unwrap_err();
    assert!(matches!(err, ServiceError::Conflict(_)));
}

#[tokio::test]
async fn update_node_rejects_sibling_name_conflict() {
    let (svc, store) = service(Some(Role::Editor));
    store.add_folder(store.root_id, "taken");
    let other = store.add_folder(store.root_id, "other");
    let err = svc
        .update_node(
            actor(),
            store.workspace_id,
            other,
            Some("taken".to_owned()),
            None,
        )
        .await
        .unwrap_err();
    assert!(matches!(err, ServiceError::Conflict(_)));
}

#[tokio::test]
async fn update_node_requires_a_field() {
    let (svc, store) = service(Some(Role::Editor));
    let folder = store.add_folder(store.root_id, "f");
    let err = svc
        .update_node(actor(), store.workspace_id, folder, None, None)
        .await
        .unwrap_err();
    assert!(matches!(err, ServiceError::InvalidInput(_)));
}

#[tokio::test]
async fn update_node_requires_editor() {
    let (svc, store) = service(Some(Role::Viewer));
    let folder = store.add_folder(store.root_id, "f");
    let err = svc
        .update_node(
            actor(),
            store.workspace_id,
            folder,
            Some("g".to_owned()),
            None,
        )
        .await
        .unwrap_err();
    assert!(matches!(err, ServiceError::Forbidden(_)));
}
