import { describe, expect, it } from "vitest";

import { parseDelimitedText, type DelimitedFormat } from "./tabularData";

describe("parseDelimitedText", () => {
  it.each<[DelimitedFormat, string, string[][]]>([
    ["csv", "name,score\nAda,42", [["name", "score"], ["Ada", "42"]]],
    ["tsv", "name\tscore\nAda\t42", [["name", "score"], ["Ada", "42"]]]
  ])("parses %s fields as strings", (format, content, rows) => {
    expect(parseDelimitedText(format, content)).toEqual({ ok: true, rows, columnCount: 2 });
  });

  it("parses quoted delimiters, escaped quotes, and embedded line feeds", () => {
    expect(parseDelimitedText("csv", 'name,note\nAda,"hello, ""friend""\nnext line"')).toEqual({
      ok: true,
      rows: [["name", "note"], ["Ada", 'hello, "friend"\nnext line']],
      columnCount: 2
    });
  });

  it("normalizes CRLF while preserving bare carriage returns as data", () => {
    expect(parseDelimitedText("csv", "a,b\r\nc,d\re")).toEqual({
      ok: true,
      rows: [["a", "b"], ["c", "d\re"]],
      columnCount: 2
    });
  });

  it("strips a leading byte-order mark", () => {
    expect(parseDelimitedText("csv", "\uFEFFname,value\nAda,1")).toEqual({
      ok: true,
      rows: [["name", "value"], ["Ada", "1"]],
      columnCount: 2
    });
  });

  it("preserves ragged and empty rows without padding", () => {
    expect(parseDelimitedText("csv", "a,b,c\n\nd,e")).toEqual({
      ok: true,
      rows: [["a", "b", "c"], [""], ["d", "e"]],
      columnCount: 3
    });
  });

  it("reports unterminated quoted fields", () => {
    const result = parseDelimitedText("csv", 'a,"unterminated');

    expect(result.ok).toBe(false);
    if (!result.ok) expect(result.message).toMatch(/unterminated/i);
  });

  it("does not turn a trailing line feed into an extra record", () => {
    expect(parseDelimitedText("csv", "a,b\n")).toEqual({
      ok: true,
      rows: [["a", "b"]],
      columnCount: 2
    });
  });

  it("preserves explicit blank records before a trailing line feed", () => {
    expect(parseDelimitedText("csv", "a,b\n\n")).toEqual({
      ok: true,
      rows: [["a", "b"], [""]],
      columnCount: 2
    });
  });
});
