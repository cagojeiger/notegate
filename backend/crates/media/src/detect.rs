use crate::docx::validate_docx_package;
use crate::{DOCX_MEDIA_TYPE, DocxRejection, UNKNOWN_MEDIA_TYPE};

pub(crate) const ZIP_MEDIA_TYPE: &str = "application/zip";
const ZIP_LOCAL_FILE_SIGNATURE: &[u8; 4] = b"PK\x03\x04";

/// Byte-based classification and the reason a ZIP was not accepted as DOCX.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MediaDetection {
    pub media_type: &'static str,
    pub docx_rejection: Option<DocxRejection>,
}

/// Sniff a prefix without decompressing or validating a DOCX package.
pub fn sniff_media_type(bytes: &[u8]) -> Option<&'static str> {
    infer::get(bytes).map(|kind| kind.mime_type())
}

/// Decide whether a ZIP prefix and its filename/type hints warrant full validation.
pub fn is_docx_candidate(
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

/// Classify complete bytes, validating ZIP packages before accepting DOCX.
/// The caller bounds input size and concurrent work; this function performs no I/O.
pub fn detect_media_type_from_bytes(bytes: &[u8]) -> MediaDetection {
    if bytes.starts_with(ZIP_LOCAL_FILE_SIGNATURE) {
        let reason = match validate_docx_package(bytes) {
            Ok(()) => {
                return MediaDetection {
                    media_type: DOCX_MEDIA_TYPE,
                    docx_rejection: None,
                };
            }
            Err(reason) => reason,
        };
        let inferred = sniff_media_type(bytes).unwrap_or(ZIP_MEDIA_TYPE);
        return MediaDetection {
            media_type: if inferred == DOCX_MEDIA_TYPE {
                ZIP_MEDIA_TYPE
            } else {
                inferred
            },
            docx_rejection: Some(reason),
        };
    }
    MediaDetection {
        media_type: sniff_media_type(bytes).unwrap_or(UNKNOWN_MEDIA_TYPE),
        docx_rejection: None,
    }
}
