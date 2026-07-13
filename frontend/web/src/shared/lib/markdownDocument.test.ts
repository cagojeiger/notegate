import { describe, expect, it } from "vitest";

import { formatFrontmatterValue, parseMarkdownDocument } from "./markdownDocument";

describe("markdownDocument", () => {
  it("separates object frontmatter from the markdown body", () => {
    expect(parseMarkdownDocument("---\ntitle: Note\ntags: [one, two]\n---\n# Body")).toEqual({
      frontmatter: { title: "Note", tags: ["one", "two"] },
      body: "# Body"
    });
  });

  it("keeps content unchanged when frontmatter is missing or invalid", () => {
    const content = "---\n- not\n- an object\n---\n# Body";
    expect(parseMarkdownDocument(content)).toEqual({ frontmatter: null, body: content });
    expect(parseMarkdownDocument("# Body")).toEqual({ frontmatter: null, body: "# Body" });
  });

  it("supports an empty frontmatter object and the YAML closing fence", () => {
    expect(parseMarkdownDocument("---\n...\nBody")).toEqual({ frontmatter: {}, body: "Body" });
  });

  it("accepts BOM-prefixed CRLF frontmatter", () => {
    expect(parseMarkdownDocument("\uFEFF---\r\ntitle: Note\r\n---\r\nBody")).toEqual({
      frontmatter: { title: "Note" },
      body: "Body"
    });
  });

  it("formats nested frontmatter values for display", () => {
    expect(formatFrontmatterValue(["one", 2, null])).toBe("one, 2, null");
    expect(formatFrontmatterValue({ enabled: true })).toBe('{"enabled":true}');
  });
});
