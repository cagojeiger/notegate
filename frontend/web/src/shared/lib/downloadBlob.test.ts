import { describe, expect, it, vi } from "vitest";

import { downloadBlob, downloadUrl } from "./downloadBlob";

describe("downloadBlob", () => {
  it("creates and revokes an object URL for the download", () => {
    const click = vi.fn();
    const remove = vi.fn();
    vi.spyOn(document, "createElement").mockReturnValue({ click, remove } as unknown as HTMLAnchorElement);
    vi.spyOn(document.body, "append").mockImplementation(() => undefined);
    Object.assign(URL, {
      createObjectURL: vi.fn().mockReturnValue("blob:test"),
      revokeObjectURL: vi.fn()
    });

    downloadBlob(new Blob(["hello"]), "note.txt");

    expect(URL.createObjectURL).toHaveBeenCalled();
    expect(document.body.append).toHaveBeenCalled();
    expect(click).toHaveBeenCalled();
    expect(remove).toHaveBeenCalled();
    expect(URL.revokeObjectURL).toHaveBeenCalledWith("blob:test");
  });

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
