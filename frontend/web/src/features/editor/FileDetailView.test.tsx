import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { ApiError } from "../../api/errors";
import type { RestNode } from "../../api/types";
import { FileDetailView } from "./FileDetailView";
import { useFileDownload } from "./useEditorQueries";
import { useFilePreviewUrl } from "./useFilePreviewQueries";

vi.mock("./useEditorQueries", () => ({
  useFileDownload: vi.fn()
}));

vi.mock("./useFilePreviewQueries", () => ({
  useFilePreviewUrl: vi.fn()
}));

describe("FileDetailView", () => {
  beforeEach(() => {
    vi.mocked(useFileDownload).mockReturnValue(vi.fn());
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
    expect(screen.getByRole("article")).toHaveClass("min-h-0", "flex-1", "overflow-y-auto");
    expect(screen.getByText("image/png")).toBeInTheDocument();
  });

  it("keeps download and metadata available without a preview", () => {
    render(<FileDetailView node={fileNode({ media_type: "application/pdf" })} />);

    expect(screen.queryByRole("img")).not.toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Download" })).toBeInTheDocument();
  });

  it("renders a verified PDF inside the file detail view", () => {
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
      media_type: "application/octet-stream",
      detected_media_type: "application/pdf"
    })} />);

    expect(screen.getByTitle("PDF preview: document.pdf")).toHaveAttribute(
      "src",
      "https://storage.example/document.pdf"
    );
    expect(screen.getByTitle("PDF preview: document.pdf")).toHaveAttribute(
      "referrerpolicy",
      "no-referrer"
    );
  });

  it("shows an error when preview URL issuance fails", () => {
    vi.mocked(useFilePreviewUrl).mockReturnValue({
      data: undefined,
      isError: true,
      error: new ApiError("storage unavailable", 503)
    } as never);

    render(<FileDetailView node={fileNode()} />);

    expect(screen.getByText("File preview cannot be displayed")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Download" })).toBeInTheDocument();
  });

  it("does not show an error when the file is not previewable", () => {
    vi.mocked(useFilePreviewUrl).mockReturnValue({
      data: undefined,
      isError: true,
      error: new ApiError("not previewable", 404)
    } as never);

    render(<FileDetailView node={fileNode({ preview_available: undefined })} />);

    expect(screen.queryByText("Image cannot be displayed")).not.toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Download" })).toBeInTheDocument();
  });

  it("refreshes a failed preview URL once before showing an error", async () => {
    const refetch = vi.fn().mockResolvedValue({});
    vi.mocked(useFilePreviewUrl).mockReturnValue({
      data: {
        url: "https://storage.example/broken.png",
        media_type: "image/png",
        expires_at: "2026-06-13T00:15:00Z"
      },
      refetch
    } as never);
    render(<FileDetailView node={fileNode()} />);

    fireEvent.error(screen.getByRole("img", { name: "image.png" }));
    await waitFor(() => expect(refetch).toHaveBeenCalledTimes(1));
    fireEvent.error(await screen.findByRole("img", { name: "image.png" }));

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
    has_children: false,
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
