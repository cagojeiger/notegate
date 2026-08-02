//! Pure Markdown reference extraction using the browser link path contract.

use std::collections::BTreeMap;

use notegate_model::LinkReferenceKind;
use percent_encoding::percent_decode_str;
use pulldown_cmark::{Event, Options, Parser, Tag};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedReference {
    pub kind: LinkReferenceKind,
    pub raw_href: String,
    pub normalized_target_path: Option<String>,
    pub occurrence_count: i32,
}

pub fn parse_references(source_path: &str, content: &str) -> Vec<ParsedReference> {
    let body = markdown_body(content);
    let options = Options::ENABLE_TABLES
        | Options::ENABLE_FOOTNOTES
        | Options::ENABLE_STRIKETHROUGH
        | Options::ENABLE_TASKLISTS;
    let mut references = BTreeMap::<(LinkReferenceKind, String), ParsedReference>::new();

    for event in Parser::new_ext(body, options) {
        let (kind, href) = match event {
            Event::Start(Tag::Link { dest_url, .. }) => {
                (LinkReferenceKind::Link, dest_url.into_string())
            }
            Event::Start(Tag::Image { dest_url, .. }) => {
                (LinkReferenceKind::Image, dest_url.into_string())
            }
            _ => continue,
        };
        let raw_href = href.trim().to_owned();
        let Some(intent) = classify_path(source_path, &raw_href) else {
            continue;
        };
        let key = (kind, raw_href.clone());
        references
            .entry(key)
            .and_modify(|reference| {
                reference.occurrence_count = reference.occurrence_count.saturating_add(1);
            })
            .or_insert(ParsedReference {
                kind,
                raw_href,
                normalized_target_path: intent,
                occurrence_count: 1,
            });
    }

    references.into_values().collect()
}

/// `None` means external/non-indexed. `Some(None)` means an invalid internal
/// candidate, and `Some(Some(path))` is a canonical Space path.
fn classify_path(source_path: &str, href: &str) -> Option<Option<String>> {
    let value = href.trim();
    if value.is_empty() || value.starts_with('#') || value.starts_with("//") || has_scheme(value) {
        return None;
    }

    let path_part = value
        .split_once('#')
        .map_or(value, |(path, _fragment)| path);
    if path_part.is_empty() || path_part.contains('?') {
        return Some(None);
    }
    let Some(decoded) = decode_path_segments(path_part) else {
        return Some(None);
    };
    let absolute = if decoded.starts_with('/') {
        decoded
    } else {
        let parent = parent_path(source_path);
        format!("{}/{decoded}", parent.trim_end_matches('/'))
    };
    Some(normalize_absolute_path(&absolute))
}

fn has_scheme(value: &str) -> bool {
    let Some(colon) = value.find(':') else {
        return false;
    };
    let boundary = [value.find('/'), value.find('?'), value.find('#')]
        .into_iter()
        .flatten()
        .min();
    if boundary.is_some_and(|boundary| colon > boundary) {
        return false;
    }
    let scheme = &value[..colon];
    let mut chars = scheme.chars();
    chars
        .next()
        .is_some_and(|first| first.is_ascii_alphabetic())
        && chars.all(|character| {
            character.is_ascii_alphanumeric() || matches!(character, '+' | '.' | '-')
        })
}

fn decode_path_segments(path: &str) -> Option<String> {
    path.split('/')
        .map(|segment| {
            if !has_valid_percent_encoding(segment) {
                return None;
            }
            let decoded = percent_decode_str(segment).decode_utf8().ok()?;
            if decoded.contains('/') || decoded.chars().any(char::is_control) {
                return None;
            }
            Some(decoded.into_owned())
        })
        .collect::<Option<Vec<_>>>()
        .map(|segments| segments.join("/"))
}

fn has_valid_percent_encoding(value: &str) -> bool {
    let mut bytes = value.bytes();
    while let Some(byte) = bytes.next() {
        if byte != b'%' {
            continue;
        }
        let Some(first) = bytes.next() else {
            return false;
        };
        let Some(second) = bytes.next() else {
            return false;
        };
        if !first.is_ascii_hexdigit() || !second.is_ascii_hexdigit() {
            return false;
        }
    }
    true
}

fn parent_path(source_path: &str) -> &str {
    let path = source_path.strip_prefix('/').unwrap_or(source_path);
    match path.rsplit_once('/') {
        Some((parent, _name)) if !parent.is_empty() => {
            let start = source_path.len().saturating_sub(path.len());
            &source_path[..start + parent.len()]
        }
        _ => "/",
    }
}

