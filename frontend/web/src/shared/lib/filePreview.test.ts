import { describe, expect, it } from "vitest";

import { filePreviewKind } from "./filePreview";

describe("filePreviewKind", () => {
  it("separates safe raster images, PDF, and unsupported media", () => {
    expect(filePreviewKind("image/png")).toBe("image");
    expect(filePreviewKind("image/webp")).toBe("image");
    expect(filePreviewKind("application/pdf")).toBe("pdf");
    expect(filePreviewKind("image/svg+xml")).toBeNull();
    expect(filePreviewKind("text/html")).toBeNull();
    expect(filePreviewKind(undefined)).toBeNull();
  });
});
