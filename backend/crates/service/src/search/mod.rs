//! Space search: `find` (node name metadata) and `grep` (content).
//!
//! The service owns authorization, limit clamping, opaque cursors, and search
//! result shaping. The two query implementations live in the [`find`] and [`grep`]
//! submodules; shared types, the permission gate, and query validation live here.

use notegate_core::limits;
use notegate_db::FilesRepo;
use notegate_model::files::{ChildrenCursor, NodeView, TextStats};
pub use notegate_model::search::{
    DfsFrame, FindMatchMode, FindPage, FindRequest, GrepLineMode, GrepMatchMode, GrepPage,
    GrepRequest, SearchCursor, TreeCursor, TreeFrame, TreePage, TreeRequest,
};
use notegate_model::{Node, NodeKind, Permission, TextObject};
use regex::{Regex, RegexBuilder};
use uuid::Uuid;

use crate::cursor;
use crate::error::{ServiceError, ServiceResult};
use crate::files::policy::{self, FileCommand};

mod find;
mod grep;
mod tree;

/// Search service. The `find`/`grep` query methods are implemented in the
/// [`find`] and [`grep`] submodules.
#[derive(Debug, Clone)]
pub struct SearchService {
    store: FilesRepo,
}

impl SearchService {
    pub fn new(store: FilesRepo) -> Self {
        Self { store }
    }

    async fn resolve_scope_folder(
        &self,
        space_id: Uuid,
        path: Option<&str>,
    ) -> ServiceResult<Uuid> {
        let normalized = match path {
            Some(path) => crate::files::validation::normalize_path(path)?,
            None => "/".to_owned(),
        };
        let node_id = self
            .store
            .resolve_scope(space_id, Some(&normalized))
            .await?
            .ok_or_else(|| ServiceError::NotFound("scope path not found".to_owned()))?;
        let node = self
            .store
            .find_node(space_id, node_id)
            .await?
            .ok_or_else(|| ServiceError::NotFound("scope path not found".to_owned()))?;
        if node.kind != NodeKind::Folder {
            return Err(ServiceError::InvalidInput(
                "search scope must be a folder".to_owned(),
            ));
        }
        Ok(node_id)
    }

    async fn node_view(&self, space_id: Uuid, node: Node, path: String) -> ServiceResult<NodeView> {
        let has_children = if node.kind == NodeKind::Folder {
            self.store.has_children(space_id, node.id).await?
        } else {
            false
        };
        let text = if node.kind == NodeKind::Text {
            self.store.text_stats(space_id, node.id).await?
        } else {
            None
        };
        let file = if node.kind == NodeKind::File {
            self.store.file_stats(space_id, node.id).await?
        } else {
            None
        };
        Ok(NodeView {
            node,
            path,
            has_children,
            text,
            file,
        })
    }

    fn text_node_view(&self, node: Node, path: String, text: &TextObject) -> NodeView {
        NodeView {
            node,
            path,
            has_children: false,
            text: Some(TextStats {
                content_sha256: text.content_sha256.clone(),
                byte_len: text.byte_len,
                line_count: text.line_count,
            }),
            file: None,
        }
    }

    fn decode_search_cursor(
        &self,
        raw: Option<&str>,
        command: &str,
        fingerprint: &str,
        scope_node_id: Uuid,
    ) -> ServiceResult<Vec<DfsFrame>> {
        match raw {
            None => Ok(vec![DfsFrame {
                folder_node_id: scope_node_id,
                after: None,
            }]),
            Some(raw) => {
                let cursor: SearchCursor = cursor::decode(raw)?;
                if cursor.version != 1
                    || cursor.command != command
                    || cursor.fingerprint != fingerprint
                {
                    return Err(ServiceError::InvalidInput(
                        "search cursor does not match this query".to_owned(),
                    ));
                }
                Ok(cursor.stack)
            }
        }
    }

    fn encode_search_cursor(
        &self,
        command: &str,
        fingerprint: String,
        stack: Vec<DfsFrame>,
    ) -> ServiceResult<Option<String>> {
        if stack.is_empty() {
            return Ok(None);
        }
        let cursor = SearchCursor {
            version: 1,
            command: command.to_owned(),
            fingerprint,
            stack,
        };
        cursor::encode(&cursor)
            .map(Some)
            .map_err(|_error| ServiceError::Internal("failed to encode cursor".to_owned()))
    }

