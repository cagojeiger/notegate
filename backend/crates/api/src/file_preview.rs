use std::collections::HashSet;
use std::io::{Cursor, Read};
use std::path::Path;

use notegate_model::FileEncryptionMode;
use roxmltree::{Document, ParsingOptions};
use serde::Serialize;
use utoipa::ToSchema;
use zip::{CompressionMethod, ZipArchive};

use crate::object_storage::{ObjectStorage, ObjectStorageError};

pub const PREVIEW_URL_TTL_SECONDS: i64 = 15 * 60;
pub const PREVIEW_MAX_BYTES: i64 = 10 * 1024 * 1024;
const PDF_MEDIA_TYPE: &str = "application/pdf";
const DOCX_MEDIA_TYPE: &str =
    "application/vnd.openxmlformats-officedocument.wordprocessingml.document";
const DOCX_MAIN_DOCUMENT_MEDIA_TYPE: &str =
    "application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml";
const OFFICE_DOCUMENT_RELATIONSHIP_TYPE: &str =
    "http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument";
const STRICT_OFFICE_DOCUMENT_RELATIONSHIP_TYPE: &str =
    "http://purl.oclc.org/ooxml/officeDocument/relationships/officeDocument";
const CONTENT_TYPES_NAMESPACE: &str =
    "http://schemas.openxmlformats.org/package/2006/content-types";
const PACKAGE_RELATIONSHIPS_NAMESPACE: &str =
    "http://schemas.openxmlformats.org/package/2006/relationships";
const STRICT_PACKAGE_RELATIONSHIPS_NAMESPACE: &str =
    "http://purl.oclc.org/ooxml/package/relationships";
const MEDIA_TYPE_SNIFF_BYTES: usize = 8 * 1024;
const UNKNOWN_MEDIA_TYPE: &str = "application/octet-stream";
const ZIP_MEDIA_TYPE: &str = "application/zip";
const DOCX_MAX_ENTRIES: usize = 2_048;
const DOCX_MAX_ENTRY_BYTES: u64 = 32 * 1024 * 1024;
const DOCX_MAX_EXPANDED_BYTES: u64 = 64 * 1024 * 1024;
const DOCX_CONTENT_TYPES_MAX_BYTES: u64 = 64 * 1024;
const DOCX_ROOT_RELATIONSHIPS_MAX_BYTES: u64 = 64 * 1024;
const DOCX_XML_MAX_NODES: u32 = 4_096;
const ZIP_LOCAL_FILE_SIGNATURE: &[u8; 4] = b"PK\x03\x04";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum FilePreviewKind {
    Image,
    Pdf,
    Docx,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum FileMediaKind {
    Image,
    Pdf,
    Audio,
    Other,
}

pub async fn detect_object_media_type(
    storage: &ObjectStorage,
    object_key: &str,
    byte_len: i64,
    encryption_mode: FileEncryptionMode,
    declared_media_type: &str,
    original_filename: Option<&str>,
) -> Result<Option<String>, ObjectStorageError> {
    if encryption_mode == FileEncryptionMode::Client {
        return Ok(None);
    }
    if byte_len == 0 {
        return Ok(Some(UNKNOWN_MEDIA_TYPE.to_owned()));
    }

    let prefix = storage
        .read_prefix(object_key, MEDIA_TYPE_SNIFF_BYTES)
        .await?;
    let inferred_media_type = infer::get(&prefix)
        .map(|kind| kind.mime_type())
        .unwrap_or(UNKNOWN_MEDIA_TYPE);
    let media_type = if is_preview_size_allowed(byte_len)
        && is_docx_candidate(
            &prefix,
            declared_media_type,
            original_filename,
            inferred_media_type,
        ) {
        let max_bytes = usize::try_from(byte_len).map_err(|_| ObjectStorageError::Unavailable)?;
        let bytes = if max_bytes <= prefix.len() {
            prefix
        } else {
            storage.read_prefix(object_key, max_bytes).await?
        };
        detect_media_type_from_bytes(&bytes)
    } else {
        inferred_media_type
    };
    Ok(Some(media_type.to_owned()))
}

fn is_docx_candidate(
    prefix: &[u8],
    declared_media_type: &str,
    original_filename: Option<&str>,
    inferred_media_type: &str,
) -> bool {
    prefix.starts_with(ZIP_LOCAL_FILE_SIGNATURE)
        && (inferred_media_type == DOCX_MEDIA_TYPE
            || media_type_essence(declared_media_type).eq_ignore_ascii_case(DOCX_MEDIA_TYPE)
            || original_filename.is_some_and(has_docx_extension))
}

fn has_docx_extension(filename: &str) -> bool {
    filename
        .rsplit_once('.')
        .is_some_and(|(_, extension)| extension.eq_ignore_ascii_case("docx"))
}

fn media_type_essence(media_type: &str) -> &str {
    media_type
        .split_once(';')
        .map_or(media_type, |(essence, _)| essence)
        .trim()
}

fn detect_media_type_from_bytes(bytes: &[u8]) -> &'static str {
    if bytes.starts_with(ZIP_LOCAL_FILE_SIGNATURE) {
        if is_docx_package(bytes) {
            return DOCX_MEDIA_TYPE;
        }

        let inferred = infer::get(bytes)
            .map(|kind| kind.mime_type())
            .unwrap_or(ZIP_MEDIA_TYPE);
        return if inferred == DOCX_MEDIA_TYPE {
            ZIP_MEDIA_TYPE
        } else {
            inferred
        };
    }

    infer::get(bytes)
        .map(|kind| kind.mime_type())
        .unwrap_or(UNKNOWN_MEDIA_TYPE)
}

