import { describe, expect, it } from "vitest";

import { formatCodeBlockLabel, normalizeCodeLanguage } from "./codeBlockLanguage";

describe("codeBlockLanguage", () => {
  it("normalizes syntax highlighter aliases", () => {
    expect(normalizeCodeLanguage("MD")).toBe("markdown");
    expect(normalizeCodeLanguage("yml")).toBe("yaml");
    expect(normalizeCodeLanguage("txt")).toBe("text");
  });

  it("formats labels through the same aliases", () => {
    expect(formatCodeBlockLabel("md")).toBe("Markdown");
    expect(formatCodeBlockLabel("sh")).toBe("Shell");
    expect(formatCodeBlockLabel("yml")).toBe("YAML");
  });
});
