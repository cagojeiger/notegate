use base64::Engine as _;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use chrono::{DateTime, Utc};
use notegate_domain::files::{
    ChildrenCursor, ChildrenPage, Document, DocumentBundle, GrepMatch, Node,
};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::error::ApiError;

const OPEN_DEFAULT_MAX_LINES: i64 = 200;
const OPEN_MAX_LINES: i64 = 1000;
const OPEN_DEFAULT_MAX_BYTES: usize = 65_536;
const OPEN_MAX_BYTES: usize = 262_144;

#[derive(Debug, Deserialize)]
pub(super) struct ResolveQuery {
    pub(super) path: String,
}

#[derive(Debug, Deserialize)]
pub(super) struct ChildrenQuery {
    pub(super) limit: Option<i64>,
    pub(super) cursor: Option<String>,
}

#[derive(Debug, Deserialize)]
pub(super) struct OpenDocumentQuery {
    pub(super) start_line: Option<i64>,
    pub(super) max_lines: Option<i64>,
    pub(super) max_bytes: Option<usize>,
}

#[derive(Debug, Deserialize)]
pub(super) struct DeleteNodeQuery {
    pub(super) recursive: Option<bool>,
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
    pub(super) cursor: Option<String>,
}

#[derive(Debug, Deserialize)]
pub(super) struct GrepRequest {
    pub(super) q: String,
    pub(super) path: Option<String>,
    pub(super) context: Option<i64>,
    pub(super) limit: Option<i64>,
    pub(super) cursor: Option<String>,
}

#[derive(Debug, Serialize)]
pub(super) struct NodeResponseBody {
    pub(super) node: NodeOutput,
}

#[derive(Debug, Serialize)]
pub(super) struct ChildrenResponse {
    parent: ParentOutput,
    children: Vec<NodeOutput>,
    page: PageOutput,
}

impl ChildrenResponse {
    pub(super) fn try_from_page(value: ChildrenPage) -> Result<Self, ApiError> {
        let next_cursor = value
            .page
            .next_cursor
            .as_ref()
            .map(encode_cursor)
            .transpose()?;
        let children = value
            .page
            .items
            .into_iter()
            .map(NodeOutput::from)
            .collect::<Vec<_>>();
        Ok(Self {
            parent: ParentOutput {
                id: value.parent.id,
                path: value.parent.path,
            },
            page: PageOutput {
                limit: value.page.limit,
                returned: children.len(),
                has_more: value.page.has_more,
                next_cursor,
            },
            children,
        })
    }
}

#[derive(Debug, Serialize)]
pub(super) struct DocumentResponse {
    node: NodeOutput,
    document: DocumentOutput,
}

impl DocumentResponse {
    pub(super) fn from_bundle(value: DocumentBundle) -> Self {
        Self {
            node: NodeOutput::from(value.node),
            document: DocumentOutput::from_document(value.document),
        }
    }

    pub(super) fn from_bundle_range(value: DocumentBundle, query: OpenDocumentQuery) -> Self {
        Self {
            node: NodeOutput::from(value.node),
            document: DocumentOutput::from_document_range(value.document, query),
        }
    }
}

#[derive(Debug, Serialize)]
pub(super) struct FindResponse {
    pub(super) results: Vec<NodeOutput>,
    pub(super) page: PageOutput,
}

#[derive(Debug, Serialize)]
pub(super) struct GrepResponse {
    pub(super) results: Vec<GrepMatchOutput>,
    pub(super) page: PageOutput,
}

#[derive(Debug, Serialize)]
pub(super) struct PageOutput {
    limit: i64,
    returned: usize,
    has_more: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    next_cursor: Option<String>,
}

impl PageOutput {
    pub(super) fn terminal(limit: i64, returned: usize) -> Self {
        Self {
            limit,
            returned,
            has_more: false,
            next_cursor: None,
        }
    }
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
    content_sha256: String,
    byte_len: i32,
    line_count: i32,
    start_line: i64,
    end_line: i64,
    truncated: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    next_start_line: Option<i64>,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

impl DocumentOutput {
    fn from_document(value: Document) -> Self {
        let end_line = i64::from(value.line_count);
        Self {
            node_id: value.node_id,
            content_md: value.content_md,
            content_sha256: value.content_sha256,
            byte_len: value.byte_len,
            line_count: value.line_count,
            start_line: 1,
            end_line,
            truncated: false,
            next_start_line: None,
            created_at: value.created_at,
            updated_at: value.updated_at,
        }
    }

    fn from_document_range(value: Document, query: OpenDocumentQuery) -> Self {
        let range = slice_document(&value.content_md, query);
        Self {
            node_id: value.node_id,
            content_md: range.content_md,
            content_sha256: value.content_sha256,
            byte_len: value.byte_len,
            line_count: value.line_count,
            start_line: range.start_line,
            end_line: range.end_line,
            truncated: range.truncated,
            next_start_line: range.next_start_line,
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

#[derive(Debug, Deserialize, Serialize)]
struct CursorToken {
    sort_order: i32,
    name: String,
    id: Uuid,
}

pub(super) fn decode_cursor(value: Option<String>) -> Result<Option<ChildrenCursor>, ApiError> {
    value
        .map(|value| {
            let bytes = URL_SAFE_NO_PAD
                .decode(value)
                .map_err(|_error| ApiError::invalid_field("invalid cursor"))?;
            let token: CursorToken = serde_json::from_slice(&bytes)
                .map_err(|_error| ApiError::invalid_field("invalid cursor"))?;
            Ok(ChildrenCursor {
                sort_order: token.sort_order,
                name: token.name,
                id: token.id,
            })
        })
        .transpose()
}

fn encode_cursor(value: &ChildrenCursor) -> Result<String, ApiError> {
    let bytes = serde_json::to_vec(&CursorToken {
        sort_order: value.sort_order,
        name: value.name.clone(),
        id: value.id,
    })
    .map_err(|_error| ApiError::internal("failed to encode cursor"))?;
    Ok(URL_SAFE_NO_PAD.encode(bytes))
}

struct DocumentSlice {
    content_md: String,
    start_line: i64,
    end_line: i64,
    truncated: bool,
    next_start_line: Option<i64>,
}

fn slice_document(content: &str, query: OpenDocumentQuery) -> DocumentSlice {
    let start_line = query.start_line.unwrap_or(1).max(1);
    let max_lines = query
        .max_lines
        .unwrap_or(OPEN_DEFAULT_MAX_LINES)
        .clamp(1, OPEN_MAX_LINES);
    let max_bytes = query
        .max_bytes
        .unwrap_or(OPEN_DEFAULT_MAX_BYTES)
        .clamp(1, OPEN_MAX_BYTES);

    let mut output = String::new();
    let mut end_line = start_line.saturating_sub(1);
    let mut truncated = false;
    let mut next_start_line = None;

    for (idx, line) in content.split_inclusive('\n').enumerate() {
        let line_no = idx as i64 + 1;
        if line_no < start_line {
            continue;
        }
        if line_no >= start_line + max_lines {
            truncated = true;
            next_start_line = Some(line_no);
            break;
        }
        if output.len() + line.len() > max_bytes {
            truncated = true;
            next_start_line = Some(line_no);
            break;
        }
        output.push_str(line);
        end_line = line_no;
    }

    DocumentSlice {
        content_md: output,
        start_line,
        end_line,
        truncated,
        next_start_line,
    }
}
