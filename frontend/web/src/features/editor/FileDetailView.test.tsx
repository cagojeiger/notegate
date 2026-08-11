import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { ApiError } from "../../api/errors";
import type { RestNode } from "../../api/types";
import { makeRestNode } from "../../test/fixtures";
import { FileDetailView } from "./FileDetailView";
import type { useFilePreviewUrl } from "./useFilePreviewQueries";

type FilePreviewQuery = ReturnType<typeof useFilePreviewUrl>;
type FilePreviewRefetchResult = Pick<
  Awaited<ReturnType<FilePreviewQuery["refetch"]>>,
  "data" | "isSuccess"
>;
type FilePreviewQueryMock = Pick<
  FilePreviewQuery,
  "data" | "error" | "isError" | "isLoading"
> & {
  refetch: () => Promise<FilePreviewRefetchResult>;
};

const filePreviewQueryMocks = vi.hoisted(() => ({
  useFilePreviewUrl: vi.fn<
    (...args: Parameters<typeof useFilePreviewUrl>) => FilePreviewQueryMock
  >()
}));

vi.mock("./useFilePreviewQueries", async (importOriginal) => ({
  ...await importOriginal<typeof import("./useFilePreviewQueries")>(),
  useFilePreviewUrl: filePreviewQueryMocks.useFilePreviewUrl
}));

vi.mock("./PdfPreview", () => ({
  PdfPreview: ({ url, name, onError }: { url: string; name: string; onError: () => void }) => (
    <button type="button" data-testid="pdf-preview" data-url={url} onClick={onError}>{name} PDF preview</button>
  )
}));

describe("FileDetailView", () => {
  beforeEach(() => {
    filePreviewQueryMocks.useFilePreviewUrl.mockReturnValue(previewQuery());
  });

  it("renders a verified image from its preview URL", () => {
    filePreviewQueryMocks.useFilePreviewUrl.mockReturnValue(previewQuery({
      data: {
        url: "https://storage.example/image.png",
        media_type: "image/png",
        expires_at: "2026-06-13T00:15:00Z"
      }
    }));

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

  it("labels audio files as recordings", () => {
    render(<FileDetailView node={fileNode({
      name: "meeting.m4a",
      path: "/meeting.m4a",
      media_type: "audio/mp4",
      detected_media_type: "video/mp4",
      preview_available: false,
      file_media_kind: "audio"
    })} />);

    expect(screen.getByText("Audio recording")).toBeInTheDocument();
    expect(screen.getByRole("heading", { name: "meeting.m4a" })).toBeInTheDocument();
  });

  it("renders a verified PDF from its preview URL", async () => {
    filePreviewQueryMocks.useFilePreviewUrl.mockReturnValue(previewQuery({
      data: {
        url: "https://storage.example/document.pdf",
        media_type: "application/pdf",
        expires_at: "2026-06-13T00:15:00Z"
      }
    }));

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
    filePreviewQueryMocks.useFilePreviewUrl.mockReturnValue(previewQuery({
      isError: true,
      error: new ApiError("storage unavailable", 503)
    }));

    render(<FileDetailView node={fileNode()} />);

    expect(screen.getByText("Image cannot be displayed")).toBeInTheDocument();
  });

  it("uses PDF copy for PDF preview failures", () => {
    filePreviewQueryMocks.useFilePreviewUrl.mockReturnValue(previewQuery({
      isError: true,
      error: new ApiError("storage unavailable", 503)
    }));

    render(<FileDetailView node={fileNode({
      media_type: "application/pdf",
      preview_available: false,
      file_preview_kind: "pdf"
    })} />);

    expect(screen.getByText("PDF cannot be displayed")).toBeInTheDocument();
  });

  it("does not show an error when the file is not previewable", () => {
    filePreviewQueryMocks.useFilePreviewUrl.mockReturnValue(previewQuery({
      isError: true,
      error: new ApiError("not previewable", 404)
    }));

    render(<FileDetailView node={fileNode({ preview_available: undefined })} />);

    expect(screen.queryByText("Image cannot be displayed")).not.toBeInTheDocument();
  });

  it("keeps a failed preview hidden when refresh fails", async () => {
    const previewData = {
      url: "https://storage.example/broken.png",
      media_type: "image/png",
      expires_at: "2026-06-13T00:15:00Z"
    };
    const refetch = vi.fn().mockResolvedValue({
      isSuccess: false,
      data: undefined,
      error: new ApiError("storage unavailable", 503)
    });
    filePreviewQueryMocks.useFilePreviewUrl.mockReturnValue(previewQuery({
      data: previewData,
      refetch
    }));
    render(<FileDetailView node={fileNode()} />);

    fireEvent.error(screen.getByRole("img", { name: "image.png" }));
    await waitFor(() => expect(refetch).toHaveBeenCalledTimes(1));

    expect(screen.queryByRole("img", { name: "image.png" })).not.toBeInTheDocument();
    expect(screen.getByText("Image cannot be displayed")).toBeInTheDocument();
  });

  it("keeps a failed preview hidden when refresh returns the same URL", async () => {
    const previewData = {
      url: "https://storage.example/broken.png",
      media_type: "image/png",
      expires_at: "2026-06-13T00:15:00Z"
    };
    const refetch = vi.fn().mockResolvedValue({ isSuccess: true, data: previewData });
    filePreviewQueryMocks.useFilePreviewUrl.mockReturnValue(previewQuery({
      data: previewData,
      refetch
    }));
    render(<FileDetailView node={fileNode()} />);

    fireEvent.error(screen.getByRole("img", { name: "image.png" }));
    await waitFor(() => expect(refetch).toHaveBeenCalledTimes(1));

    expect(screen.queryByRole("img", { name: "image.png" })).not.toBeInTheDocument();
    expect(screen.getByText("Image cannot be displayed")).toBeInTheDocument();
  });

  it("recovers when refresh returns a new preview URL", async () => {
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
    filePreviewQueryMocks.useFilePreviewUrl.mockImplementation(() => previewQuery({
      data: {
        url: previewUrl,
        media_type: "image/png",
        expires_at: "2026-06-13T00:15:00Z"
      },
      refetch
    }));
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

function previewQuery(
  overrides: Partial<FilePreviewQueryMock> = {}
): FilePreviewQueryMock {
  return {
    data: undefined,
    error: null,
    isError: false,
    isLoading: false,
    refetch: vi.fn().mockResolvedValue({
      data: undefined,
      isSuccess: false
    }),
    ...overrides
  };
}

function fileNode(overrides: Partial<RestNode> = {}): RestNode {
  return makeRestNode({
    id: "file-1",
    name: "image.png",
    kind: "file",
    path: "/image.png",
    byte_len: 29,
    media_type: "image/png",
    detected_media_type: "image/png",
    preview_available: true,
    encryption_mode: "none",
    ...overrides
  });
}
