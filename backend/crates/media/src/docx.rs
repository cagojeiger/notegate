use std::collections::HashSet;
use std::io::{Cursor, Read};
use std::path::Path;

use roxmltree::{Document, ParsingOptions};
use zip::{CompressionMethod, ZipArchive};

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

const DOCX_MAX_ENTRIES: usize = 2_048;

const DOCX_MAX_ENTRY_BYTES: u64 = 32 * 1024 * 1024;

const DOCX_MAX_EXPANDED_BYTES: u64 = 64 * 1024 * 1024;

const DOCX_CONTENT_TYPES_MAX_BYTES: u64 = 64 * 1024;

const DOCX_ROOT_RELATIONSHIPS_MAX_BYTES: u64 = 64 * 1024;

const DOCX_XML_MAX_NODES: u32 = 4_096;

const DOCX_MAX_COMPRESSED_ENTRY_BYTES: u64 = 10 * 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DocxRejection {
    InvalidArchive,
    NonzeroArchiveOffset,
    EmptyArchive,
    TooManyEntries,
    OverlappingEntries,
    InvalidEntryMetadata,
    EncryptedEntry,
    SymlinkEntry,
    UnsupportedCompression,
    CompressedEntryTooLarge,
    NoncanonicalPath,
    DuplicatePath,
    ForbiddenEntry,
    DeclaredEntryTooLarge,
    DeclaredArchiveTooLarge,
    MissingRequiredPart,
    RequiredXmlTooLarge,
    EntryOpen,
    EntryRead,
    ActualEntryTooLarge,
    ActualArchiveTooLarge,
    DeclaredSizeMismatch,
    InvalidXmlEncoding,
    InvalidContentTypes,
    InvalidRelationships,
}

#[derive(Default)]
struct RequiredParts {
    has_content_types: bool,
    has_package_relationships: bool,
    has_main_document: bool,
}

#[derive(Clone, Copy)]
enum RetainedXml {
    ContentTypes,
    PackageRelationships,
}