fn is_docx_package(bytes: &[u8]) -> bool {
    let Ok(mut archive) = ZipArchive::new(Cursor::new(bytes)) else {
        return false;
    };
    if archive.offset() != 0 || archive.is_empty() || archive.len() > DOCX_MAX_ENTRIES {
        return false;
    }

    let mut total_expanded_bytes = 0_u64;
    let mut normalized_paths = HashSet::with_capacity(archive.len());
    let mut has_content_types = false;
    let mut has_package_relationships = false;
    let mut has_main_document = false;

    for index in 0..archive.len() {
        let Ok(entry) = archive.by_index_raw(index) else {
            return false;
        };
        let name = entry.name();
        let normalized_path = name.strip_suffix('/').unwrap_or(name).to_ascii_lowercase();
        if !is_supported_docx_entry(
            entry.encrypted(),
            entry.is_symlink(),
            entry.compression(),
            entry.compressed_size(),
        ) || entry.enclosed_name().as_deref() != Some(Path::new(name))
            || !is_canonical_zip_path(name)
            || !normalized_paths.insert(normalized_path)
        {
            return false;
        }
        let Some(next_total) = bounded_expanded_total(total_expanded_bytes, entry.size()) else {
            return false;
        };
        total_expanded_bytes = next_total;

        let lowercase_name = name.to_ascii_lowercase();
        if lowercase_name == "word/vbaproject.bin"
            || lowercase_name.starts_with("word/activex/")
            || lowercase_name.starts_with("word/embeddings/")
        {
            return false;
        }

        match name {
            "[Content_Types].xml" => {
                if entry.size() > DOCX_CONTENT_TYPES_MAX_BYTES {
                    return false;
                }
                has_content_types = true;
            }
            "_rels/.rels" => has_package_relationships = true,
            "word/document.xml" => has_main_document = true,
            _ => {}
        }
    }

    if !has_content_types || !has_package_relationships || !has_main_document {
        return false;
    }

    let Ok(mut content_types) = archive.by_name("[Content_Types].xml") else {
        return false;
    };
    let mut xml = String::new();
    let Ok(read_bytes) = content_types
        .by_ref()
        .take(DOCX_CONTENT_TYPES_MAX_BYTES + 1)
        .read_to_string(&mut xml)
    else {
        return false;
    };
    if read_bytes as u64 > DOCX_CONTENT_TYPES_MAX_BYTES
        || !has_docx_main_document_override(xml.trim_start_matches('\u{feff}'))
    {
        return false;
    }
    drop(content_types);

    let Ok(mut relationships) = archive.by_name("_rels/.rels") else {
        return false;
    };
    if relationships.size() > DOCX_ROOT_RELATIONSHIPS_MAX_BYTES {
        return false;
    }
    let mut xml = String::new();
    let Ok(read_bytes) = relationships
        .by_ref()
        .take(DOCX_ROOT_RELATIONSHIPS_MAX_BYTES + 1)
        .read_to_string(&mut xml)
    else {
        return false;
    };
    read_bytes as u64 <= DOCX_ROOT_RELATIONSHIPS_MAX_BYTES
        && has_main_document_relationship(xml.trim_start_matches('\u{feff}'))
}