    /// Resolve the caller's permission (none ⇒ `404`) and gate by command
    /// (insufficient permission ⇒ `403`). Mirrors the file service's authorization.
    async fn authorize(
        &self,
        space_id: Uuid,
        account_id: Uuid,
        command: FileCommand,
    ) -> ServiceResult<Permission> {
        let permission = self
            .store
            .permission_for(space_id, account_id)
            .await?
            .ok_or_else(|| ServiceError::NotFound("space not found".to_owned()))?;
        policy::require(permission, command)?;
        Ok(permission)
    }
}

/// Reject empty, multi-line, or very long search strings before they can become
/// broad or expensive search scans.
fn validate_query(q: &str) -> ServiceResult<&str> {
    let trimmed = q.trim();
    if trimmed.is_empty() {
        return Err(ServiceError::InvalidInput(
            "search query cannot be empty".to_owned(),
        ));
    }
    if trimmed.contains(['\n', '\r']) {
        return Err(ServiceError::InvalidInput(
            "search query must be a single line".to_owned(),
        ));
    }
    if trimmed.chars().count() > limits::SEARCH_QUERY_MAX_CHARS {
        return Err(ServiceError::InvalidInput(format!(
            "search query must be at most {} characters",
            limits::SEARCH_QUERY_MAX_CHARS
        )));
    }
    Ok(trimmed)
}

fn child_cursor(node: &Node) -> ChildrenCursor {
    ChildrenCursor {
        sort_order: node.sort_order,
        name: node.name.clone(),
        id: node.id,
    }
}

fn join_path(parent: &str, name: &str) -> String {
    if parent == "/" {
        format!("/{name}")
    } else {
        format!("{parent}/{name}")
    }
}

fn search_fingerprint(parts: &[String]) -> String {
    parts.join("\u{1f}")
}

enum NameMatcher {
    Contains(String),
    Regex(Regex),
    Glob(Regex),
}

impl NameMatcher {
    fn new(q: &str, mode: FindMatchMode) -> ServiceResult<Self> {
        match mode {
            FindMatchMode::Contains => Ok(Self::Contains(q.to_lowercase())),
            FindMatchMode::Regex => Ok(Self::Regex(compile_regex(q)?)),
            FindMatchMode::Glob => Ok(Self::Glob(compile_glob(q)?)),
        }
    }

    fn is_match(&self, value: &str) -> bool {
        match self {
            Self::Contains(needle) => value.to_lowercase().contains(needle),
            Self::Regex(regex) | Self::Glob(regex) => regex.is_match(value),
        }
    }
}

enum ContentMatcher {
    Literal(String),
    Regex(Regex),
}

impl ContentMatcher {
    fn new(q: &str, mode: GrepMatchMode) -> ServiceResult<Self> {
        match mode {
            GrepMatchMode::Literal => Ok(Self::Literal(q.to_lowercase())),
            GrepMatchMode::Regex => Ok(Self::Regex(compile_regex(q)?)),
        }
    }

    fn match_lines(&self, content: &str, mode: GrepLineMode) -> Vec<i32> {
        if content.is_empty() {
            return Vec::new();
        }

        let mut lines = Vec::new();
        for (index, line) in logical_lines(content).enumerate() {
            let matched = match self {
                Self::Literal(needle) => line.to_lowercase().contains(needle),
                Self::Regex(regex) => regex.is_match(line),
            };
            if !matched {
                continue;
            }

            let line_number = index as i32 + 1;
            match mode {
                GrepLineMode::None => return vec![line_number],
                GrepLineMode::First => return vec![line_number],
                GrepLineMode::All => lines.push(line_number),
            }
        }
        lines
    }
}

fn logical_lines(content: &str) -> impl Iterator<Item = &str> {
    let content = content.strip_suffix('\n').unwrap_or(content);
    content.split('\n')
}

struct PathFilters {
    include: Vec<Regex>,
    exclude: Vec<Regex>,
}

