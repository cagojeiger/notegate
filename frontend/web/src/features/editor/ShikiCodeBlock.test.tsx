import { render, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { highlightCode } from "./highlightCode";
import { ShikiCodeBlock } from "./ShikiCodeBlock";

vi.mock("./highlightCode", () => ({
  highlightCode: vi.fn()
}));

describe("ShikiCodeBlock", () => {
  beforeEach(() => {
    vi.mocked(highlightCode).mockReset();
  });

  it("falls back to escaped source text when syntax highlighting fails", async () => {
    vi.mocked(highlightCode).mockRejectedValue(new Error("highlight failed"));
    const source = "if value < 2:\n    return '<safe>'";
    const { container } = render(<ShikiCodeBlock code={source} language="python" />);

    await waitFor(() => expect(highlightCode).toHaveBeenCalledWith(source, "python"));
    const fallback = container.querySelector("pre.ng-code-fallback");
    expect(fallback?.textContent).toBe(source);
    expect(fallback?.innerHTML).toContain("&lt;safe&gt;");
  });
});
