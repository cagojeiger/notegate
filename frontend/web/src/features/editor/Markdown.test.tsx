import { render } from "@testing-library/react";
import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it, vi } from "vitest";

import { parseMarkdownDocument } from "../../shared/lib/markdownDocument";
import { Markdown } from "./Markdown";

vi.mock("../../shared/lib/markdownDocument", async (importOriginal) => {
  const actual = await importOriginal<typeof import("../../shared/lib/markdownDocument")>();
  return {
    ...actual,
    parseMarkdownDocument: vi.fn(actual.parseMarkdownDocument)
  };
});

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

  it("does not parse unchanged content again when its parent rerenders", () => {
    vi.mocked(parseMarkdownDocument).mockClear();
    const props = { content: "# Cached preview" };
    const { rerender } = render(<Markdown {...props} />);

    rerender(<Markdown {...props} />);

    expect(parseMarkdownDocument).toHaveBeenCalledTimes(1);
  });
});
