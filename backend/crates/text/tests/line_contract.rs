//! Cross-operation regression cases for the shared logical-line contract.

#![allow(clippy::unwrap_used)]

use notegate_text::LineEdit;
use notegate_text::content::compute;
use notegate_text::lines::{line_ranges, logical_lines, with_endings};
use notegate_text::patch::{PatchError, apply_line_edits};

#[test]
fn metrics_ranges_and_edits_agree_on_logical_lines() {
    let cases: &[(&str, &[&str], &[&str])] = &[
        ("", &[], &[]),
        ("a", &["a"], &["a"]),
        ("a\n", &["a\n"], &["a"]),
        ("\n", &["\n"], &[""]),
        ("\n\n", &["\n", "\n"], &["", ""]),
        ("a\n\n", &["a\n", "\n"], &["a", ""]),
        ("\r", &["\r"], &["\r"]),
        ("\r\n", &["\r\n"], &["\r"]),
        (
            "가\r\n🙂\n끝",
            &["가\r\n", "🙂\n", "끝"],
            &["가\r", "🙂", "끝"],
        ),
    ];

    for &(source, expected, matching_lines) in cases {
        assert_eq!(with_endings(source).collect::<Vec<_>>(), expected);
        assert_eq!(logical_lines(source).collect::<Vec<_>>(), matching_lines);
        assert_eq!(compute(source).line_count, expected.len());
        assert_eq!(compute(source).byte_len, source.len());
        let slices: Vec<_> = line_ranges(source)
            .map(|range| source.get(range).unwrap())
            .collect();
        assert_eq!(slices, expected);
        assert_eq!(slices.concat(), source);

        for index in 0..expected.len() {
            let edited = apply_line_edits(
                source,
                &[LineEdit::DeleteLines {
                    start_line: index as i64 + 1,
                    end_line: index as i64 + 1,
                }],
            )
            .unwrap();
            let remaining = expected
                .iter()
                .enumerate()
                .filter_map(|(other, line)| (other != index).then_some(*line))
                .collect::<String>();
            assert_eq!(edited.content, remaining);
            assert_eq!(edited.replacements, 1);
            assert_eq!(compute(&edited.content).line_count, expected.len() - 1);
        }

        let past_end = expected.len() as i64 + 1;
        assert!(matches!(
            apply_line_edits(
                source,
                &[LineEdit::DeleteLines {
                    start_line: past_end,
                    end_line: past_end,
                }],
            ),
            Err(PatchError::InvalidLine(_))
        ));
    }
}

#[test]
fn line_replacement_preserves_untouched_utf8_and_crlf_bytes() {
    let edited = apply_line_edits(
        "가\r\n🙂\r\n끝",
        &[LineEdit::ReplaceLines {
            start_line: 2,
            end_line: 2,
            content: "새 줄\r\n".to_owned(),
        }],
    )
    .unwrap();
    assert_eq!(edited.content, "가\r\n새 줄\r\n끝");
    assert_eq!(compute(&edited.content).line_count, 3);
}
