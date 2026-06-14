import { describe, expect, it } from "vitest";

import { parseStructuredText } from "./structuredData";

describe("parseStructuredText", () => {
  it("parses json, yaml, and toml into tree values", () => {
    expect(parseStructuredText("json", '{"ok":true}')).toMatchObject({ ok: true, value: { ok: true } });
    expect(parseStructuredText("yaml", "root:\n  child: 1\n")).toMatchObject({ ok: true, value: { root: { child: 1 } } });
    expect(parseStructuredText("toml", "[root]\nchild = 1\n")).toMatchObject({ ok: true, value: { root: { child: 1 } } });
  });

  it("wraps jsonl records with source line numbers", () => {
    const result = parseStructuredText("jsonl", '{"a":1}\n[2]\n');
    expect(result).toMatchObject({
      ok: true,
      value: [
        { line: 1, value: { a: 1 } },
        { line: 2, value: [2] }
      ]
    });
  });

  it("returns parse errors instead of throwing", () => {
    const result = parseStructuredText("json", "{");
    expect(result.ok).toBe(false);
    if (!result.ok) expect(result.message).toContain("JSON");
  });
});
