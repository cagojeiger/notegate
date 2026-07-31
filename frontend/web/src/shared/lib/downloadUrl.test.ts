import { describe, expect, it, vi } from "vitest";

import { downloadUrl } from "./downloadUrl";

describe("downloadUrl", () => {
  it("clicks a temporary same-origin download link", () => {
    const anchor = document.createElement("a");
    const click = vi.spyOn(anchor, "click").mockImplementation(() => undefined);
    const remove = vi.spyOn(anchor, "remove");
    vi.spyOn(document, "createElement").mockReturnValue(anchor);
    vi.spyOn(document.body, "append").mockImplementation(() => undefined);

    downloadUrl("/api/v1/spaces/space-1/files/file-1/content", "report.pdf");

    expect(anchor.getAttribute("href")).toBe("/api/v1/spaces/space-1/files/file-1/content");
    expect(anchor.download).toBe("report.pdf");
    expect(click).toHaveBeenCalled();
    expect(remove).toHaveBeenCalled();
  });
});
