import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { ApiError } from "../../api/errors";
import type { RestNode } from "../../entities/node/model";
import { FileDetailView } from "./FileDetailView";
import { useFileDownload } from "./useEditorQueries";
import { useFilePreviewUrl } from "./useFilePreviewQueries";

const { pdfPreviewMounted, pdfPreviewUnmounted } = vi.hoisted(() => ({
  pdfPreviewMounted: vi.fn(),
  pdfPreviewUnmounted: vi.fn()
}));

vi.mock("./useEditorQueries", () => ({
  useFileDownload: vi.fn()
}));

vi.mock("./useFilePreviewQueries", () => ({
  useFilePreviewUrl: vi.fn()
}));

vi.mock("./PdfPreview", async () => {
  const { useEffect, useRef } = await import("react");
  return {
    PdfPreview: ({ name, url }: { name: string; url: string }) => {
      const initialUrl = useRef(url).current;
      useEffect(() => {
        pdfPreviewMounted(initialUrl);
        return () => pdfPreviewUnmounted(initialUrl);
      }, [initialUrl]);
      return <section aria-label={`PDF preview: ${name}`} data-url={url} />;
    }
  };
});

describe("FileDetailView", () => {
  beforeEach(() => {
    pdfPreviewMounted.mockClear();
    pdfPreviewUnmounted.mockClear();
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

  it("renders a verified PDF inside the file detail view", async () => {
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

    expect(await screen.findByRole("region", { name: "PDF preview: document.pdf" })).toHaveAttribute(
      "data-url",
      "https://storage.example/document.pdf"
    );
  });

  it("remounts the PDF viewer when the presigned URL changes", async () => {
    const node = fileNode({
      name: "document.pdf",
      path: "/document.pdf",
      media_type: "application/pdf"
    });
    vi.mocked(useFilePreviewUrl).mockReturnValue(pdfPreviewQuery("https://storage.example/first.pdf"));
    const view = render(<FileDetailView node={node} />);
    await waitFor(() => expect(pdfPreviewMounted).toHaveBeenCalledWith("https://storage.example/first.pdf"));

    vi.mocked(useFilePreviewUrl).mockReturnValue(pdfPreviewQuery("https://storage.example/refreshed.pdf"));
    view.rerender(<FileDetailView node={node} />);

    await waitFor(() => {
      expect(pdfPreviewUnmounted).toHaveBeenCalledWith("https://storage.example/first.pdf");
      expect(pdfPreviewMounted).toHaveBeenCalledWith("https://storage.example/refreshed.pdf");
    });
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

  it("keeps the preview error visible when URL refresh fails", async () => {
    const refetch = vi.fn().mockResolvedValue({
      isSuccess: false,
      data: undefined,
      error: new ApiError("storage unavailable", 503)
    });
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

    expect(screen.queryByRole("img", { name: "image.png" })).not.toBeInTheDocument();
    expect(screen.getByText("Image cannot be displayed")).toBeInTheDocument();
  });

  it("keeps the preview error visible when URL refresh returns the failed URL", async () => {
    const previewData = {
      url: "https://storage.example/broken.png",
      media_type: "image/png",
      expires_at: "2026-06-13T00:15:00Z"
    };
    const refetch = vi.fn().mockResolvedValue({
      isSuccess: true,
      data: previewData
    });
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

  it("recovers only when URL refresh returns a new preview URL", async () => {
    let previewUrl = "https://storage.example/broken.png";
    const refetch = vi.fn().mockImplementation(async () => {
      previewUrl = "https://storage.example/refreshed.png";
      return {
        isSuccess: true,
        data: {
          url: previewUrl,
          media_type: "image/png",
          expires_at: "2026-06-13T00:15:00Z"
        }
      };
    });
    vi.mocked(useFilePreviewUrl).mockImplementation(() => ({
      data: {
        url: previewUrl,
        media_type: "image/png",
        expires_at: "2026-06-13T00:15:00Z"
      },
      refetch
    } as never));
    render(<FileDetailView node={fileNode()} />);

    fireEvent.error(screen.getByRole("img", { name: "image.png" }));

    expect(await screen.findByRole("img", { name: "image.png" })).toHaveAttribute(
      "src",
      "https://storage.example/refreshed.png"
    );
    expect(refetch).toHaveBeenCalledTimes(1);
    expect(screen.queryByText("Image cannot be displayed")).not.toBeInTheDocument();
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

function pdfPreviewQuery(url: string) {
  return {
    data: {
      url,
      media_type: "application/pdf",
      expires_at: "2026-06-13T00:15:00Z"
    }
  } as never;
}
