import { describe, expect, it } from "vitest";

import { parseMarkdownDocument } from "./markdownDocument";
import { formatFrontmatterValue, hasMarkdownFrontmatterCandidate } from "./markdownFrontmatter";

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

  it("detects leading BOM-prefixed CRLF frontmatter candidates", () => {
    expect(hasMarkdownFrontmatterCandidate("\uFEFF---\r\ntitle: Note\r\n---\r\nBody")).toBe(true);
    expect(hasMarkdownFrontmatterCandidate("# Body\n\n---\nnot frontmatter")).toBe(false);
  });

  it("formats nested frontmatter values for display", () => {
    expect(formatFrontmatterValue(["one", 2, null])).toBe("one, 2, null");
    expect(formatFrontmatterValue({ enabled: true })).toBe('{"enabled":true}');
  });

  it.each([
    "value: &cycle { self: *cycle }",
    "value: &cycle [*cycle]",
    "value: &cycle { children: [*cycle] }"
  ])("keeps the document readable when frontmatter contains %s", (source) => {
    const document = parseMarkdownDocument(`---\n${source}\n---\n# Body`);
    expect(formatFrontmatterValue(document.frontmatter!.value)).toBe("[Circular reference]");
    expect(document.body).toBe("# Body");
  });

  it("keeps shared aliases distinct from circular references", () => {
    const document = parseMarkdownDocument("---\nvalue: { first: &shared { label: normal }, second: *shared }\n---\n# Body");
    expect(formatFrontmatterValue(document.frontmatter!.value))
      .toBe('{"first":{"label":"normal"},"second":{"label":"normal"}}');
    expect(formatFrontmatterValue(["one", [2, null], { enabled: true }]))
      .toBe('one, 2, null, {"enabled":true}');
  });

  it("limits deeply nested properties before recursive formatting", () => {
    let value: unknown = "leaf";
    for (let index = 0; index < 5_000; index++) value = { child: value };
    expect(formatFrontmatterValue(value)).toBe("[Value too complex to display]");
  });

  it("limits repeated alias expansion without traversing the entire graph", () => {
    let value: unknown = ["leaf"];
    for (let index = 0; index < 24; index++) value = [value, value];
    expect(formatFrontmatterValue(value)).toBe("[Value too complex to display]");
  });
});
