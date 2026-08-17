use std::collections::BTreeMap;

use notegate_model::LinkReferenceKind;
use percent_encoding::percent_decode_str;
use pulldown_cmark::{Event, Parser, Tag};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct ParsedLinkReference {
    pub target_path: String,
    pub kind: LinkReferenceKind,
    pub occurrence_count: i32,
}

pub(super) fn parse_internal_references(
    source_path: &str,
    content: &str,
) -> Vec<ParsedLinkReference> {
    let mut references = BTreeMap::<(String, LinkReferenceKind), i32>::new();
    for event in Parser::new(content) {
        let (destination, kind) = match event {
            Event::Start(Tag::Link { dest_url, .. }) => (dest_url, LinkReferenceKind::Link),
            Event::Start(Tag::Image { dest_url, .. }) => (dest_url, LinkReferenceKind::Image),
            _ => continue,
        };
        let Some(target_path) = internal_target_path(source_path, destination.as_ref()) else {
            continue;
        };
        references
            .entry((target_path, kind))
            .and_modify(|count| *count += 1)
            .or_insert(1);
    }

    references
        .into_iter()
        .map(
            |((target_path, kind), occurrence_count)| ParsedLinkReference {
                target_path,
                kind,
                occurrence_count,
            },
        )
        .collect()
}

fn internal_target_path(source_path: &str, destination: &str) -> Option<String> {
    let value = destination.trim();
    if value.is_empty()
        || value.starts_with('#')
        || value.starts_with("//")
        || has_url_scheme(value)
    {
        return None;
    }

    let path = value
        .split_once('#')
        .map_or(value, |(path, _fragment)| path);
    if path.is_empty() || path.contains('?') {
        return None;
    }

    let decoded = decode_path_segments(path)?;
    let absolute = if decoded.starts_with('/') {
        decoded
    } else {
        let parent = source_path
            .rfind('/')
            .filter(|index| *index > 0)
            .map_or("/", |index| &source_path[..index]);
        if parent == "/" {
            format!("/{decoded}")
        } else {
            format!("{parent}/{decoded}")
        }
    };
    let normalized = normalize_absolute_path(&absolute)?;
    notegate_core::validation::normalize_path(&normalized).ok()
}

fn has_url_scheme(value: &str) -> bool {
    let Some(colon) = value.find(':') else {
        return false;
    };
    if value
        .find(['/', '?', '#'])
        .is_some_and(|separator| separator < colon)
    {
        return false;
    }

    let mut characters = value[..colon].chars();
    characters
        .next()
        .is_some_and(|first| first.is_ascii_alphabetic())
        && characters.all(|character| {
            character.is_ascii_alphanumeric() || matches!(character, '+' | '-' | '.')
        })
}

fn decode_path_segments(path: &str) -> Option<String> {
    path.split('/')
        .map(|segment| {
            if !has_valid_percent_encoding(segment) {
                return None;
            }
            let decoded = percent_decode_str(segment).decode_utf8().ok()?;
            (!decoded.contains('/') && !decoded.chars().any(is_ascii_control))
                .then(|| decoded.into_owned())
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
        let (Some(first), Some(second)) = (bytes.next(), bytes.next()) else {
            return false;
        };
        if !first.is_ascii_hexdigit() || !second.is_ascii_hexdigit() {
            return false;
        }
    }
    true
}

fn is_ascii_control(character: char) -> bool {
    matches!(character as u32, 0x00..=0x1f | 0x7f)
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_standard_internal_links_and_images() {
        let references = parse_internal_references(
            "/docs/current.md",
            "[one](../README.md) [again](../README.md#top) ![asset](./a%20b.png) \
             [web](https://example.com) [anchor](#local)",
        );

        assert_eq!(
            references,
            vec![
                ParsedLinkReference {
                    target_path: "/README.md".to_owned(),
                    kind: LinkReferenceKind::Link,
                    occurrence_count: 2,
                },
                ParsedLinkReference {
                    target_path: "/docs/a b.png".to_owned(),
                    kind: LinkReferenceKind::Image,
                    occurrence_count: 1,
                },
            ]
        );
    }

    #[test]
    fn rejects_escape_query_encoded_slash_and_protocols() {
        for destination in [
            "../../outside.md",
            "note.md?view=1",
            "bad%path.md",
            "hidden%2Fchild.md",
            "mailto:test@example.com",
            "//example.com/note.md",
        ] {
            assert_eq!(
                internal_target_path("/docs/current.md", destination),
                None,
                "{destination}"
            );
        }
    }

    #[test]
    fn resolves_relative_root_and_fragment_paths() {
        assert_eq!(
            internal_target_path("/docs/current.md", "./child.md"),
            Some("/docs/child.md".to_owned())
        );
        assert_eq!(
            internal_target_path("/docs/current.md", "/README.md#top"),
            Some("/README.md".to_owned())
        );
        assert_eq!(internal_target_path("/current.md", "../outside.md"), None);
    }

    #[test]
    fn follows_commonmark_links_but_ignores_code_and_raw_html() {
        let references = parse_internal_references(
            "/docs/current.md",
            "[reference][target]\n\n[target]: ./target.md\n\n\
             `<a href=\"./code.md\">code</a>`\n\n\
             <a href=\"./raw.md\">raw</a>\n\n\
             ```md\n[code](./fenced.md)\n```",
        );

        assert_eq!(
            references,
            vec![ParsedLinkReference {
                target_path: "/docs/target.md".to_owned(),
                kind: LinkReferenceKind::Link,
                occurrence_count: 1,
            }]
        );
    }

    #[test]
    fn ignores_obsidian_wikilinks() {
        let references = parse_internal_references(
            "/docs/current.md",
            "[[target.md]] ![[image.png]] [standard](./target.md)",
        );

        assert_eq!(
            references,
            vec![ParsedLinkReference {
                target_path: "/docs/target.md".to_owned(),
                kind: LinkReferenceKind::Link,
                occurrence_count: 1,
            }]
        );
    }

    #[test]
    fn keeps_link_and_image_occurrences_separate() {
        let references = parse_internal_references(
            "/docs/current.md",
            "[file](./asset.png) ![image](./asset.png) ![again](./asset.png)",
        );

        assert_eq!(references.len(), 2);
        assert_eq!(references[0].kind, LinkReferenceKind::Link);
        assert_eq!(references[0].occurrence_count, 1);
        assert_eq!(references[1].kind, LinkReferenceKind::Image);
        assert_eq!(references[1].occurrence_count, 2);
    }

    #[test]
    fn accepts_unicode_and_rejects_encoded_controls() {
        assert_eq!(
            internal_target_path("/문서/현재.md", "./다음%20문서.md"),
            Some("/문서/다음 문서.md".to_owned())
        );
        assert_eq!(
            internal_target_path("/docs/current.md", "./Design%20%231%3F.md"),
            Some("/docs/Design #1?.md".to_owned())
        );
        assert_eq!(
            internal_target_path("/docs/current.md", "./hidden%00name.md"),
            None
        );
    }
}
