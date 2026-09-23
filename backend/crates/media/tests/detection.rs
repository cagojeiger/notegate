use notegate_media::{DocxRejection, detect_media_type_from_bytes, sniff_media_type};

#[test]
fn non_zip_detection_preserves_known_types_and_unknown_fallback() {
    for (bytes, expected) in [
        (b"%PDF-1.7\n".as_slice(), "application/pdf"),
        (b"\x89PNG\r\n\x1a\n".as_slice(), "image/png"),
        (b"plain text".as_slice(), "application/octet-stream"),
        (b"".as_slice(), "application/octet-stream"),
    ] {
        let detected = detect_media_type_from_bytes(bytes);
        assert_eq!(detected.media_type, expected);
        assert_eq!(detected.docx_rejection, None);
        assert_eq!(
            sniff_media_type(bytes).unwrap_or("application/octet-stream"),
            expected
        );
    }
}

#[test]
fn malformed_zip_retains_zip_type_and_reports_validation_failure() {
    let detected = detect_media_type_from_bytes(b"PK\x03\x04not a complete archive");
    assert_eq!(detected.media_type, "application/zip");
    assert_eq!(detected.docx_rejection, Some(DocxRejection::InvalidArchive));
}
