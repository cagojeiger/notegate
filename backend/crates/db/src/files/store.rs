use uuid::Uuid;

use super::FilesRepo;
use notegate_domain::files::{
    DocumentBundle, FilesResult, FilesStore, FindQuery, GrepCandidate, GrepCandidateQuery, Node,
};

impl FilesStore for FilesRepo {
    async fn initialize_default_workspace(&self, user_id: Uuid) -> FilesResult<Uuid> {
        self.initialize_default_workspace(user_id).await
    }

    async fn default_workspace_id(&self, user_id: Uuid) -> FilesResult<Uuid> {
        self.default_workspace_id(user_id).await
    }

    async fn root_for_workspace(&self, workspace_id: Uuid) -> FilesResult<Node> {
        self.root_for_workspace(workspace_id).await
    }

    async fn node_by_id(&self, workspace_id: Uuid, node_id: Uuid) -> FilesResult<Node> {
        self.node_by_id(workspace_id, node_id).await
    }

    async fn node_by_path(&self, workspace_id: Uuid, path: &str) -> FilesResult<Node> {
        self.node_by_path(workspace_id, path).await
    }

    async fn child_nodes(
        &self,
        workspace_id: Uuid,
        parent_node_id: Uuid,
    ) -> FilesResult<Vec<Node>> {
        self.child_nodes(workspace_id, parent_node_id).await
    }

    async fn create_folder_node(
        &self,
        workspace_id: Uuid,
        parent_node_id: Uuid,
        name: &str,
        path: &str,
    ) -> FilesResult<Node> {
        self.create_folder_node(workspace_id, parent_node_id, name, path)
            .await
    }

    async fn create_document_node(
        &self,
        workspace_id: Uuid,
        parent_node_id: Uuid,
        name: &str,
        path: &str,
    ) -> FilesResult<DocumentBundle> {
        self.create_document_node(workspace_id, parent_node_id, name, path)
            .await
    }

    async fn document_by_node_id(
        &self,
        workspace_id: Uuid,
        node_id: Uuid,
    ) -> FilesResult<DocumentBundle> {
        self.document_by_node_id(workspace_id, node_id).await
    }

    async fn save_document_content(
        &self,
        workspace_id: Uuid,
        node_id: Uuid,
        content_md: &str,
    ) -> FilesResult<()> {
        self.save_document_content(workspace_id, node_id, content_md)
            .await
    }

    async fn move_node_record(
        &self,
        workspace_id: Uuid,
        node_id: Uuid,
        new_parent_node_id: Uuid,
        new_name: &str,
        old_path: &str,
        new_path: &str,
    ) -> FilesResult<()> {
        self.move_node_record(
            workspace_id,
            node_id,
            new_parent_node_id,
            new_name,
            old_path,
            new_path,
        )
        .await
    }

    async fn soft_delete_subtree(&self, workspace_id: Uuid, node_id: Uuid) -> FilesResult<()> {
        self.soft_delete_subtree(workspace_id, node_id).await
    }

    async fn find_nodes(&self, workspace_id: Uuid, query: FindQuery) -> FilesResult<Vec<Node>> {
        self.find_nodes(workspace_id, query).await
    }

    async fn grep_candidates(
        &self,
        workspace_id: Uuid,
        query: GrepCandidateQuery,
    ) -> FilesResult<Vec<GrepCandidate>> {
        self.grep_candidates(workspace_id, query).await
    }
}
