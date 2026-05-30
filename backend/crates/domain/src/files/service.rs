use uuid::Uuid;

use super::validation::{
    child_path, clamp_limit, normalize_path, validate_document_name, validate_folder_name,
};
use super::{
    Children, CreateDocument, CreateFolder, DocumentBundle, FilesError, FilesResult, FilesStore,
    FindQuery, FindRequest, GrepCandidateQuery, GrepMatch, GrepRequest, MoveNode, Node, NodeKind,
    SaveDocument,
};

#[derive(Debug, Clone)]
pub struct FilesService<S> {
    store: S,
}

impl<S> FilesService<S>
where
    S: FilesStore,
{
    pub fn new(store: S) -> Self {
        Self { store }
    }

    pub async fn root(&self, user_id: Uuid) -> FilesResult<Node> {
        let workspace_id = self.store.initialize_default_workspace(user_id).await?;
        self.store.root_for_workspace(workspace_id).await
    }

    pub async fn resolve(&self, user_id: Uuid, path: &str) -> FilesResult<Node> {
        let workspace_id = self.store.default_workspace_id(user_id).await?;
        let path = normalize_path(path)?;
        self.store.node_by_path(workspace_id, &path).await
    }

    pub async fn children(&self, user_id: Uuid, node_id: Uuid) -> FilesResult<Children> {
        let workspace_id = self.store.default_workspace_id(user_id).await?;
        let parent = self.store.node_by_id(workspace_id, node_id).await?;
        if parent.kind != NodeKind::Folder {
            return Err(FilesError::InvalidInput("node is not a folder".into()));
        }

        let children = self.store.child_nodes(workspace_id, node_id).await?;
        Ok(Children { parent, children })
    }

    pub async fn create_folder(&self, user_id: Uuid, command: CreateFolder) -> FilesResult<Node> {
        validate_folder_name(&command.name)?;
        let workspace_id = self.store.default_workspace_id(user_id).await?;
        let parent = self
            .folder_node(
                workspace_id,
                command.parent_node_id,
                "parent is not a folder",
            )
            .await?;
        let path = child_path(&parent.path, &command.name);
        self.store
            .create_folder_node(workspace_id, command.parent_node_id, &command.name, &path)
            .await
    }

    pub async fn create_document(
        &self,
        user_id: Uuid,
        command: CreateDocument,
    ) -> FilesResult<DocumentBundle> {
        validate_document_name(&command.name)?;
        let workspace_id = self.store.default_workspace_id(user_id).await?;
        let parent = self
            .folder_node(
                workspace_id,
                command.parent_node_id,
                "parent is not a folder",
            )
            .await?;
        let path = child_path(&parent.path, &command.name);
        self.store
            .create_document_node(workspace_id, command.parent_node_id, &command.name, &path)
            .await
    }

    pub async fn document(&self, user_id: Uuid, node_id: Uuid) -> FilesResult<DocumentBundle> {
        let workspace_id = self.store.default_workspace_id(user_id).await?;
        self.store.document_by_node_id(workspace_id, node_id).await
    }

    pub async fn save_document(
        &self,
        user_id: Uuid,
        command: SaveDocument,
    ) -> FilesResult<DocumentBundle> {
        let workspace_id = self.store.default_workspace_id(user_id).await?;
        let node = self.store.node_by_id(workspace_id, command.node_id).await?;
        if node.kind != NodeKind::Document {
            return Err(FilesError::InvalidInput("node is not a document".into()));
        }

        self.store
            .save_document_content(workspace_id, command.node_id, &command.content_md)
            .await?;
        self.store
            .document_by_node_id(workspace_id, command.node_id)
            .await
    }

    pub async fn move_node(&self, user_id: Uuid, command: MoveNode) -> FilesResult<Node> {
        let workspace_id = self.store.default_workspace_id(user_id).await?;
        let node = self.store.node_by_id(workspace_id, command.node_id).await?;
        if node.parent_id.is_none() {
            return Err(FilesError::Conflict("root cannot be moved".into()));
        }

        let new_parent = self
            .folder_node(
                workspace_id,
                command.new_parent_node_id,
                "new parent is not a folder",
            )
            .await?;

        let final_name = command.new_name.as_deref().unwrap_or(&node.name);
        match node.kind {
            NodeKind::Folder => validate_folder_name(final_name)?,
            NodeKind::Document => validate_document_name(final_name)?,
        }

        if node.id == new_parent.id
            || new_parent.path == node.path
            || new_parent
                .path
                .starts_with(&format!("{}/", node.path.trim_end_matches('/')))
        {
            return Err(FilesError::Conflict(
                "node cannot move into itself or its descendant".into(),
            ));
        }

        let old_path = node.path.clone();
        let new_path = child_path(&new_parent.path, final_name);
        self.store
            .move_node_record(
                workspace_id,
                command.node_id,
                command.new_parent_node_id,
                final_name,
                &old_path,
                &new_path,
            )
            .await?;

        self.store.node_by_id(workspace_id, command.node_id).await
    }

    pub async fn delete_node(&self, user_id: Uuid, node_id: Uuid) -> FilesResult<()> {
        let workspace_id = self.store.default_workspace_id(user_id).await?;
        let node = self.store.node_by_id(workspace_id, node_id).await?;
        if node.parent_id.is_none() {
            return Err(FilesError::Conflict("root cannot be deleted".into()));
        }

        self.store.soft_delete_subtree(workspace_id, node_id).await
    }

    pub async fn find(&self, user_id: Uuid, request: FindRequest) -> FilesResult<Vec<Node>> {
        let q = request.q.trim();
        if q.is_empty() {
            return Err(FilesError::InvalidInput("query cannot be empty".into()));
        }

        let kind = request
            .kind
            .as_deref()
            .map(|kind| {
                NodeKind::parse(kind)
                    .ok_or_else(|| FilesError::InvalidInput("invalid node kind".into()))
            })
            .transpose()?;
        let workspace_id = self.store.default_workspace_id(user_id).await?;
        let query = FindQuery {
            q: q.to_owned(),
            path: request.path.as_deref().map(normalize_path).transpose()?,
            kind,
            limit: clamp_limit(request.limit),
        };

        self.store.find_nodes(workspace_id, query).await
    }

    pub async fn grep(&self, user_id: Uuid, request: GrepRequest) -> FilesResult<Vec<GrepMatch>> {
        let q = request.q.trim();
        if q.is_empty() {
            return Err(FilesError::InvalidInput("query cannot be empty".into()));
        }

        let workspace_id = self.store.default_workspace_id(user_id).await?;
        let limit = clamp_limit(request.limit) as usize;
        let context = request.context.unwrap_or(0).clamp(0, 5) as usize;
        let query = GrepCandidateQuery {
            q: q.to_owned(),
            path: request.path.as_deref().map(normalize_path).transpose()?,
            limit: limit as i64,
        };
        let candidates = self.store.grep_candidates(workspace_id, query).await?;

        let needle = q.to_lowercase();
        let mut matches = Vec::new();
        for candidate in candidates {
            let lines: Vec<&str> = candidate.content_md.split('\n').collect();
            for (idx, line) in lines.iter().enumerate() {
                if !line.to_lowercase().contains(&needle) {
                    continue;
                }

                let before_start = idx.saturating_sub(context);
                let before = lines[before_start..idx]
                    .iter()
                    .map(|line| (*line).to_owned())
                    .collect();
                let after_end = (idx + 1 + context).min(lines.len());
                let after = lines[idx + 1..after_end]
                    .iter()
                    .map(|line| (*line).to_owned())
                    .collect();

                matches.push(GrepMatch {
                    node_id: candidate.node_id,
                    path: candidate.path.clone(),
                    line_no: idx as i64 + 1,
                    line: (*line).to_owned(),
                    before,
                    after,
                });

                if matches.len() >= limit {
                    return Ok(matches);
                }
            }
        }

        Ok(matches)
    }

    async fn folder_node(
        &self,
        workspace_id: Uuid,
        node_id: Uuid,
        message: &str,
    ) -> FilesResult<Node> {
        let node = self.store.node_by_id(workspace_id, node_id).await?;
        if node.kind != NodeKind::Folder {
            return Err(FilesError::InvalidInput(message.into()));
        }
        Ok(node)
    }
}
