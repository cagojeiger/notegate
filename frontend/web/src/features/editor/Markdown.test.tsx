import { render } from "@testing-library/react";
import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it, vi } from "vitest";

import { Markdown } from "./Markdown";

describe("Markdown", () => {
  it("does not render raw internal image src before the loader runs", () => {
    const loadInternalImage = vi.fn();
    const markup = renderToStaticMarkup(
      <Markdown
        content={"![Logo](./Assets/logo.png)"}
        imagePolicy={{ sourcePath: "/Docs/index.md", loadInternalImage }}
      />
    );

    expect(markup).not.toContain('src="./Assets/logo.png"');
    expect(markup).toContain("Loading image...: Logo");
    expect(loadInternalImage).not.toHaveBeenCalled();
  });

  it("renders supplied frontmatter as properties before the markdown body", () => {
    const { container } = render(
      <Markdown
        content="# Body"
        frontmatter={{
          title: "Note",
          tags: ["one", "two"]
        }}
      />
    );

    expect(container.querySelector(".markdown-frontmatter")).toHaveTextContent("title");
    expect(container.querySelector(".markdown-frontmatter")).toHaveTextContent("Note");
    expect(container.querySelector(".markdown-frontmatter")).toHaveTextContent("tags");
    expect(container.querySelector(".markdown-frontmatter")).toHaveTextContent("one, two");
    expect(container.querySelector("h1")).toHaveTextContent("Body");
  });
});
