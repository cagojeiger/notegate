use uuid::Uuid;

const DOCUMENT_MAX_BYTES: usize = 2 * 1024 * 1024;

use super::validation::{
    clamp_limit, normalize_path, validate_document_name, validate_folder_name, validate_node_name,
};
use super::{
    Children, ChildrenPage, ChildrenRequest, CreateDocument, CreateFolder, DocumentBundle,
    FilesError, FilesResult, FilesStore, FindQuery, FindRequest, GrepCandidateQuery, GrepMatch,
    GrepRequest, MoveNode, Node, NodeKind, SaveDocument,
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
        self.store.initialize_root_node(user_id).await
    }

    pub async fn resolve(&self, user_id: Uuid, path: &str) -> FilesResult<Node> {
        let path = normalize_path(path)?;
        self.store.resolve_node(user_id, path).await
    }

    pub async fn children(&self, user_id: Uuid, node_id: Uuid) -> FilesResult<Children> {
        self.store.child_nodes(user_id, node_id).await
    }

    pub async fn children_page(
        &self,
        user_id: Uuid,
        node_id: Uuid,
        request: ChildrenRequest,
    ) -> FilesResult<ChildrenPage> {
        self.store
            .paged_child_nodes(user_id, node_id, request)
            .await
    }

    pub async fn create_folder(&self, user_id: Uuid, command: CreateFolder) -> FilesResult<Node> {
        validate_folder_name(&command.name)?;
        self.store.create_folder(user_id, command).await
    }

    pub async fn create_document(
        &self,
        user_id: Uuid,
        command: CreateDocument,
    ) -> FilesResult<DocumentBundle> {
        validate_document_name(&command.name)?;
        self.store.create_document(user_id, command).await
    }

    pub async fn document(&self, user_id: Uuid, node_id: Uuid) -> FilesResult<DocumentBundle> {
        self.store.document(user_id, node_id).await
    }

    pub async fn save_document(
        &self,
        user_id: Uuid,
        command: SaveDocument,
    ) -> FilesResult<DocumentBundle> {
        if command.content_md.len() > DOCUMENT_MAX_BYTES {
            return Err(FilesError::InvalidInput("document is too large".into()));
        }
        self.store.save_document(user_id, command).await
    }

    pub async fn move_node(&self, user_id: Uuid, command: MoveNode) -> FilesResult<Node> {
        if let Some(name) = command.new_name.as_deref() {
            validate_node_name(name)?;
        }
        self.store.move_node(user_id, command).await
    }

    pub async fn delete_node(&self, user_id: Uuid, node_id: Uuid) -> FilesResult<()> {
        self.store.delete_node(user_id, node_id).await
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
        let query = FindQuery {
            q: q.to_owned(),
            path: request.path.as_deref().map(normalize_path).transpose()?,
            kind,
            limit: clamp_limit(request.limit),
        };

        self.store.find_nodes(user_id, query).await
    }

    pub async fn grep(&self, user_id: Uuid, request: GrepRequest) -> FilesResult<Vec<GrepMatch>> {
        let q = request.q.trim();
        if q.is_empty() {
            return Err(FilesError::InvalidInput("query cannot be empty".into()));
        }

        let limit = clamp_limit(request.limit) as usize;
        let context = request.context.unwrap_or(0).clamp(0, 5) as usize;
        let query = GrepCandidateQuery {
            q: q.to_owned(),
            path: request.path.as_deref().map(normalize_path).transpose()?,
            limit: limit as i64,
        };
        let candidates = self.store.grep_candidates(user_id, query).await?;

        let needle = q.to_lowercase();
        let mut matches = Vec::new();
        for candidate in candidates {
            let lines: Vec<&str> = candidate.content_md.split('\n').collect();
            for (idx, line) in lines.iter().enumerate() {
                if !line.to_lowercase().contains(&needle) {
                    continue;
                }

                let before_start = idx.saturating_sub(context);
                let before = lines
                    .get(before_start..idx)
                    .unwrap_or(&[])
                    .iter()
                    .map(|line| (*line).to_owned())
                    .collect();
                let after_end = (idx + 1 + context).min(lines.len());
                let after = lines
                    .get(idx + 1..after_end)
                    .unwrap_or(&[])
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
}
