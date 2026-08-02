import { describe, expect, it } from "vitest";

import { formatCodeBlockLabel, normalizeCodeLanguage } from "./codeBlockLanguage";

describe("codeBlockLanguage", () => {
  it("normalizes syntax highlighter aliases", () => {
    expect(normalizeCodeLanguage("MD")).toBe("markdown");
    expect(normalizeCodeLanguage("PY")).toBe("python");
    expect(normalizeCodeLanguage("yml")).toBe("yaml");
    expect(normalizeCodeLanguage("txt")).toBe("text");
    expect(normalizeCodeLanguage("JS")).toBe("javascript");
    expect(normalizeCodeLanguage("MTS")).toBe("typescript");
    expect(normalizeCodeLanguage("RS")).toBe("rust");
    expect(normalizeCodeLanguage("ZSH")).toBe("shellscript");
  });

  it("formats labels through the same aliases", () => {
    expect(formatCodeBlockLabel("md")).toBe("Markdown");
    expect(formatCodeBlockLabel("py")).toBe("Python");
    expect(formatCodeBlockLabel("python")).toBe("Python");
    expect(formatCodeBlockLabel("go")).toBe("Go");
    expect(formatCodeBlockLabel("js")).toBe("JavaScript");
    expect(formatCodeBlockLabel("rs")).toBe("Rust");
    expect(formatCodeBlockLabel("sh")).toBe("Shell");
    expect(formatCodeBlockLabel("ts")).toBe("TypeScript");
    expect(formatCodeBlockLabel("tsx")).toBe("TSX");
    expect(formatCodeBlockLabel("yml")).toBe("YAML");
  });
});