pub(crate) fn validate_docx_package(bytes: &[u8]) -> Result<(), DocxRejection> {
    let mut archive =
        ZipArchive::new(Cursor::new(bytes)).map_err(|_| DocxRejection::InvalidArchive)?;
    if archive.offset() != 0 || archive.is_empty() || archive.len() > DOCX_MAX_ENTRIES {
        return Err(if archive.offset() != 0 {
            DocxRejection::NonzeroArchiveOffset
        } else if archive.is_empty() {
            DocxRejection::EmptyArchive
        } else {
            DocxRejection::TooManyEntries
        });
    }
    if archive
        .has_overlapping_files()
        .map_err(|_| DocxRejection::InvalidArchive)?
    {
        return Err(DocxRejection::OverlappingEntries);
    }

    let mut total_declared_bytes = 0_u64;
    let mut normalized_paths = HashSet::with_capacity(archive.len());
    let mut required_parts = RequiredParts::default();

    for index in 0..archive.len() {
        let entry = archive
            .by_index_raw(index)
            .map_err(|_| DocxRejection::InvalidEntryMetadata)?;
        let name = entry.name();
        let normalized_path = name.strip_suffix('/').unwrap_or(name).to_ascii_lowercase();
        validate_supported_docx_entry(
            entry.encrypted(),
            entry.is_symlink(),
            entry.compression(),
            entry.compressed_size(),
        )?;
        if entry.is_dir() && (entry.size() != 0 || entry.compressed_size() != 0) {
            return Err(DocxRejection::InvalidEntryMetadata);
        }
        if entry.enclosed_name().as_deref() != Some(Path::new(name)) || !is_canonical_zip_path(name)
        {
            return Err(DocxRejection::NoncanonicalPath);
        }
        if !normalized_paths.insert(normalized_path) {
            return Err(DocxRejection::DuplicatePath);
        }
        total_declared_bytes = bounded_expanded_total(total_declared_bytes, entry.size()).ok_or(
            if entry.size() > DOCX_MAX_ENTRY_BYTES {
                DocxRejection::DeclaredEntryTooLarge
            } else {
                DocxRejection::DeclaredArchiveTooLarge
            },
        )?;

        let lowercase_name = name.to_ascii_lowercase();
        if lowercase_name == "word/vbaproject.bin"
            || lowercase_name.starts_with("word/activex/")
            || lowercase_name.starts_with("word/embeddings/")
        {
            return Err(DocxRejection::ForbiddenEntry);
        }

        match name {
            "[Content_Types].xml" => {
                if entry.size() > DOCX_CONTENT_TYPES_MAX_BYTES {
                    return Err(DocxRejection::RequiredXmlTooLarge);
                }
                required_parts.has_content_types = true;
            }
            "_rels/.rels" => {
                if entry.size() > DOCX_ROOT_RELATIONSHIPS_MAX_BYTES {
                    return Err(DocxRejection::RequiredXmlTooLarge);
                }
                required_parts.has_package_relationships = true;
            }
            "word/document.xml" => required_parts.has_main_document = true,
            _ => {}
        }
    }

    if !required_parts.has_content_types
        || !required_parts.has_package_relationships
        || !required_parts.has_main_document
    {
        return Err(DocxRejection::MissingRequiredPart);
    }

    let mut total_actual_bytes = 0_u64;
    let mut content_types = None;
    let mut package_relationships = None;
    let mut has_declared_size_mismatch = false;
    let mut buffer = [0_u8; 8 * 1024];
    let buffer_len = u64::try_from(buffer.len()).map_err(|_| DocxRejection::ActualEntryTooLarge)?;
    for index in 0..archive.len() {
        let mut entry = archive
            .by_index(index)
            .map_err(|_| DocxRejection::EntryOpen)?;
        if entry.is_dir() {
            continue;
        }
        let declared_size = entry.size();
        let retained_xml = match entry.name() {
            "[Content_Types].xml" => Some(RetainedXml::ContentTypes),
            "_rels/.rels" => Some(RetainedXml::PackageRelationships),
            _ => None,
        };
        let mut retained_bytes = retained_xml.map(|_| Vec::new());
        let mut entry_actual_bytes = 0_u64;
        loop {
            let mut remaining = DOCX_MAX_ENTRY_BYTES
                .saturating_sub(entry_actual_bytes)
                .min(DOCX_MAX_EXPANDED_BYTES.saturating_sub(total_actual_bytes));
            if let Some(retained_xml) = retained_xml {
                let xml_limit = match retained_xml {
                    RetainedXml::ContentTypes => DOCX_CONTENT_TYPES_MAX_BYTES,
                    RetainedXml::PackageRelationships => DOCX_ROOT_RELATIONSHIPS_MAX_BYTES,
                };
                remaining = remaining.min(xml_limit.saturating_sub(entry_actual_bytes));
            }
            let read_limit = usize::try_from(remaining.saturating_add(1).min(buffer_len))
                .map_err(|_| DocxRejection::ActualEntryTooLarge)?;
            let read_buffer = buffer
                .get_mut(..read_limit)
                .ok_or(DocxRejection::ActualEntryTooLarge)?;
            let read_bytes = entry
                .read(read_buffer)
                .map_err(|_| DocxRejection::EntryRead)?;
            if read_bytes == 0 {
                break;
            }
            let read_bytes_usize = read_bytes;
            let read_bytes =
                u64::try_from(read_bytes).map_err(|_| DocxRejection::ActualEntryTooLarge)?;
            entry_actual_bytes = entry_actual_bytes
                .checked_add(read_bytes)
                .filter(|size| *size <= DOCX_MAX_ENTRY_BYTES)
                .ok_or(DocxRejection::ActualEntryTooLarge)?;
            total_actual_bytes = total_actual_bytes
                .checked_add(read_bytes)
                .filter(|size| *size <= DOCX_MAX_EXPANDED_BYTES)
                .ok_or(DocxRejection::ActualArchiveTooLarge)?;

            if let Some(retained_bytes) = &mut retained_bytes {
                let xml_limit = match retained_xml {
                    Some(RetainedXml::ContentTypes) => DOCX_CONTENT_TYPES_MAX_BYTES,
                    Some(RetainedXml::PackageRelationships) => DOCX_ROOT_RELATIONSHIPS_MAX_BYTES,
                    None => 0,
                };
                if entry_actual_bytes > xml_limit {
                    return Err(DocxRejection::RequiredXmlTooLarge);
                }
                let read_chunk = buffer
                    .get(..read_bytes_usize)
                    .ok_or(DocxRejection::ActualEntryTooLarge)?;
                retained_bytes.extend_from_slice(read_chunk);
            }
        }
        if entry_actual_bytes != declared_size {
            has_declared_size_mismatch = true;
        }
        match (retained_xml, retained_bytes) {
            (Some(RetainedXml::ContentTypes), Some(bytes)) => content_types = Some(bytes),
            (Some(RetainedXml::PackageRelationships), Some(bytes)) => {
                package_relationships = Some(bytes);
            }
            _ => {}
        }
    }
    if has_declared_size_mismatch {
        return Err(DocxRejection::DeclaredSizeMismatch);
    }

    let Some(content_types) = content_types else {
        return Err(DocxRejection::MissingRequiredPart);
    };
    let Some(package_relationships) = package_relationships else {
        return Err(DocxRejection::MissingRequiredPart);
    };
    let content_types =
        std::str::from_utf8(&content_types).map_err(|_| DocxRejection::InvalidXmlEncoding)?;
    if !has_docx_main_document_override(content_types.trim_start_matches('\u{feff}')) {
        return Err(DocxRejection::InvalidContentTypes);
    }
    let package_relationships = std::str::from_utf8(&package_relationships)
        .map_err(|_| DocxRejection::InvalidXmlEncoding)?;
    if !has_main_document_relationship(package_relationships.trim_start_matches('\u{feff}')) {
        return Err(DocxRejection::InvalidRelationships);
    }
    Ok(())
}

fn bounded_expanded_total(current_total: u64, entry_size: u64) -> Option<u64> {
    if entry_size > DOCX_MAX_ENTRY_BYTES {
        return None;
    }
    current_total
        .checked_add(entry_size)
        .filter(|total| *total <= DOCX_MAX_EXPANDED_BYTES)
}

fn validate_supported_docx_entry(
    encrypted: bool,
    symlink: bool,
    compression: CompressionMethod,
    compressed_size: u64,
) -> Result<(), DocxRejection> {
    if encrypted {
        return Err(DocxRejection::EncryptedEntry);
    }
    if symlink {
        return Err(DocxRejection::SymlinkEntry);
    }
    if !matches!(
        compression,
        CompressionMethod::Stored | CompressionMethod::Deflated
    ) {
        return Err(DocxRejection::UnsupportedCompression);
    }
    if compressed_size > DOCX_MAX_COMPRESSED_ENTRY_BYTES {
        return Err(DocxRejection::CompressedEntryTooLarge);
    }
    Ok(())
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

#[cfg(test)]
mod tests;
