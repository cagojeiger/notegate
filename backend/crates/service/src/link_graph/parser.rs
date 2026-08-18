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

#[derive(Debug, Clone, Copy, thiserror::Error, PartialEq, Eq)]
pub(super) enum ParseInternalReferencesError {
    #[error("text contains more than {max} unique internal references")]
    TooManyReferences { max: usize },
}

pub(super) fn parse_internal_references(
    source_path: &str,
    content: &str,
) -> Result<Vec<ParsedLinkReference>, ParseInternalReferencesError> {
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
        let key = (target_path, kind);
        if let Some(count) = references.get_mut(&key) {
            *count += 1;
            continue;
        }
        if references.len() >= notegate_core::limits::LINK_REFERENCES_PER_TEXT_MAX {
            return Err(ParseInternalReferencesError::TooManyReferences {
                max: notegate_core::limits::LINK_REFERENCES_PER_TEXT_MAX,
            });
        }
        references.insert(key, 1);
    }

    Ok(references
        .into_iter()
        .map(
            |((target_path, kind), occurrence_count)| ParsedLinkReference {
                target_path,
                kind,
                occurrence_count,
            },
        )
        .collect())
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
            (!decoded.contains('/') && !decoded.chars().any(is_forbidden_path_character))
                .then(|| decoded.into_owned())
        })
        .collect::<Option<Vec<_>>>()
        .map(|segments| segments.join("/"))
}

fn is_forbidden_path_character(character: char) -> bool {
    character.is_control()
        || matches!(
            character,
            '\u{061c}'
                | '\u{200e}'
                | '\u{200f}'
                | '\u{2028}'..='\u{202e}'
                | '\u{2066}'..='\u{206f}'
        )
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
    fn parses_standard_internal_links_and_images() -> Result<(), ParseInternalReferencesError> {
        let references = parse_internal_references(
            "/docs/current.md",
            "[one](../README.md) [again](../README.md#top) ![asset](./a%20b.png) \
             [web](https://example.com) [anchor](#local)",
        )?;

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
        Ok(())
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
    fn follows_commonmark_links_but_ignores_code_and_raw_html()
    -> Result<(), ParseInternalReferencesError> {
        let references = parse_internal_references(
            "/docs/current.md",
            "[reference][target]\n\n[target]: ./target.md\n\n\
             `<a href=\"./code.md\">code</a>`\n\n\
             <a href=\"./raw.md\">raw</a>\n\n\
             ```md\n[code](./fenced.md)\n```",
        )?;

        assert_eq!(
            references,
            vec![ParsedLinkReference {
                target_path: "/docs/target.md".to_owned(),
                kind: LinkReferenceKind::Link,
                occurrence_count: 1,
            }]
        );
        Ok(())
    }

    #[test]
    fn ignores_obsidian_wikilinks() -> Result<(), ParseInternalReferencesError> {
        let references = parse_internal_references(
            "/docs/current.md",
            "[[target.md]] ![[image.png]] [standard](./target.md)",
        )?;

        assert_eq!(
            references,
            vec![ParsedLinkReference {
                target_path: "/docs/target.md".to_owned(),
                kind: LinkReferenceKind::Link,
                occurrence_count: 1,
            }]
        );
        Ok(())
    }

    #[test]
    fn keeps_link_and_image_occurrences_separate() -> Result<(), ParseInternalReferencesError> {
        let references = parse_internal_references(
            "/docs/current.md",
            "[file](./asset.png) ![image](./asset.png) ![again](./asset.png)",
        )?;

        assert_eq!(
            references,
            vec![
                ParsedLinkReference {
                    target_path: "/docs/asset.png".to_owned(),
                    kind: LinkReferenceKind::Link,
                    occurrence_count: 1,
                },
                ParsedLinkReference {
                    target_path: "/docs/asset.png".to_owned(),
                    kind: LinkReferenceKind::Image,
                    occurrence_count: 2,
                },
            ]
        );
        Ok(())
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
        assert_eq!(
            internal_target_path("/docs/current.md", "./hidden%C2%85name.md"),
            None
        );
        assert_eq!(
            internal_target_path("/docs/current.md", "./hidden%E2%80%AEname.md"),
            None
        );
        assert_eq!(
            internal_target_path("/docs/current.md", "./hidden\u{2066}name.md"),
            None
        );
    }

    #[test]
    fn rejects_more_than_the_unique_reference_limit() {
        let content = (0..=notegate_core::limits::LINK_REFERENCES_PER_TEXT_MAX)
            .map(|index| format!("[{index}](./target-{index}.md)"))
            .collect::<Vec<_>>()
            .join(" ");

        assert_eq!(
            parse_internal_references("/source.md", &content),
            Err(ParseInternalReferencesError::TooManyReferences {
                max: notegate_core::limits::LINK_REFERENCES_PER_TEXT_MAX,
            })
        );
    }

    #[test]
    fn repeated_references_count_once_toward_the_limit() -> Result<(), ParseInternalReferencesError>
    {
        let content = std::iter::repeat_n(
            "[target](./target.md)",
            notegate_core::limits::LINK_REFERENCES_PER_TEXT_MAX + 1,
        )
        .collect::<Vec<_>>()
        .join(" ");

        assert_eq!(
            parse_internal_references("/source.md", &content)?,
            vec![ParsedLinkReference {
                target_path: "/target.md".to_owned(),
                kind: LinkReferenceKind::Link,
                occurrence_count: 1_001,
            }]
        );
        Ok(())
    }
}
