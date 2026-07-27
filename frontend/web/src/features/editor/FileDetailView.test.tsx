import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { ApiError } from "../../api/errors";
import type { RestNode } from "../../api/types";
import { FileDetailView } from "./FileDetailView";
import { useFilePreviewUrl } from "./useFilePreviewQueries";

vi.mock("./useFilePreviewQueries", async (importOriginal) => ({
  ...await importOriginal<typeof import("./useFilePreviewQueries")>(),
  useFilePreviewUrl: vi.fn()
}));

vi.mock("./PdfPreview", () => ({
  PdfPreview: ({ url, name, onError }: { url: string; name: string; onError: () => void }) => (
    <button type="button" data-testid="pdf-preview" data-url={url} onClick={onError}>{name} PDF preview</button>
  )
}));

describe("FileDetailView", () => {
  beforeEach(() => {
    vi.mocked(useFilePreviewUrl).mockReturnValue({ data: undefined } as never);
  });

  it("renders a verified image from its preview URL", () => {
    vi.mocked(useFilePreviewUrl).mockReturnValue({
      data: {
        url: "https://storage.example/image.png",
        media_type: "image/png",
        expires_at: "2026-06-13T00:15:00Z"
      }
    } as never);

    render(<FileDetailView node={fileNode({
      media_type: "text/plain",
      detected_media_type: undefined,
      preview_available: undefined
    })} />);

    expect(screen.getByRole("img", { name: "image.png" })).toHaveAttribute(
      "src",
      "https://storage.example/image.png"
    );
    expect(screen.queryByText("image/png")).not.toBeInTheDocument();
  });

  it("keeps metadata available without a preview", () => {
    render(<FileDetailView node={fileNode({
      media_type: "application/pdf",
      preview_available: false
    })} />);

    expect(screen.queryByRole("img")).not.toBeInTheDocument();
    expect(screen.getByText("application/pdf")).toBeInTheDocument();
  });

  it("renders a verified PDF from its preview URL", async () => {
    vi.mocked(useFilePreviewUrl).mockReturnValue({
      data: {
        url: "https://storage.example/document.pdf",
        media_type: "application/pdf",
        expires_at: "2026-06-13T00:15:00Z"
      }
    } as never);

    render(<FileDetailView node={fileNode({
      name: "document.pdf",
      path: "/document.pdf",
      media_type: "application/pdf",
      detected_media_type: "application/pdf",
      preview_available: false,
      file_preview_kind: "pdf"
    })} />);

    expect(await screen.findByTestId("pdf-preview")).toHaveAttribute(
      "data-url",
      "https://storage.example/document.pdf"
    );
    expect(screen.queryByRole("img")).not.toBeInTheDocument();
  });

  it("shows an error when preview URL issuance fails", () => {
    vi.mocked(useFilePreviewUrl).mockReturnValue({
      data: undefined,
      isError: true,
      error: new ApiError("storage unavailable", 503)
    } as never);

    render(<FileDetailView node={fileNode()} />);

    expect(screen.getByText("Image cannot be displayed")).toBeInTheDocument();
  });

  it("uses PDF copy for PDF preview failures", () => {
    vi.mocked(useFilePreviewUrl).mockReturnValue({
      data: undefined,
      isError: true,
      error: new ApiError("storage unavailable", 503)
    } as never);

    render(<FileDetailView node={fileNode({
      media_type: "application/pdf",
      preview_available: false,
      file_preview_kind: "pdf"
    })} />);

    expect(screen.getByText("PDF cannot be displayed")).toBeInTheDocument();
  });

  it("does not show an error when the file is not previewable", () => {
    vi.mocked(useFilePreviewUrl).mockReturnValue({
      data: undefined,
      isError: true,
      error: new ApiError("not previewable", 404)
    } as never);

    render(<FileDetailView node={fileNode({ preview_available: undefined })} />);

    expect(screen.queryByText("Image cannot be displayed")).not.toBeInTheDocument();
  });

  it("keeps a failed preview hidden when refresh returns the same URL", async () => {
    const previewData = {
      url: "https://storage.example/broken.png",
      media_type: "image/png",
      expires_at: "2026-06-13T00:15:00Z"
    };
    const refetch = vi.fn().mockResolvedValue({ isSuccess: true, data: previewData });
    vi.mocked(useFilePreviewUrl).mockReturnValue({
      data: previewData,
      refetch
    } as never);
    render(<FileDetailView node={fileNode()} />);

    fireEvent.error(screen.getByRole("img", { name: "image.png" }));
    await waitFor(() => expect(refetch).toHaveBeenCalledTimes(1));

    expect(screen.queryByRole("img", { name: "image.png" })).not.toBeInTheDocument();
    expect(screen.getByText("Image cannot be displayed")).toBeInTheDocument();
  });
});

function fileNode(overrides: Partial<RestNode> = {}): RestNode {
  return {
    id: "file-1",
    space_id: "space-1",
    parent_id: "root-1",
    name: "image.png",
    kind: "file",
    path: "/image.png",
    sort_order: 0,
    metadata: {},
    search_enabled: true,
    write_locked: false,
    write_lock_sources: [],
    has_children: false,
    effective_write_locked: false,
    byte_len: 29,
    media_type: "image/png",
    detected_media_type: "image/png",
    preview_available: true,
    encryption_mode: "none",
    created_by: { id: "user-1", kind: "user", display_name: "User" },
    updated_by: { id: "user-1", kind: "user", display_name: "User" },
    created_at: "2026-06-13T00:00:00Z",
    updated_at: "2026-06-13T00:00:00Z",
    ...overrides
  };
}