impl PathFilters {
    fn new(include: &[String], exclude: &[String]) -> ServiceResult<Self> {
        validate_glob_patterns("include", include)?;
        validate_glob_patterns("exclude", exclude)?;
        Ok(Self {
            include: include
                .iter()
                .map(|pattern| compile_glob(pattern))
                .collect::<ServiceResult<_>>()?,
            exclude: exclude
                .iter()
                .map(|pattern| compile_glob(pattern))
                .collect::<ServiceResult<_>>()?,
        })
    }

    fn allows(&self, path: &str) -> bool {
        (self.include.is_empty() || self.include.iter().any(|regex| regex.is_match(path)))
            && !self.exclude.iter().any(|regex| regex.is_match(path))
    }
}

fn validate_glob_patterns(label: &str, patterns: &[String]) -> ServiceResult<()> {
    if patterns.len() > limits::SEARCH_GLOB_PATTERNS_MAX {
        return Err(ServiceError::InvalidInput(format!(
            "{label} must contain at most {} glob patterns",
            limits::SEARCH_GLOB_PATTERNS_MAX
        )));
    }
    for pattern in patterns {
        if pattern.chars().count() > limits::SEARCH_GLOB_PATTERN_MAX_CHARS {
            return Err(ServiceError::InvalidInput(format!(
                "{label} glob patterns must be at most {} characters",
                limits::SEARCH_GLOB_PATTERN_MAX_CHARS
            )));
        }
    }
    Ok(())
}

fn compile_regex(pattern: &str) -> ServiceResult<Regex> {
    RegexBuilder::new(pattern)
        .case_insensitive(true)
        .build()
        .map_err(|error| ServiceError::InvalidInput(format!("invalid regex pattern: {error}")))
}

fn compile_glob(pattern: &str) -> ServiceResult<Regex> {
    let mut out = String::from("^");
    for ch in pattern.chars() {
        match ch {
            '*' => out.push_str(".*"),
            '?' => out.push('.'),
            _ => out.push_str(&regex::escape(&ch.to_string())),
        }
    }
    out.push('$');
    compile_regex(&out)
}

#[cfg(test)]
mod tests {
    #![allow(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::indexing_slicing,
        clippy::panic,
        clippy::unwrap_in_result
    )]
    use super::*;
    use crate::cursor;

    /// The search traversal cursor round-trips through the shared opaque codec.
    #[test]
    fn search_cursor_round_trips() {
        let value = SearchCursor {
            version: 1,
            command: "find".to_owned(),
            fingerprint: "fingerprint".to_owned(),
            stack: vec![DfsFrame {
                folder_node_id: Uuid::new_v4(),
                after: Some(ChildrenCursor {
                    sort_order: 0,
                    name: "note.md".to_owned(),
                    id: Uuid::new_v4(),
                }),
            }],
        };
        let encoded = cursor::encode(&value).unwrap();
        let decoded: SearchCursor = cursor::decode(&encoded).unwrap();
        assert_eq!(decoded, value);
    }

    /// A garbage cursor fails to decode.
    #[test]
    fn garbage_cursor_fails_to_decode() {
        assert!(cursor::decode::<SearchCursor>("!!!not-base64!!!").is_err());
    }

    #[test]
    fn invalid_queries_are_rejected() {
        assert!(matches!(
            validate_query("   "),
            Err(ServiceError::InvalidInput(_))
        ));
        assert!(matches!(
            validate_query("alpha\nbeta"),
            Err(ServiceError::InvalidInput(_))
        ));
        let too_long = "x".repeat(limits::SEARCH_QUERY_MAX_CHARS + 1);
        assert!(matches!(
            validate_query(&too_long),
            Err(ServiceError::InvalidInput(_))
        ));
        assert_eq!(validate_query("  note  ").unwrap(), "note");
    }

    #[test]
    fn glob_pattern_limits_are_rejected() {
        let too_many = vec!["*.md".to_owned(); limits::SEARCH_GLOB_PATTERNS_MAX + 1];
        assert!(matches!(
            PathFilters::new(&too_many, &[]),
            Err(ServiceError::InvalidInput(_))
        ));

        let too_long = vec!["x".repeat(limits::SEARCH_GLOB_PATTERN_MAX_CHARS + 1)];
        assert!(matches!(
            PathFilters::new(&[], &too_long),
            Err(ServiceError::InvalidInput(_))
        ));
    }
}