fn bounded_expanded_total(current_total: u64, entry_size: u64) -> Option<u64> {
    if entry_size > DOCX_MAX_ENTRY_BYTES {
        return None;
    }
    current_total
        .checked_add(entry_size)
        .filter(|total| *total <= DOCX_MAX_EXPANDED_BYTES)
}

fn is_supported_docx_entry(
    encrypted: bool,
    symlink: bool,
    compression: CompressionMethod,
    compressed_size: u64,
) -> bool {
    !encrypted
        && !symlink
        && matches!(
            compression,
            CompressionMethod::Stored | CompressionMethod::Deflated
        )
        && compressed_size <= PREVIEW_MAX_BYTES as u64
}

fn has_docx_main_document_override(xml: &str) -> bool {
    let Some(document) = parse_bounded_xml(xml) else {
        return false;
    };
    let root = document.root_element();
    root.tag_name().name() == "Types"
        && root.tag_name().namespace() == Some(CONTENT_TYPES_NAMESPACE)
        && root.children().any(|node| {
            node.is_element()
                && node.tag_name().name() == "Override"
                && node.tag_name().namespace() == Some(CONTENT_TYPES_NAMESPACE)
                && node.attribute("PartName") == Some("/word/document.xml")
                && node.attribute("ContentType") == Some(DOCX_MAIN_DOCUMENT_MEDIA_TYPE)
        })
}

fn has_main_document_relationship(xml: &str) -> bool {
    let Some(document) = parse_bounded_xml(xml) else {
        return false;
    };
    let root = document.root_element();
    let namespace = root.tag_name().namespace();
    root.tag_name().name() == "Relationships"
        && matches!(
            namespace,
            Some(PACKAGE_RELATIONSHIPS_NAMESPACE | STRICT_PACKAGE_RELATIONSHIPS_NAMESPACE)
        )
        && root.children().any(|node| {
            let relationship_type = node.attribute("Type");
            node.is_element()
                && node.tag_name().name() == "Relationship"
                && node.tag_name().namespace() == namespace
                && matches!(
                    relationship_type,
                    Some(OFFICE_DOCUMENT_RELATIONSHIP_TYPE)
                        | Some(STRICT_OFFICE_DOCUMENT_RELATIONSHIP_TYPE)
                )
                && matches!(
                    node.attribute("Target"),
                    Some("word/document.xml" | "/word/document.xml")
                )
                && node.attribute("TargetMode").is_none()
        })
}

fn parse_bounded_xml(xml: &str) -> Option<Document<'_>> {
    if xml.contains("<!DOCTYPE") || xml.contains("<!ENTITY") {
        return None;
    }
    Document::parse_with_options(
        xml,
        ParsingOptions {
            allow_dtd: false,
            nodes_limit: DOCX_XML_MAX_NODES,
            entity_resolver: None,
        },
    )
    .ok()
}

fn is_canonical_zip_path(name: &str) -> bool {
    if name.is_empty()
        || name.starts_with('/')
        || name.contains(['\\', '\0'])
        || name.ends_with("//")
    {
        return false;
    }

    let path = name.strip_suffix('/').unwrap_or(name);
    !path.is_empty()
        && path
            .split('/')
            .all(|component| !component.is_empty() && !matches!(component, "." | ".."))
}

pub fn is_preview_size_allowed(byte_len: i64) -> bool {
    byte_len > 0 && byte_len <= PREVIEW_MAX_BYTES
}

pub fn is_previewable_image_type(media_type: &str) -> bool {
    matches!(
        media_type,
        "image/png" | "image/jpeg" | "image/webp" | "image/avif" | "image/gif"
    )
}

pub fn is_previewable_pdf_type(media_type: &str) -> bool {
    media_type == PDF_MEDIA_TYPE
}

pub fn is_previewable_docx_type(media_type: &str) -> bool {
    media_type == DOCX_MEDIA_TYPE
}

pub fn audio_preview_media_type(
    declared_media_type: &str,
    detected_media_type: &str,
) -> Option<String> {
    if detected_media_type.starts_with("audio/") {
        return Some(detected_media_type.to_owned());
    }

    let declared_media_type = declared_media_type
        .split_once(';')
        .map_or(declared_media_type, |(essence, _)| essence)
        .trim();
    match detected_media_type {
        "video/webm" if declared_media_type.eq_ignore_ascii_case("audio/webm") => {
            Some("audio/webm".to_owned())
        }
        "video/mp4" if declared_media_type.eq_ignore_ascii_case("audio/mp4") => {
            Some("audio/mp4".to_owned())
        }
        _ => None,
    }
}

