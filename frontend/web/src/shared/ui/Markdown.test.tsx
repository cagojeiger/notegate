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
});
