import { describe, expect, it } from "vitest";

import { inferTextFormat, isCodeFormat, isStructuredFormat, shikiLangForFormat } from "./textFormat";

describe("textFormat", () => {
  it("infers known document formats from file names", () => {
    expect(inferTextFormat("README.md")).toBe("markdown");
    expect(inferTextFormat("data.JSON")).toBe("json");
    expect(inferTextFormat("events.jsonl")).toBe("jsonl");
    expect(inferTextFormat("config.yaml")).toBe("yaml");
    expect(inferTextFormat("config.yml")).toBe("yaml");
    expect(inferTextFormat("Cargo.toml")).toBe("toml");
    expect(inferTextFormat("notes")).toBe("plain");
  });

  it("infers SQL and Python source formats case-insensitively", () => {
    expect(inferTextFormat("query.SQL")).toBe("sql");
    expect(inferTextFormat("script.Py")).toBe("python");
    expect(inferTextFormat("types.PYI")).toBe("python");
    expect(inferTextFormat("launcher.pyw")).toBe("python");
  });

  it("maps formats to highlighter languages", () => {
    expect(shikiLangForFormat("markdown")).toBe("markdown");
    expect(shikiLangForFormat("jsonl")).toBe("jsonl");
    expect(shikiLangForFormat("sql")).toBe("sql");
    expect(shikiLangForFormat("python")).toBe("python");
    expect(shikiLangForFormat("plain")).toBe("text");
  });

  it("keeps source formats separate from structured previews", () => {
    expect(isCodeFormat("sql")).toBe(true);
    expect(isCodeFormat("python")).toBe(true);
    expect(isStructuredFormat("sql")).toBe(false);
    expect(isStructuredFormat("python")).toBe(false);
    expect(isStructuredFormat("json")).toBe(true);
  });
});
