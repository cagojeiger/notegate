//! Space search: `find` (node name metadata) and `grep` (content).
//!
//! The service owns authorization, limit clamping, opaque cursors, and search
//! result shaping. The two query implementations live in the [`find`] and [`grep`]
//! submodules; shared types, the permission gate, and query validation live here.

use notegate_core::{SearchBodyCacheConfig, limits};
use notegate_db::FilesRepo;
use notegate_model::files::{ChildrenCursor, NodeView, TextStats};
use notegate_model::search::SearchTextCandidate;
pub use notegate_model::search::{
    FindMatchMode, FindPage, FindRequest, GrepLineMode, GrepMatchMode, GrepPage, GrepRequest,
    SearchCursor, TreeCursor, TreeFrame, TreePage, TreeRequest,
};
use notegate_model::{Node, NodeKind, Permission, TextStorageFormat};
use uuid::Uuid;

use crate::cursor;
use crate::error::{ServiceError, ServiceResult};
use crate::files::policy::{self, FileCommand};

mod body_cache;
mod find;
mod grep;
mod matcher;
mod telemetry;
mod tree;

use body_cache::SearchBodyCache;
use telemetry::SearchTelemetry;

/// Process-local snapshot of the decrypted search body cache.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct SearchBodyCacheStats {
    pub entries: u64,
    pub size_bytes: u64,
    pub capacity_bytes: u64,
}

/// Search service. The `find`/`grep` query methods are implemented in the
/// [`find`] and [`grep`] submodules.
#[derive(Debug, Clone)]
pub struct SearchService {
    store: FilesRepo,
    body_cache: SearchBodyCache,
    telemetry: SearchTelemetry,
}

impl SearchService {
    pub fn new(store: FilesRepo) -> Self {
        Self::with_body_cache_config(store, SearchBodyCacheConfig::default())
    }

    pub fn with_body_cache_config(
        store: FilesRepo,
        body_cache_config: SearchBodyCacheConfig,
    ) -> Self {
        Self {
            store,
            body_cache: SearchBodyCache::new(body_cache_config),
            telemetry: SearchTelemetry::default(),
        }
    }

    pub fn with_metrics_enabled(mut self, enabled: bool) -> Self {
        self.telemetry = SearchTelemetry::new(enabled);
        self
    }

    pub fn body_cache_stats(&self) -> SearchBodyCacheStats {
        self.body_cache.stats()
    }

    async fn resolve_scope_folder(
        &self,
        space_id: Uuid,
        path: Option<&str>,
    ) -> ServiceResult<(Uuid, String)> {
        let normalized = match path {
            Some(path) => crate::files::validation::normalize_path(path)?,
            None => "/".to_owned(),
        };
        let (node_id, kind, path) = self
            .store
            .resolve_search_scope(space_id, &normalized)
            .await?
            .ok_or_else(|| ServiceError::NotFound("scope path not found".to_owned()))?;
        if kind != NodeKind::Folder {
            return Err(ServiceError::InvalidInput(
                "search scope must be a folder".to_owned(),
            ));
        }
        Ok((node_id, path))
    }

