use notegate_core::limits;

use crate::files::ReadContent;

/// Slice a document by a 1-based line range and a byte budget, reporting whether
/// the result was truncated and the next start line.
pub(super) fn slice_document(
    content: &str,
    start_line: Option<i64>,
    max_lines: Option<i64>,
    max_bytes: Option<usize>,
) -> ReadContent {
    let start_line = start_line.unwrap_or(1).max(1);
    let max_lines = match max_lines {
        None => limits::READ_DEFAULT_MAX_LINES,
        Some(value) if value < 1 => 1,
        Some(value) => value.min(limits::READ_MAX_LINES),
    };
    let max_bytes = match max_bytes {
        None => limits::READ_DEFAULT_MAX_BYTES,
        Some(value) => value.min(limits::READ_MAX_BYTES),
    };

    // Split into lines preserving the logical line count used elsewhere.
    let lines = split_lines(content);
    let total_lines = lines.len() as i64;

    if total_lines == 0 || start_line > total_lines {
        return ReadContent {
            content_md: String::new(),
            start_line,
            end_line: start_line.saturating_sub(1),
            returned_lines: 0,
            truncated: false,
            next_start_line: None,
        };
    }

    let start_index = (start_line - 1) as usize;
    let mut out = String::new();
    let mut returned = 0_i64;

    for line in lines.iter().skip(start_index).take(max_lines as usize) {
        // Re-add the newline that `split_lines` stripped, reconstructing exactly
        // one '\n' between lines as the canonical separator.
        let candidate_len = line.len() + 1;
        if !out.is_empty() && out.len() + candidate_len > max_bytes {
            // Byte budget reached after at least one line; stop here.
            break;
        }
        out.push_str(line);
        out.push('\n');
        returned += 1;
        if out.len() >= max_bytes {
            // Always return at least one line (forward progress), then stop once
            // the byte budget is met or exceeded.
            break;
        }
    }

    let end_line = start_line + returned - 1;
    // Truncated whenever any line beyond what we returned remains (whether the
    // stop was the line cap or the byte budget).
    let truncated = (start_index as i64 + returned) < total_lines;
    let next_start_line = if truncated { Some(end_line + 1) } else { None };

    ReadContent {
        content_md: out,
        start_line,
        end_line,
        returned_lines: returned,
        truncated,
        next_start_line,
    }
}

/// Split content into logical lines (drops the single trailing newline so a
/// document ending in `\n` is not counted as having a trailing empty line). This
/// mirrors [`content_metrics`]'s line count.
fn split_lines(content: &str) -> Vec<&str> {
    if content.is_empty() {
        return Vec::new();
    }
    let trimmed = content.strip_suffix('\n').unwrap_or(content);
    trimmed.split('\n').collect()
}
