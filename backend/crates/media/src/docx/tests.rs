use super::*;
use crate::detect::ZIP_MEDIA_TYPE;
use crate::{DOCX_MEDIA_TYPE, detect_media_type_from_bytes, is_docx_candidate};
use std::io::{Cursor, Write};
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipArchive, ZipWriter};

fn is_docx_package(bytes: &[u8]) -> bool {
    validate_docx_package(bytes).is_ok()
}

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
    archive_bytes_with_compression(required_entries, extra_entries, CompressionMethod::Deflated)
}

fn archive_bytes_with_compression(
    required_entries: &[(&str, &[u8])],
    extra_entries: &[(&str, &[u8])],
    compression: CompressionMethod,
) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    let cursor = Cursor::new(Vec::new());
    let mut writer = ZipWriter::new(cursor);
    let options = SimpleFileOptions::default().compression_method(compression);
    for (name, bytes) in required_entries.iter().chain(extra_entries) {
        writer.start_file(*name, options)?;
        writer.write_all(bytes)?;
    }
    Ok(writer.finish()?.into_inner())
}

fn docx_bytes_with_compression(
    extra_entries: &[(&str, &[u8])],
    compression: CompressionMethod,
) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    let content_types = content_types(DOCX_MAIN_DOCUMENT_MEDIA_TYPE);
    archive_bytes_with_compression(
        &[
            ("[Content_Types].xml", content_types.as_bytes()),
            ("_rels/.rels", PACKAGE_RELATIONSHIPS),
            ("word/document.xml", MAIN_DOCUMENT),
        ],
        extra_entries,
        compression,
    )
}

fn central_header_offset(bytes: &[u8], target_name: &str) -> Option<usize> {
    for (offset, signature) in bytes.windows(4).enumerate() {
        if signature != b"PK\x01\x02" {
            continue;
        }
        let name_length = bytes
            .get(offset.checked_add(28)?..offset.checked_add(30)?)?
            .try_into()
            .ok()
            .map(u16::from_le_bytes)?;
        let name_start = offset.checked_add(46)?;
        let name_end = name_start.checked_add(usize::from(name_length))?;
        if bytes.get(name_start..name_end) == Some(target_name.as_bytes()) {
            return Some(offset);
        }
    }
    None
}

fn patch_declared_size(bytes: &mut [u8], target_name: &str, size: u32) -> TestResult {
    let offset = central_header_offset(bytes, target_name)
        .ok_or_else(|| std::io::Error::other("central entry not found"))?;
    let size_start = offset
        .checked_add(24)
        .ok_or_else(|| std::io::Error::other("central entry offset overflow"))?;
    let size_end = size_start
        .checked_add(4)
        .ok_or_else(|| std::io::Error::other("central entry size overflow"))?;
    bytes
        .get_mut(size_start..size_end)
        .ok_or_else(|| std::io::Error::other("central entry size is truncated"))?
        .copy_from_slice(&size.to_le_bytes());
    Ok(())
}

fn central_local_header_offset(bytes: &[u8], target_name: &str) -> Option<u32> {
    let offset = central_header_offset(bytes, target_name)?;
    let offset_start = offset.checked_add(42)?;
    let offset_end = offset_start.checked_add(4)?;
    bytes
        .get(offset_start..offset_end)?
        .try_into()
        .ok()
        .map(u32::from_le_bytes)
}

fn patch_local_header_offset(
    bytes: &mut [u8],
    target_name: &str,
    local_header_offset: u32,
) -> TestResult {
    let offset = central_header_offset(bytes, target_name)
        .ok_or_else(|| std::io::Error::other("central entry not found"))?;
    let offset_start = offset
        .checked_add(42)
        .ok_or_else(|| std::io::Error::other("central entry offset overflow"))?;
    let offset_end = offset_start
        .checked_add(4)
        .ok_or_else(|| std::io::Error::other("central entry size overflow"))?;
    bytes
        .get_mut(offset_start..offset_end)
        .ok_or_else(|| std::io::Error::other("central entry offset is truncated"))?
        .copy_from_slice(&local_header_offset.to_le_bytes());
    Ok(())
}

fn corrupt_entry_data(bytes: &mut [u8], target_name: &str) -> TestResult {
    let data_start = {
        let mut archive = ZipArchive::new(Cursor::new(&*bytes))?;
        let entry = archive.by_name(target_name)?;
        entry
            .data_start()
            .ok_or_else(|| std::io::Error::other("entry data offset unavailable"))?
    };
    let data_start = usize::try_from(data_start)?;
    let byte = bytes
        .get_mut(data_start)
        .ok_or_else(|| std::io::Error::other("entry data is truncated"))?;
    *byte ^= 1;
    Ok(())
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
    assert_eq!(detect_media_type_from_bytes(&valid).docx_rejection, None);
    assert_eq!(
        detect_media_type_from_bytes(&valid).media_type,
        DOCX_MEDIA_TYPE
    );

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
    assert_eq!(
        detect_media_type_from_bytes(&wrong).docx_rejection,
        Some(DocxRejection::InvalidContentTypes)
    );
    assert_eq!(
        detect_media_type_from_bytes(&wrong).media_type,
        ZIP_MEDIA_TYPE
    );

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
    assert_eq!(
        detect_media_type_from_bytes(&generic_zip).media_type,
        ZIP_MEDIA_TYPE
    );
    assert_ne!(
        detect_media_type_from_bytes(b"<!doctype html><html></html>").media_type,
        DOCX_MEDIA_TYPE
    );
    Ok(())
}