fn normalize_absolute_path(path: &str) -> Option<String> {
    let mut segments = Vec::new();
    for segment in path.split('/') {
        match segment {
            "" | "." => {}
            ".." => {
                segments.pop()?;
            }
            value => segments.push(value),
        }
    }
    Some(format!("/{}", segments.join("/")))
}

fn markdown_body(content: &str) -> &str {
    let content = content.strip_prefix('\u{feff}').unwrap_or(content);
    let Some((first_line, rest)) = content.split_once('\n') else {
        return content;
    };
    if first_line
        .trim_end_matches('\r')
        .trim_end_matches([' ', '\t'])
        != "---"
    {
        return content;
    }

    let mut offset = 0;
    for line_with_ending in rest.split_inclusive('\n') {
        let line = line_with_ending
            .trim_end_matches('\n')
            .trim_end_matches('\r')
            .trim_end_matches([' ', '\t']);
        if line == "---" || line == "..." {
            let yaml = &rest[..offset];
            if yaml.trim().is_empty() || yaml_is_mapping(yaml) {
                return &rest[offset + line_with_ending.len()..];
            }
            return content;
        }
        offset += line_with_ending.len();
    }
    content
}

fn yaml_is_mapping(source: &str) -> bool {
    yaml_rust2::YamlLoader::load_from_str(source)
        .ok()
        .and_then(|documents| documents.into_iter().next())
        .is_some_and(|document| matches!(document, yaml_rust2::Yaml::Hash(_)))
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::indexing_slicing)]

    use serde::Deserialize;

    use super::*;

    #[derive(Deserialize)]
    struct PathContractCase {
        name: String,
        source_path: String,
        href: String,
        expected: String,
    }

    fn paths(source: &str, markdown: &str) -> Vec<Option<String>> {
        parse_references(source, markdown)
            .into_iter()
            .map(|reference| reference.normalized_target_path)
            .collect()
    }

    #[test]
    fn matches_the_shared_browser_path_contract() {
        let cases: Vec<PathContractCase> = serde_json::from_str(include_str!(
            "../../../../../docs/spec/fixtures/markdown-link-paths.json"
        ))
        .unwrap();

        for case in cases {
            let actual = match classify_path(&case.source_path, &case.href) {
                None => "external".to_owned(),
                Some(None) => "invalid".to_owned(),
                Some(Some(path)) => path,
            };
            assert_eq!(actual, case.expected, "{}", case.name);
        }
    }

    #[test]
    fn parses_and_aggregates_standard_links_and_images() {
        let references = parse_references(
            "/docs/index.md",
            "[one](./A%20B.md) [again](./A%20B.md) ![image](../image.png)",
        );
        assert_eq!(references.len(), 2);
        assert_eq!(references[0].kind, LinkReferenceKind::Link);
        assert_eq!(
            references[0].normalized_target_path.as_deref(),
            Some("/docs/A B.md")
        );
        assert_eq!(references[0].occurrence_count, 2);
        assert_eq!(references[1].kind, LinkReferenceKind::Image);
        assert_eq!(
            references[1].normalized_target_path.as_deref(),
            Some("/image.png")
        );
    }

    #[test]
    fn mirrors_internal_path_validation() {
        assert_eq!(
            paths("/a/b.md", "[ok](../c.md#part)"),
            vec![Some("/c.md".to_owned())]
        );
        assert_eq!(paths("/a/b.md", "[query](./c.md?view=1)"), vec![None]);
        assert_eq!(paths("/a/b.md", "[slash](folder%2Fsecret.md)"), vec![None]);
        assert_eq!(paths("/a/b.md", "[control](bad%0Aname.md)"), vec![None]);
        assert_eq!(paths("/a.md", "[escape](../outside.md)"), vec![None]);
    }

    #[test]
    fn excludes_external_anchor_code_and_valid_frontmatter_links() {
        let content = "---\nrelated: '[hidden](secret.md)'\n---\n\
                       [visible](note.md) `![code](ignored.png)`\n\n\
                       ```md\n[also ignored](code.md)\n```\n\
                       [web](https://example.com) [anchor](#part)";
        let references = parse_references("/index.md", content);
        assert_eq!(references.len(), 1);
        assert_eq!(references[0].raw_href, "note.md");
    }

    #[test]
    fn keeps_invalid_or_non_mapping_frontmatter_as_markdown() {
        let content = "---\n- '[visible](note.md)'\n---\n";
        let references = parse_references("/index.md", content);
        assert_eq!(references.len(), 1);
    }
}
