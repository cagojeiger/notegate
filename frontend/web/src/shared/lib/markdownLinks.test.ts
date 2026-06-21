import { describe, expect, it } from "vitest";

import { classifyMarkdownLink, safeMarkdownUrlTransform } from "./markdownLinks";

describe("safeMarkdownUrlTransform", () => {
  it("allows safe external and relative markdown links", () => {
    expect(safeMarkdownUrlTransform("https://example.com")).toBe("https://example.com");
    expect(safeMarkdownUrlTransform("mailto:hello@example.com")).toBe("mailto:hello@example.com");
    expect(safeMarkdownUrlTransform("tel:+15551234567")).toBe("tel:+15551234567");
    expect(safeMarkdownUrlTransform("./note.md?view=1")).toBe("./note.md?view=1");
    expect(safeMarkdownUrlTransform("//example.com/doc.md")).toBe("//example.com/doc.md");
  });

  it("removes unsafe markdown link protocols", () => {
    expect(safeMarkdownUrlTransform("javascript:alert(1)")).toBe("");
    expect(safeMarkdownUrlTransform("data:text/html,<script>alert(1)</script>")).toBe("");
    expect(safeMarkdownUrlTransform("blob:https://example.com/id")).toBe("");
  });
});

describe("classifyMarkdownLink", () => {
  it("classifies only relative and root paths as internal or invalid node path candidates", () => {
    expect(classifyMarkdownLink("/index.md", "./note.md").kind).toBe("internal");
    expect(classifyMarkdownLink("/folder/index.md", "../note.md")).toEqual({ kind: "internal", path: "/note.md" });
    expect(classifyMarkdownLink("/index.md", "/note.md").kind).toBe("internal");
    expect(classifyMarkdownLink("/index.md", "#section")).toEqual({ kind: "external" });
    expect(classifyMarkdownLink("/index.md", "https://example.com")).toEqual({ kind: "external" });
    expect(classifyMarkdownLink("/index.md", "//example.com/doc.md")).toEqual({ kind: "external" });
  });

  it("resolves relative links from the source document folder", () => {
    expect(classifyMarkdownLink("/Security and Compliance (원본)/index.md", "./Policies/Access%20Control%20Policy.md")).toEqual({
      kind: "internal",
      path: "/Security and Compliance (원본)/Policies/Access Control Policy.md"
    });
    expect(classifyMarkdownLink("/runtime-architecture/00-repo-catalog.md", "01-runtime-map.md")).toEqual({ kind: "internal", path: "/runtime-architecture/01-runtime-map.md" });
  });

  it("resolves root-relative links from the active space root", () => {
    expect(classifyMarkdownLink("/runtime-architecture/00-repo-catalog.md", "/Security/Policies.md")).toEqual({ kind: "internal", path: "/Security/Policies.md" });
  });

  it("decodes encoded filename characters without treating them as separators", () => {
    expect(classifyMarkdownLink("/Security/index.md", "./Design%20%231.md")).toEqual({ kind: "internal", path: "/Security/Design #1.md" });
    expect(classifyMarkdownLink("/Security/index.md", "./Decision%3F.md")).toEqual({ kind: "internal", path: "/Security/Decision?.md" });
  });

  it("normalizes dot segments without escaping the active space root", () => {
    expect(classifyMarkdownLink("/a/b/c.md", "../README.md")).toEqual({ kind: "internal", path: "/a/README.md" });
    expect(classifyMarkdownLink("/a/b/c.md", "./../b/./d.md")).toEqual({ kind: "internal", path: "/a/b/d.md" });
    expect(classifyMarkdownLink("/a.md", "../outside.md")).toEqual({ kind: "invalid" });
  });

  it("ignores anchors, external URLs, protocol-relative URLs, and query links", () => {
    expect(classifyMarkdownLink("/a/b.md", "#section")).toEqual({ kind: "external" });
    expect(classifyMarkdownLink("/a/b.md", "https://example.com/doc.md")).toEqual({ kind: "external" });
    expect(classifyMarkdownLink("/a/b.md", "mailto:hello@example.com")).toEqual({ kind: "external" });
    expect(classifyMarkdownLink("/a/b.md", "//example.com/doc.md")).toEqual({ kind: "external" });
    expect(classifyMarkdownLink("/a/b.md", "./doc.md?view=1")).toEqual({ kind: "invalid" });
  });

  it("strips file fragments after path resolution", () => {
    expect(classifyMarkdownLink("/a/index.md", "./doc.md#details")).toEqual({ kind: "internal", path: "/a/doc.md" });
  });

  it("ignores malformed encoded paths", () => {
    expect(classifyMarkdownLink("/a/index.md", "./bad%path.md")).toEqual({ kind: "invalid" });
  });

  it("does not resolve encoded slashes as node names", () => {
    expect(classifyMarkdownLink("/a/index.md", "./folder%2Fsecret.md")).toEqual({ kind: "invalid" });
  });

  it("does not resolve encoded control characters as node names", () => {
    expect(classifyMarkdownLink("/a/index.md", "./bad%0Aname.md")).toEqual({ kind: "invalid" });
  });
});