#[test]
fn docx_detection_requires_the_internal_office_document_relationship() -> TestResult {
    let missing_relationship =
        br#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"/>"#;
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
fn docx_entry_validation_rejects_prohibited_features_and_size() {
    assert!(
        validate_supported_docx_entry(
            false,
            false,
            CompressionMethod::Deflated,
            DOCX_MAX_COMPRESSED_ENTRY_BYTES
        )
        .is_ok()
    );
    assert_eq!(
        validate_supported_docx_entry(true, false, CompressionMethod::Deflated, 1),
        Err(DocxRejection::EncryptedEntry)
    );
    assert_eq!(
        validate_supported_docx_entry(false, true, CompressionMethod::Deflated, 1),
        Err(DocxRejection::SymlinkEntry)
    );
    assert_eq!(
        validate_supported_docx_entry(false, false, CompressionMethod::BZIP2, 1),
        Err(DocxRejection::UnsupportedCompression)
    );
    assert_eq!(
        validate_supported_docx_entry(
            false,
            false,
            CompressionMethod::Deflated,
            DOCX_MAX_COMPRESSED_ENTRY_BYTES + 1,
        ),
        Err(DocxRejection::CompressedEntryTooLarge)
    );
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
        assert_eq!(
            detect_media_type_from_bytes(&bytes).media_type,
            ZIP_MEDIA_TYPE
        );
    }
    Ok(())
}

#[test]
fn docx_validation_rejects_overlapping_entry_payloads_before_reading() -> TestResult {
    let shared_payload = b"same payload";
    let mut bytes = docx_bytes_with_compression(
        &[
            ("word/media/first.bin", shared_payload),
            ("word/media/second.bin", shared_payload),
        ],
        CompressionMethod::Stored,
    )?;
    let first_offset = central_local_header_offset(&bytes, "word/media/first.bin")
        .ok_or_else(|| std::io::Error::other("first local header not found"))?;
    patch_local_header_offset(&mut bytes, "word/media/second.bin", first_offset)?;

    assert_eq!(
        validate_docx_package(&bytes),
        Err(DocxRejection::OverlappingEntries)
    );
    Ok(())
}

#[test]
fn docx_validation_reads_unretained_entries_through_crc_eof() -> TestResult {
    let mut bytes = docx_bytes_with_compression(
        &[("word/media/image.bin", b"crc protected payload")],
        CompressionMethod::Stored,
    )?;
    corrupt_entry_data(&mut bytes, "word/media/image.bin")?;

    assert_eq!(validate_docx_package(&bytes), Err(DocxRejection::EntryRead));
    Ok(())
}

#[test]
fn docx_validation_rejects_underdeclared_entry_after_actual_limit() -> TestResult {
    let payload = vec![0_u8; (DOCX_MAX_ENTRY_BYTES + 1) as usize];
    let mut bytes = docx_bytes(&[("word/media/large.bin", &payload)])?;
    patch_declared_size(&mut bytes, "word/media/large.bin", 1)?;

    assert_eq!(
        validate_docx_package(&bytes),
        Err(DocxRejection::ActualEntryTooLarge)
    );
    Ok(())
}

#[test]
fn docx_validation_rejects_underdeclared_aggregate_after_actual_limit() -> TestResult {
    let payload = vec![0_u8; (DOCX_MAX_EXPANDED_BYTES / 3 + 1) as usize];
    let names = [
        "word/media/first.bin",
        "word/media/second.bin",
        "word/media/third.bin",
    ];
    let mut bytes = docx_bytes(&[
        (names[0], &payload),
        (names[1], &payload),
        (names[2], &payload),
    ])?;
    for name in names {
        patch_declared_size(&mut bytes, name, 1)?;
    }

    assert_eq!(
        validate_docx_package(&bytes),
        Err(DocxRejection::ActualArchiveTooLarge)
    );
    Ok(())
}

#[test]
fn docx_validation_rejects_declared_actual_size_mismatch() -> TestResult {
    let mut bytes = docx_bytes(&[("word/media/data.bin", b"actual bytes")])?;
    patch_declared_size(&mut bytes, "word/media/data.bin", 1)?;

    assert_eq!(
        validate_docx_package(&bytes),
        Err(DocxRejection::DeclaredSizeMismatch)
    );
    Ok(())
}

#[test]
fn docx_validation_preflights_forbidden_entries_before_decompression() -> TestResult {
    let mut bytes = docx_bytes_with_compression(
        &[("word/vbaProject.bin", b"macro payload")],
        CompressionMethod::Stored,
    )?;
    corrupt_entry_data(&mut bytes, "word/vbaProject.bin")?;

    assert_eq!(
        validate_docx_package(&bytes),
        Err(DocxRejection::ForbiddenEntry)
    );
    Ok(())
}

#[test]
fn docx_validation_rejects_directory_entries_with_payloads() -> TestResult {
    let bytes = docx_bytes_with_compression(
        &[("word/media/", b"hidden payload")],
        CompressionMethod::Stored,
    )?;

    assert_eq!(
        validate_docx_package(&bytes),
        Err(DocxRejection::InvalidEntryMetadata)
    );
    Ok(())
}

#[test]
fn docx_validation_accepts_entry_at_actual_expansion_limit() -> TestResult {
    let payload = vec![0_u8; DOCX_MAX_ENTRY_BYTES as usize];
    let bytes = docx_bytes(&[("word/media/large.bin", &payload)])?;

    assert_eq!(validate_docx_package(&bytes), Ok(()));
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
