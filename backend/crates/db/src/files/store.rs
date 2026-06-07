use uuid::Uuid;

use super::FilesRepo;
use notegate_domain::files::{
    Children, ChildrenPage, ChildrenRequest, CreateDocument, CreateFolder, DocumentBundle,
    FilesResult, FilesStore, FindQuery, GrepCandidate, GrepCandidateQuery, MoveNode, Node,
    SaveDocument,
};

impl FilesStore for FilesRepo {
    async fn initialize_root_node(&self, user_id: Uuid) -> FilesResult<Node> {
        FilesRepo::initialize_root_node(self, user_id).await
    }

    async fn resolve_node(&self, user_id: Uuid, path: String) -> FilesResult<Node> {
        FilesRepo::resolve_node(self, user_id, path).await
    }

    async fn child_nodes(&self, user_id: Uuid, parent_node_id: Uuid) -> FilesResult<Children> {
        FilesRepo::child_nodes(self, user_id, parent_node_id).await
    }

    async fn paged_child_nodes(
        &self,
        user_id: Uuid,
        parent_node_id: Uuid,
        request: ChildrenRequest,
    ) -> FilesResult<ChildrenPage> {
        FilesRepo::paged_child_nodes(self, user_id, parent_node_id, request).await
    }

    async fn create_folder(&self, user_id: Uuid, command: CreateFolder) -> FilesResult<Node> {
        self.create_folder_atomic(user_id, command).await
    }

    async fn create_document(
        &self,
        user_id: Uuid,
        command: CreateDocument,
    ) -> FilesResult<DocumentBundle> {
        self.create_document_atomic(user_id, command).await
    }

    async fn document(&self, user_id: Uuid, node_id: Uuid) -> FilesResult<DocumentBundle> {
        self.document_by_node_id(user_id, node_id).await
    }

    async fn save_document(
        &self,
        user_id: Uuid,
        command: SaveDocument,
    ) -> FilesResult<DocumentBundle> {
        self.save_document_atomic(user_id, command).await
    }

    async fn move_node(&self, user_id: Uuid, command: MoveNode) -> FilesResult<Node> {
        self.move_node_atomic(user_id, command).await
    }

    async fn delete_node(&self, user_id: Uuid, node_id: Uuid) -> FilesResult<()> {
        self.delete_node_atomic(user_id, node_id).await
    }

    async fn find_nodes(&self, user_id: Uuid, query: FindQuery) -> FilesResult<Vec<Node>> {
        FilesRepo::find_nodes(self, user_id, query).await
    }

    async fn grep_candidates(
        &self,
        user_id: Uuid,
        query: GrepCandidateQuery,
    ) -> FilesResult<Vec<GrepCandidate>> {
        FilesRepo::grep_candidates(self, user_id, query).await
    }
}
