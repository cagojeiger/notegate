import { describe, expect, it, vi } from "vitest";

import { downloadBlob } from "./downloadBlob";

describe("downloadBlob", () => {
  it("creates and revokes an object URL for the download", () => {
    const click = vi.fn();
    vi.spyOn(document, "createElement").mockReturnValue({ click } as unknown as HTMLAnchorElement);
    Object.assign(URL, {
      createObjectURL: vi.fn().mockReturnValue("blob:test"),
      revokeObjectURL: vi.fn()
    });

    downloadBlob(new Blob(["hello"]), "note.txt");

    expect(URL.createObjectURL).toHaveBeenCalled();
    expect(click).toHaveBeenCalled();
    expect(URL.revokeObjectURL).toHaveBeenCalledWith("blob:test");
  });
});