pub fn file_preview_kind(
    byte_len: i64,
    encryption_mode: FileEncryptionMode,
    detected_media_type: Option<&str>,
) -> Option<FilePreviewKind> {
    if encryption_mode != FileEncryptionMode::None || !is_preview_size_allowed(byte_len) {
        return None;
    }

    match detected_media_type {
        Some(media_type) if is_previewable_image_type(media_type) => Some(FilePreviewKind::Image),
        Some(media_type) if is_previewable_pdf_type(media_type) => Some(FilePreviewKind::Pdf),
        Some(media_type) if is_previewable_docx_type(media_type) => Some(FilePreviewKind::Docx),
        _ => None,
    }
}

pub fn file_media_kind(
    declared_media_type: &str,
    detected_media_type: Option<&str>,
) -> FileMediaKind {
    match detected_media_type {
        Some(media_type) if media_type.starts_with("image/") => FileMediaKind::Image,
        Some(media_type) if is_previewable_pdf_type(media_type) => FileMediaKind::Pdf,
        Some(media_type) if audio_preview_media_type(declared_media_type, media_type).is_some() => {
            FileMediaKind::Audio
        }
        None if declared_media_type.starts_with("audio/") => FileMediaKind::Audio,
        _ => FileMediaKind::Other,
    }
}

#[cfg(test)]
mod tests {
    use std::io::{Cursor, Write};

    use notegate_model::FileEncryptionMode;
    use zip::write::SimpleFileOptions;
    use zip::{CompressionMethod, ZipWriter};

    use super::{
        DOCX_CONTENT_TYPES_MAX_BYTES, DOCX_MAIN_DOCUMENT_MEDIA_TYPE, DOCX_MAX_ENTRIES,
        DOCX_MAX_ENTRY_BYTES, DOCX_MAX_EXPANDED_BYTES, DOCX_MEDIA_TYPE, FileMediaKind,
        FilePreviewKind, PREVIEW_MAX_BYTES, ZIP_MEDIA_TYPE, audio_preview_media_type,
        bounded_expanded_total, detect_media_type_from_bytes, file_media_kind, file_preview_kind,
        is_docx_candidate, is_docx_package, is_preview_size_allowed, is_previewable_docx_type,
        is_previewable_image_type, is_previewable_pdf_type, is_supported_docx_entry,
    };

    const PACKAGE_RELATIONSHIPS: &[u8] = br#"<?xml version="1.0"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
  <Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="word/document.xml"/>
</Relationships>"#;
    const PACKAGE_RELATIONSHIPS_WITH_EXTERNAL_LINK: &[u8] = br#"<?xml version="1.0"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
  <Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="word/document.xml"/>
  <Relationship Id="rId2" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/hyperlink" Target="https://example.test" TargetMode="External"/>
</Relationships>"#;
    const MAIN_DOCUMENT: &[u8] = br#"<?xml version="1.0"?>
<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"/>"#;

    type TestResult = Result<(), Box<dyn std::error::Error>>;

    fn content_types(main_content_type: &str) -> String {
        format!(
            r#"<?xml version="1.0" encoding="UTF-8"?>
<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types">
  <Override ContentType="{main_content_type}" PartName="/word/document.xml"/>
</Types>"#
        )
    }

