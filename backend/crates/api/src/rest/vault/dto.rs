use chrono::{DateTime, Utc};
use notegate_db::{Children, Document, DocumentBundle, GrepMatch, Node};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Deserialize)]
pub(super) struct ResolveQuery {
    pub(super) path: String,
}

#[derive(Debug, Deserialize)]
pub(super) struct CreateNodeRequest {
    pub(super) parent_node_id: Uuid,
    pub(super) name: String,
}

#[derive(Debug, Deserialize)]
pub(super) struct SaveDocumentRequest {
    pub(super) content_md: String,
}

#[derive(Debug, Deserialize)]
pub(super) struct MoveNodeRequest {
    pub(super) new_parent_node_id: Uuid,
    pub(super) new_name: Option<String>,
}

#[derive(Debug, Deserialize)]
pub(super) struct FindRequest {
    pub(super) q: String,
    pub(super) path: Option<String>,
    pub(super) kind: Option<String>,
    pub(super) limit: Option<i64>,
}

#[derive(Debug, Deserialize)]
pub(super) struct GrepRequest {
    pub(super) q: String,
    pub(super) path: Option<String>,
    pub(super) context: Option<i64>,
    pub(super) limit: Option<i64>,
}

#[derive(Debug, Serialize)]
pub(super) struct NodeResponseBody {
    pub(super) node: NodeOutput,
}

#[derive(Debug, Serialize)]
pub(super) struct ChildrenResponse {
    parent: ParentOutput,
    children: Vec<NodeOutput>,
}

impl From<Children> for ChildrenResponse {
    fn from(value: Children) -> Self {
        Self {
            parent: ParentOutput {
                id: value.parent.id,
                path: value.parent.path,
            },
            children: value.children.into_iter().map(NodeOutput::from).collect(),
        }
    }
}

#[derive(Debug, Serialize)]
pub(super) struct DocumentResponse {
    node: NodeOutput,
    document: DocumentOutput,
}

impl From<DocumentBundle> for DocumentResponse {
    fn from(value: DocumentBundle) -> Self {
        Self {
            node: NodeOutput::from(value.node),
            document: DocumentOutput::from(value.document),
        }
    }
}

#[derive(Debug, Serialize)]
pub(super) struct FindResponse {
    pub(super) results: Vec<NodeOutput>,
}

#[derive(Debug, Serialize)]
pub(super) struct GrepResponse {
    pub(super) results: Vec<GrepMatchOutput>,
}

#[derive(Debug, Serialize)]
struct ParentOutput {
    id: Uuid,
    path: String,
}

#[derive(Debug, Serialize)]
pub(super) struct NodeOutput {
    id: Uuid,
    parent_id: Option<Uuid>,
    name: String,
    kind: &'static str,
    path: String,
    sort_order: i32,
    has_children: bool,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

impl From<Node> for NodeOutput {
    fn from(value: Node) -> Self {
        Self {
            id: value.id,
            parent_id: value.parent_id,
            name: value.name,
            kind: value.kind.as_str(),
            path: value.path,
            sort_order: value.sort_order,
            has_children: value.has_children,
            created_at: value.created_at,
            updated_at: value.updated_at,
        }
    }
}

#[derive(Debug, Serialize)]
struct DocumentOutput {
    node_id: Uuid,
    content_md: String,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

impl From<Document> for DocumentOutput {
    fn from(value: Document) -> Self {
        Self {
            node_id: value.node_id,
            content_md: value.content_md,
            created_at: value.created_at,
            updated_at: value.updated_at,
        }
    }
}

#[derive(Debug, Serialize)]
pub(super) struct GrepMatchOutput {
    node_id: Uuid,
    path: String,
    line_no: i64,
    line: String,
    before: Vec<String>,
    after: Vec<String>,
}

impl From<GrepMatch> for GrepMatchOutput {
    fn from(value: GrepMatch) -> Self {
        Self {
            node_id: value.node_id,
            path: value.path,
            line_no: value.line_no,
            line: value.line,
            before: value.before,
            after: value.after,
        }
    }
}
