use uuid::Uuid;

#[derive(Debug, Clone)]
pub struct CreateFolder {
    pub parent_node_id: Uuid,
    pub name: String,
}

#[derive(Debug, Clone)]
pub struct CreateDocument {
    pub parent_node_id: Uuid,
    pub name: String,
}

#[derive(Debug, Clone)]
pub struct SaveDocument {
    pub node_id: Uuid,
    pub content_md: String,
}

#[derive(Debug, Clone)]
pub struct MoveNode {
    pub node_id: Uuid,
    pub new_parent_node_id: Uuid,
    pub new_name: Option<String>,
}

#[derive(Debug, Clone)]
pub struct FindRequest {
    pub q: String,
    pub path: Option<String>,
    pub kind: Option<String>,
    pub limit: Option<i64>,
}

#[derive(Debug, Clone)]
pub struct GrepRequest {
    pub q: String,
    pub path: Option<String>,
    pub context: Option<i64>,
    pub limit: Option<i64>,
}