    fn docx_bytes(extra_entries: &[(&str, &[u8])]) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
        let content_types = content_types(DOCX_MAIN_DOCUMENT_MEDIA_TYPE);
        docx_bytes_with_xml(
            content_types.as_bytes(),
            PACKAGE_RELATIONSHIPS,
            extra_entries,
        )
    }

    fn docx_bytes_with_xml(
        content_types: &[u8],
        package_relationships: &[u8],
        extra_entries: &[(&str, &[u8])],
    ) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
        archive_bytes(
            &[
                ("[Content_Types].xml", content_types),
                ("_rels/.rels", package_relationships),
                ("word/document.xml", MAIN_DOCUMENT),
            ],
            extra_entries,
        )
    }

    fn archive_bytes(
        required_entries: &[(&str, &[u8])],
        extra_entries: &[(&str, &[u8])],
    ) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
        let cursor = Cursor::new(Vec::new());
        let mut writer = ZipWriter::new(cursor);
        let options = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);
        for (name, bytes) in required_entries.iter().chain(extra_entries) {
            writer.start_file(*name, options)?;
            writer.write_all(bytes)?;
        }
        Ok(writer.finish()?.into_inner())
    }

    fn docx_with_empty_entries(count: usize) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
        let cursor = Cursor::new(Vec::new());
        let mut writer = ZipWriter::new(cursor);
        let options = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);
        for (name, bytes) in [
            (
                "[Content_Types].xml",
                content_types(DOCX_MAIN_DOCUMENT_MEDIA_TYPE).into_bytes(),
            ),
            ("_rels/.rels", PACKAGE_RELATIONSHIPS.to_vec()),
            ("word/document.xml", MAIN_DOCUMENT.to_vec()),
        ] {
            writer.start_file(name, options)?;
            writer.write_all(&bytes)?;
        }
        for index in 0..count {
            writer.start_file(format!("word/media/{index}.bin"), options)?;
        }
        Ok(writer.finish()?.into_inner())
    }

    #[test]
    fn only_safe_raster_image_types_are_previewable() {
        for media_type in [
            "image/png",
            "image/jpeg",
            "image/webp",
            "image/avif",
            "image/gif",
        ] {
            assert!(is_previewable_image_type(media_type), "{media_type}");
        }
        for media_type in [
            "image/svg+xml",
            "application/pdf",
            "text/html",
            "application/octet-stream",
        ] {
            assert!(!is_previewable_image_type(media_type), "{media_type}");
        }
    }

    #[test]
    fn only_pdf_type_is_previewable_pdf() {
        assert!(is_previewable_pdf_type("application/pdf"));
        for media_type in ["image/png", "text/html", "application/octet-stream"] {
            assert!(!is_previewable_pdf_type(media_type), "{media_type}");
        }
    }

    #[test]
    fn only_exact_docx_type_is_previewable_docx() {
        assert!(is_previewable_docx_type(DOCX_MEDIA_TYPE));
        for media_type in [
            ZIP_MEDIA_TYPE,
            "application/msword",
            "text/html",
            "application/octet-stream",
        ] {
            assert!(!is_previewable_docx_type(media_type), "{media_type}");
        }
    }

    #[test]
    fn only_inferred_declared_or_named_docx_zip_candidates_require_a_full_read() {
        let zip_prefix = b"PK\x03\x04placeholder";
        assert!(is_docx_candidate(
            zip_prefix,
            "application/zip",
            None,
            DOCX_MEDIA_TYPE
        ));
        assert!(is_docx_candidate(
            zip_prefix,
            " application/vnd.openxmlformats-officedocument.wordprocessingml.document; charset=binary ",
            None,
            ZIP_MEDIA_TYPE
        ));
        assert!(is_docx_candidate(
            zip_prefix,
            "application/octet-stream",
            Some("meeting.DOCX"),
            ZIP_MEDIA_TYPE
        ));
        assert!(!is_docx_candidate(
            zip_prefix,
            "application/zip",
            Some("archive.zip"),
            ZIP_MEDIA_TYPE
        ));
        assert!(!is_docx_candidate(
            b"not a zip",
            DOCX_MEDIA_TYPE,
            Some("document.docx"),
            DOCX_MEDIA_TYPE
        ));
    }

    #[test]
    fn docx_detection_requires_a_valid_package_and_exact_main_override() -> TestResult {
        let valid = docx_bytes(&[])?;
        assert!(is_docx_package(&valid));
        assert_eq!(detect_media_type_from_bytes(&valid), DOCX_MEDIA_TYPE);

        let wrong_override = content_types(DOCX_MEDIA_TYPE);
        let wrong = archive_bytes(
            &[
                ("[Content_Types].xml", wrong_override.as_bytes()),
                ("_rels/.rels", PACKAGE_RELATIONSHIPS),
                ("word/document.xml", MAIN_DOCUMENT),
            ],
            &[],
        )?;
        assert!(!is_docx_package(&wrong));
        assert_eq!(detect_media_type_from_bytes(&wrong), ZIP_MEDIA_TYPE);

        let missing_main_relationship = archive_bytes(
            &[
                (
                    "[Content_Types].xml",
                    content_types(DOCX_MAIN_DOCUMENT_MEDIA_TYPE).as_bytes(),
                ),
                (
                    "_rels/.rels",
                    br#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"/>"#,
                ),
                ("word/document.xml", MAIN_DOCUMENT),
            ],
            &[],
        )?;
        assert!(!is_docx_package(&missing_main_relationship));

        let external_link = archive_bytes(
            &[
                (
                    "[Content_Types].xml",
                    content_types(DOCX_MAIN_DOCUMENT_MEDIA_TYPE).as_bytes(),
                ),
                ("_rels/.rels", PACKAGE_RELATIONSHIPS_WITH_EXTERNAL_LINK),
                ("word/document.xml", MAIN_DOCUMENT),
            ],
            &[],
        )?;
        assert!(is_docx_package(&external_link));

        let generic_zip = archive_bytes(&[("file.txt", b"not a document")], &[])?;
        assert!(!is_docx_package(&generic_zip));
        assert_eq!(detect_media_type_from_bytes(&generic_zip), ZIP_MEDIA_TYPE);
        assert_ne!(
            detect_media_type_from_bytes(b"<!doctype html><html></html>"),
            DOCX_MEDIA_TYPE
        );
        Ok(())
    }

    #[test]
    fn docx_detection_requires_the_internal_office_document_relationship() -> TestResult {
        let missing_relationship = br#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"/>"#;
        let missing = docx_bytes_with_xml(
            content_types(DOCX_MAIN_DOCUMENT_MEDIA_TYPE).as_bytes(),
            missing_relationship,
            &[],
        )?;
        assert!(!is_docx_package(&missing));

        let external_relationship = br#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
  <Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="https://example.test/document.xml" TargetMode="External"/>
</Relationships>"#;
        let external = docx_bytes_with_xml(
            content_types(DOCX_MAIN_DOCUMENT_MEDIA_TYPE).as_bytes(),
            external_relationship,
            &[],
        )?;
        assert!(!is_docx_package(&external));
        Ok(())
    }

    #[test]
    fn docx_detection_rejects_dtds_duplicate_paths_and_traversal() -> TestResult {
        let dtd = content_types(DOCX_MAIN_DOCUMENT_MEDIA_TYPE).replacen(
            "?>",
            "?>\n<!DOCTYPE Types [<!ENTITY xxe SYSTEM \"file:///etc/passwd\">]>",
            1,
        );
        let with_dtd = docx_bytes_with_xml(dtd.as_bytes(), PACKAGE_RELATIONSHIPS, &[])?;
        assert!(!is_docx_package(&with_dtd));

        let duplicate = docx_bytes(&[("WORD/document.xml", MAIN_DOCUMENT)])?;
        assert!(!is_docx_package(&duplicate));

        let traversal = docx_bytes(&[("word/../escape.bin", b"escape")])?;
        assert!(!is_docx_package(&traversal));
        Ok(())
    }

    #[test]
    fn docx_detection_rejects_encrypted_and_unsupported_compression_entries() {
        assert!(is_supported_docx_entry(
            false,
            false,
            CompressionMethod::Deflated,
            PREVIEW_MAX_BYTES as u64
        ));
        assert!(!is_supported_docx_entry(
            true,
            false,
            CompressionMethod::Deflated,
            1
        ));
        assert!(!is_supported_docx_entry(
            false,
            false,
            CompressionMethod::BZIP2,
            1
        ));
    }

    #[test]
    fn docx_detection_rejects_active_content_parts() -> TestResult {
        for forbidden_name in [
            "word/vbaProject.bin",
            "word/activeX/activeX1.bin",
            "word/embeddings/oleObject1.bin",
        ] {
            let bytes = docx_bytes(&[(forbidden_name, b"active content")])?;
            assert!(!is_docx_package(&bytes), "{forbidden_name}");
            assert_eq!(detect_media_type_from_bytes(&bytes), ZIP_MEDIA_TYPE);
        }
        Ok(())
    }

    #[test]
    fn docx_detection_bounds_entry_count_and_expanded_sizes() -> TestResult {
        let too_many = docx_with_empty_entries(DOCX_MAX_ENTRIES - 2)?;
        assert!(!is_docx_package(&too_many));

        let oversized_content_types = vec![b' '; (DOCX_CONTENT_TYPES_MAX_BYTES + 1) as usize];
        let oversized_manifest =
            docx_bytes_with_xml(&oversized_content_types, PACKAGE_RELATIONSHIPS, &[])?;
        assert!(!is_docx_package(&oversized_manifest));

        assert_eq!(
            bounded_expanded_total(0, DOCX_MAX_ENTRY_BYTES),
            Some(DOCX_MAX_ENTRY_BYTES)
        );
        assert_eq!(bounded_expanded_total(0, DOCX_MAX_ENTRY_BYTES + 1), None);
        assert_eq!(
            bounded_expanded_total(
                DOCX_MAX_EXPANDED_BYTES - DOCX_MAX_ENTRY_BYTES + 1,
                DOCX_MAX_ENTRY_BYTES
            ),
            None
        );
        assert_eq!(bounded_expanded_total(u64::MAX, 1), None);
        Ok(())
    }

    #[test]
    fn preview_size_is_limited_to_ten_mib() {
        assert!(!is_preview_size_allowed(0));
        assert!(is_preview_size_allowed(PREVIEW_MAX_BYTES));
        assert!(!is_preview_size_allowed(PREVIEW_MAX_BYTES + 1));
    }

    #[test]
    fn file_preview_kind_uses_verified_media_type_and_file_policy() {
        assert_eq!(
            file_preview_kind(1024, FileEncryptionMode::None, Some("image/png")),
            Some(FilePreviewKind::Image)
        );
        assert_eq!(
            file_preview_kind(1024, FileEncryptionMode::None, Some("application/pdf")),
            Some(FilePreviewKind::Pdf)
        );
        assert_eq!(
            file_preview_kind(1024, FileEncryptionMode::None, Some(DOCX_MEDIA_TYPE)),
            Some(FilePreviewKind::Docx)
        );
        assert_eq!(
            file_preview_kind(1024, FileEncryptionMode::Client, Some(DOCX_MEDIA_TYPE)),
            None
        );
        assert_eq!(
            file_preview_kind(
                PREVIEW_MAX_BYTES + 1,
                FileEncryptionMode::None,
                Some(DOCX_MEDIA_TYPE)
            ),
            None
        );
        assert_eq!(
            file_preview_kind(1024, FileEncryptionMode::Client, Some("application/pdf")),
            None
        );
        assert_eq!(
            file_preview_kind(
                PREVIEW_MAX_BYTES + 1,
                FileEncryptionMode::None,
                Some("application/pdf")
            ),
            None
        );
        assert_eq!(
            file_preview_kind(1024, FileEncryptionMode::None, Some("text/html")),
            None
        );
        assert_eq!(
            file_preview_kind(1024, FileEncryptionMode::None, None),
            None
        );
    }

    #[test]
    fn file_media_kind_prefers_detection_and_recognizes_browser_audio_containers() {
        assert_eq!(
            file_media_kind("application/octet-stream", Some("audio/mpeg")),
            FileMediaKind::Audio
        );
        assert_eq!(
            file_media_kind("audio/mp4", Some("video/mp4")),
            FileMediaKind::Audio
        );
        assert_eq!(
            file_media_kind("audio/webm;codecs=opus", Some("video/webm")),
            FileMediaKind::Audio
        );
        assert_eq!(
            file_media_kind("video/mp4", Some("video/mp4")),
            FileMediaKind::Other
        );
        assert_eq!(
            file_media_kind("audio/mp4", Some("text/html")),
            FileMediaKind::Other
        );
        assert_eq!(
            file_media_kind("image/png", Some("image/png")),
            FileMediaKind::Image
        );
        assert_eq!(
            file_media_kind("application/pdf", Some("application/pdf")),
            FileMediaKind::Pdf
        );
        assert_eq!(file_media_kind("audio/mp4", None), FileMediaKind::Audio);
    }

    #[test]
    fn audio_preview_uses_verified_types_and_exact_browser_container_pairs() {
        assert_eq!(
            audio_preview_media_type("application/octet-stream", "audio/mpeg"),
            Some("audio/mpeg".to_owned())
        );
        assert_eq!(
            audio_preview_media_type("audio/webm;codecs=opus", "video/webm"),
            Some("audio/webm".to_owned())
        );
        assert_eq!(
            audio_preview_media_type("audio/mp4", "video/mp4"),
            Some("audio/mp4".to_owned())
        );
        assert_eq!(audio_preview_media_type("audio/ogg", "video/webm"), None);
        assert_eq!(audio_preview_media_type("audio/webm", "text/html"), None);
        assert_eq!(audio_preview_media_type("video/webm", "video/webm"), None);
    }
}