    async fn node_views(
        &self,
        space_id: Uuid,
        rows: Vec<(Node, String)>,
    ) -> ServiceResult<Vec<NodeView>> {
        crate::files::hydrate_node_views(&self.store, space_id, rows).await
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

fn text_node_view(candidate: &SearchTextCandidate) -> NodeView {
    NodeView {
        node: candidate.node.clone(),
        path: candidate.path.clone(),
        has_children: false,
        text: Some(TextStats {
            content_sha256: candidate.content_sha256.clone(),
            byte_len: candidate.byte_len,
            line_count: candidate.line_count,
            storage_format: TextStorageFormat::Plain,
            at_rest_encryption: candidate.at_rest_encryption,
        }),
        file: None,
        write_lock_sources: Vec::new(),
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

fn decode_search_cursor(
    raw: Option<&str>,
    command: &str,
    fingerprint: &str,
    scope_node_id: Uuid,
) -> ServiceResult<Option<String>> {
    match raw {
        None => Ok(None),
        Some(raw) => {
            let cursor: SearchCursor = cursor::decode(raw)?;
            if cursor.version != 1
                || cursor.command != command
                || cursor.fingerprint != fingerprint
                || cursor.scope_node_id != scope_node_id
            {
                return Err(ServiceError::InvalidInput(
                    "search cursor does not match this query".to_owned(),
                ));
            }
            Ok(cursor.after_sort_path)
        }
    }
}

fn encode_search_cursor(
    command: &str,
    fingerprint: String,
    scope_node_id: Uuid,
    after_sort_path: Option<String>,
) -> ServiceResult<Option<String>> {
    if after_sort_path.is_none() {
        return Ok(None);
    }
    let cursor = SearchCursor {
        version: 1,
        command: command.to_owned(),
        fingerprint,
        scope_node_id,
        after_sort_path,
    };
    cursor::encode(&cursor)
        .map(Some)
        .map_err(|_error| ServiceError::Internal("failed to encode cursor".to_owned()))
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
    use super::matcher::{ContentMatcher, NameMatcher, PathFilters, logical_lines};
    use super::*;
    use crate::cursor;

    #[test]
    fn search_cursor_helpers_round_trip_position_and_omit_empty_cursor() {
        let scope_node_id = Uuid::new_v4();
        assert_eq!(
            encode_search_cursor("find", "query".to_owned(), scope_node_id, None).unwrap(),
            None
        );
        assert_eq!(
            decode_search_cursor(None, "find", "query", scope_node_id).unwrap(),
            None
        );

        let after_sort_path = "0000000000/note.md".to_owned();
        let encoded = encode_search_cursor(
            "find",
            "query".to_owned(),
            scope_node_id,
            Some(after_sort_path.clone()),
        )
        .unwrap()
        .unwrap();
        assert_eq!(
            decode_search_cursor(Some(&encoded), "find", "query", scope_node_id).unwrap(),
            Some(after_sort_path)
        );
    }

    #[test]
    fn search_cursor_rejects_invalid_or_mismatched_queries() {
        let scope_node_id = Uuid::new_v4();
        assert!(matches!(
            decode_search_cursor(Some("!!!not-base64!!!"), "find", "query", scope_node_id),
            Err(ServiceError::InvalidInput(_))
        ));

        for cursor in [
            SearchCursor {
                version: 2,
                command: "find".to_owned(),
                fingerprint: "query".to_owned(),
                scope_node_id,
                after_sort_path: Some("version".to_owned()),
            },
            SearchCursor {
                version: 1,
                command: "grep".to_owned(),
                fingerprint: "query".to_owned(),
                scope_node_id,
                after_sort_path: Some("command".to_owned()),
            },
            SearchCursor {
                version: 1,
                command: "find".to_owned(),
                fingerprint: "other-query".to_owned(),
                scope_node_id,
                after_sort_path: Some("fingerprint".to_owned()),
            },
            SearchCursor {
                version: 1,
                command: "find".to_owned(),
                fingerprint: "query".to_owned(),
                scope_node_id: Uuid::new_v4(),
                after_sort_path: Some("scope".to_owned()),
            },
        ] {
            let encoded = cursor::encode(&cursor).unwrap();
            assert!(matches!(
                decode_search_cursor(Some(&encoded), "find", "query", scope_node_id),
                Err(ServiceError::InvalidInput(message))
                    if message == "search cursor does not match this query"
            ));
        }
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

    #[test]
    fn name_matchers_are_case_insensitive() {
        let contains = NameMatcher::new("NOTE", FindMatchMode::Contains).unwrap();
        assert!(contains.is_match("release-note.md"));
        assert!(!contains.is_match("release.md"));

        let regex = NameMatcher::new(r"^note-\d+\.md$", FindMatchMode::Regex).unwrap();
        assert!(regex.is_match("NOTE-42.MD"));
        assert!(!regex.is_match("draft-note-42.md"));

        let glob = NameMatcher::new("note-??.md", FindMatchMode::Glob).unwrap();
        assert!(glob.is_match("NOTE-42.MD"));
        assert!(!glob.is_match("draft-note-42.md"));
        assert!(!glob.is_match("note-7.md"));
    }

    #[test]
    fn glob_treats_non_wildcard_regex_characters_as_literals() {
        let matcher = NameMatcher::new("report[1].md", FindMatchMode::Glob).unwrap();
        assert!(matcher.is_match("REPORT[1].MD"));
        assert!(!matcher.is_match("report1.md"));
    }

    #[test]
    fn invalid_regex_patterns_are_rejected() {
        assert!(matches!(
            NameMatcher::new("(", FindMatchMode::Regex),
            Err(ServiceError::InvalidInput(message))
                if message.starts_with("invalid regex pattern:")
        ));
        assert!(matches!(
            ContentMatcher::new("[", GrepMatchMode::Regex),
            Err(ServiceError::InvalidInput(message))
                if message.starts_with("invalid regex pattern:")
        ));
    }

    #[test]
    fn content_matcher_reports_requested_logical_lines() {
        let matcher = ContentMatcher::new("ALPHA", GrepMatchMode::Literal).unwrap();
        let content = "alpha\nbeta ALPHA\nomega\n";

        assert_eq!(matcher.match_lines(content, GrepLineMode::None), vec![1]);
        assert_eq!(matcher.match_lines(content, GrepLineMode::First), vec![1]);
        assert_eq!(matcher.match_lines(content, GrepLineMode::All), vec![1, 2]);
    }

    #[test]
    fn content_literal_escapes_regex_syntax() {
        let matcher = ContentMatcher::new(r"a.b*[x]\path", GrepMatchMode::Literal).unwrap();

        assert_eq!(
            matcher.match_lines(
                "A.B*[X]\\PATH\naxbxxxpath\na.bbbbb[x]\\path\n",
                GrepLineMode::All
            ),
            vec![1]
        );
    }

    #[test]
    fn content_literal_uses_ripgrep_unicode_case_folding() {
        for (query, content, expected) in [
            ("i", "İstanbul\nistanbul\n", vec![2]),
            ("ς", "Σ\nσ\nς\n", vec![1, 2, 3]),
            ("ss", "Straße\nSS\n", vec![2]),
            ("k", "Kelvin\nkelvin\n", vec![1, 2]),
        ] {
            let matcher = ContentMatcher::new(query, GrepMatchMode::Literal).unwrap();
            assert_eq!(
                matcher.match_lines(content, GrepLineMode::All),
                expected,
                "query={query}"
            );
        }
    }

    #[test]
    fn content_regex_is_case_insensitive_and_matches_per_line() {
        let matcher = ContentMatcher::new(r"^error(?:-\d+)?$", GrepMatchMode::Regex).unwrap();

        assert_eq!(
            matcher.match_lines("ok\nERROR-42\nerror details\n", GrepLineMode::All),
            vec![2]
        );
    }

    #[test]
    fn content_matcher_preserves_results_across_modes_and_unicode() {
        for (query, match_mode, content, expected) in [
            (
                "needle",
                GrepMatchMode::Literal,
                "NEEDLE\nnone\nneedle\n",
                vec![1, 3],
            ),
            (
                "İSTANBUL",
                GrepMatchMode::Literal,
                "istanbul\nİstanbul\n",
                vec![2],
            ),
            (
                r"^error(?:-\d+)?$",
                GrepMatchMode::Regex,
                "ERROR\nerror-42\nerror details\n",
                vec![1, 2],
            ),
        ] {
            let matcher = ContentMatcher::new(query, match_mode).unwrap();
            assert_eq!(
                matcher.match_lines(content, GrepLineMode::None),
                expected.first().copied().into_iter().collect::<Vec<_>>()
            );
            assert_eq!(
                matcher.match_lines(content, GrepLineMode::First),
                expected.first().copied().into_iter().collect::<Vec<_>>()
            );
            assert_eq!(matcher.match_lines(content, GrepLineMode::All), expected);
        }
    }

    #[test]
    fn logical_lines_omit_only_the_terminal_newline() {
        assert_eq!(logical_lines("").collect::<Vec<_>>(), vec![""]);
        assert_eq!(
            logical_lines("first\nsecond\n").collect::<Vec<_>>(),
            vec!["first", "second"]
        );
        assert_eq!(
            logical_lines("first\n\nthird").collect::<Vec<_>>(),
            vec!["first", "", "third"]
        );

        let matcher = ContentMatcher::new("^$", GrepMatchMode::Regex).unwrap();
        assert!(matcher.match_lines("", GrepLineMode::All).is_empty());
    }

    #[test]
    fn path_filters_apply_include_then_exclude() {
        let unrestricted = PathFilters::new(&[], &[]).unwrap();
        assert!(unrestricted.allows("/notes/release.txt"));

        let filters = PathFilters::new(
            &["/notes/*.md".to_owned(), "/docs/*.txt".to_owned()],
            &["*/private/*".to_owned(), "*draft*".to_owned()],
        )
        .unwrap();
        assert!(filters.allows("/NOTES/release.MD"));
        assert!(filters.allows("/docs/readme.txt"));
        assert!(!filters.allows("/notes/private/release.md"));
        assert!(!filters.allows("/notes/draft.md"));
        assert!(!filters.allows("/images/release.png"));
    }

    #[derive(Clone, Copy)]
    enum BenchmarkMatchPosition {
        Early,
        Late,
        Absent,
    }

    impl BenchmarkMatchPosition {
        fn as_str(self) -> &'static str {
            match self {
                Self::Early => "early",
                Self::Late => "late",
                Self::Absent => "absent",
            }
        }
    }

    fn benchmark_content(position: BenchmarkMatchPosition) -> String {
        const CONTENT_BYTES: usize = 8 * 1024 * 1024;
        const LINE_BYTES: usize = 64;
        const NEEDLE: &[u8] = b"NeEdLe";

        let mut bytes = vec![b'x'; CONTENT_BYTES];
        for newline in ((LINE_BYTES - 1)..CONTENT_BYTES).step_by(LINE_BYTES) {
            bytes[newline] = b'\n';
        }
        let needle_offset = match position {
            BenchmarkMatchPosition::Early => Some(0),
            BenchmarkMatchPosition::Late => Some(CONTENT_BYTES - LINE_BYTES),
            BenchmarkMatchPosition::Absent => None,
        };
        if let Some(offset) = needle_offset {
            bytes[offset..offset + NEEDLE.len()].copy_from_slice(NEEDLE);
        }
        String::from_utf8(bytes).unwrap()
    }

    fn benchmark_expected_lines(position: BenchmarkMatchPosition) -> Vec<i32> {
        match position {
            BenchmarkMatchPosition::Early => vec![1],
            BenchmarkMatchPosition::Late => vec![131_072],
            BenchmarkMatchPosition::Absent => Vec::new(),
        }
    }

    /// Run with:
    /// `cargo test --release -p notegate-service --lib benchmark_matcher_8_mib -- --ignored --nocapture`
    #[test]
    #[ignore = "local 8 MiB matcher benchmark"]
    fn benchmark_matcher_8_mib() {
        use std::hint::black_box;
        use std::time::Instant;

        const SAMPLES: usize = 5;
        for position in [
            BenchmarkMatchPosition::Early,
            BenchmarkMatchPosition::Late,
            BenchmarkMatchPosition::Absent,
        ] {
            let content = benchmark_content(position);
            let expected = benchmark_expected_lines(position);
            for (match_mode, query) in [
                (GrepMatchMode::Literal, "needle"),
                (GrepMatchMode::Regex, r"n[e]{2}dle"),
            ] {
                let matcher = ContentMatcher::new(query, match_mode).unwrap();
                for line_mode in [GrepLineMode::None, GrepLineMode::First, GrepLineMode::All] {
                    let expected = match line_mode {
                        GrepLineMode::None | GrepLineMode::First => {
                            expected.first().copied().into_iter().collect()
                        }
                        GrepLineMode::All => expected.clone(),
                    };
                    assert_eq!(
                        matcher.match_lines(black_box(&content), line_mode),
                        expected
                    );

                    let batch_iterations = match (position, line_mode) {
                        (
                            BenchmarkMatchPosition::Early,
                            GrepLineMode::None | GrepLineMode::First,
                        ) => 10_000,
                        _ => 1,
                    };
                    let mut samples = Vec::with_capacity(SAMPLES);
                    for _ in 0..SAMPLES {
                        let started = Instant::now();
                        for _ in 0..batch_iterations {
                            let actual = matcher.match_lines(black_box(&content), line_mode);
                            black_box(actual);
                        }
                        samples.push(started.elapsed() / batch_iterations);
                    }
                    samples.sort_unstable();
                    let median = samples[SAMPLES / 2];
                    println!(
                        "matcher_bench bytes={} match={} position={} lines={} median_ns={}",
                        content.len(),
                        match_mode.as_str(),
                        position.as_str(),
                        line_mode.as_str(),
                        median.as_nanos()
                    );
                }
            }
        }
    }
}
