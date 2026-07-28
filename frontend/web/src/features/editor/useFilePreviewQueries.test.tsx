import { QueryClientProvider } from "@tanstack/react-query";
import { renderHook, waitFor } from "@testing-library/react";
import type { ReactNode } from "react";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { batchResolveFilePreviews, getFilePreviewUrl } from "../../api/files";
import { ApiError } from "../../api/errors";
import { getNode } from "../../api/nodes";
import { queryKeys } from "../../api/queryKeys";
import type { RestNode } from "../../api/types";
import { makeRestNode } from "../../test/fixtures";
import { createTestQueryClient } from "../../test/queryClient";
import {
  filePreviewKindForNode,
  useFilePreviewUrl,
  useMarkdownImageLoader
} from "./useFilePreviewQueries";

const mockClient = vi.hoisted(() => ({}));

vi.mock("../../api/ApiProvider", () => ({
  useApiClient: () => mockClient
}));

vi.mock("../../api/files", () => ({
  batchResolveFilePreviews: vi.fn(),
  filePreviewStaleTime: vi.fn(() => 60_000),
  getFilePreviewUrl: vi.fn()
}));

vi.mock("../../api/nodes", () => ({
  getNode: vi.fn()
}));

const sourceNode = makeRestNode({
  id: "source-1",
  name: "source.md",
  path: "/docs/source.md",
});

describe("useMarkdownImageLoader", () => {
  beforeEach(() => {
    vi.mocked(batchResolveFilePreviews).mockReset();
    vi.mocked(getFilePreviewUrl).mockReset();
  });

  it("loads a markdown image through the batch endpoint", async () => {
    vi.mocked(batchResolveFilePreviews).mockResolvedValue({
      results: [batchPreview("/docs/assets/diagram.png")]
    });

    const { result } = renderHook(() => useMarkdownImageLoader(sourceNode), { wrapper: createQueryWrapper() });

    await expect(result.current("/docs/assets/diagram.png")).resolves.toEqual({ status: "loaded", url: "https://storage.example/preview" });
    expect(batchResolveFilePreviews).toHaveBeenCalledWith(
      mockClient,
      "space-1",
      ["/docs/assets/diagram.png"]
    );
  });

  it("coalesces twenty near-viewport images into one request", async () => {
    const paths = Array.from({ length: 20 }, (_, index) => `/docs/image-${index}.png`);
    vi.mocked(batchResolveFilePreviews).mockImplementation(async (_client, _spaceId, requested) => ({
      results: requested.map((path, index) => batchPreview(path, `image-${index}`))
    }));

    const { result } = renderHook(() => useMarkdownImageLoader(sourceNode), { wrapper: createQueryWrapper() });

    await Promise.all(paths.map((path) => result.current(path)));

    expect(batchResolveFilePreviews).toHaveBeenCalledTimes(1);
    expect(batchResolveFilePreviews).toHaveBeenCalledWith(mockClient, "space-1", paths);
  });

  it("reuses a cached batch result for repeated markdown image loads", async () => {
    vi.mocked(batchResolveFilePreviews).mockResolvedValue({
      results: [batchPreview("/docs/assets/diagram.png")]
    });
    const { result } = renderHook(() => useMarkdownImageLoader(sourceNode), { wrapper: createQueryWrapper() });

    await result.current("/docs/assets/diagram.png");
    await result.current("/docs/assets/diagram.png");

    expect(batchResolveFilePreviews).toHaveBeenCalledTimes(1);
  });

  it("refreshes a cached batch result when image recovery is requested", async () => {
    vi.mocked(batchResolveFilePreviews)
      .mockResolvedValueOnce({ results: [batchPreview("/docs/image.png")] })
      .mockResolvedValueOnce({
        results: [{
          ...batchPreview("/docs/image.png"),
          url: "https://storage.example/refreshed"
        }]
      });
    const { result } = renderHook(() => useMarkdownImageLoader(sourceNode), { wrapper: createQueryWrapper() });

    await result.current("/docs/image.png");
    await expect(result.current("/docs/image.png", { forceRefresh: true })).resolves.toEqual({
      status: "loaded",
      url: "https://storage.example/refreshed"
    });
    expect(batchResolveFilePreviews).toHaveBeenCalledTimes(2);
  });

  it("maps per-path missing, unsupported, and error results", async () => {
    vi.mocked(batchResolveFilePreviews).mockImplementation(async (_client, _spaceId, paths) => ({
      results: paths.map((path) => ({
        path,
        status: path.includes("missing")
          ? "not_found" as const
          : path.includes("unsupported")
            ? "unsupported" as const
            : "error" as const,
        node_id: null,
        media_type: null,
        url: null,
        expires_at: null
      }))
    }));
    const { result } = renderHook(() => useMarkdownImageLoader(sourceNode), { wrapper: createQueryWrapper() });

    await expect(result.current("/docs/missing.png")).resolves.toEqual({ status: "not-found" });
    await expect(result.current("/docs/unsupported.txt")).resolves.toEqual({ status: "unsupported" });
    await expect(result.current("/docs/error.png")).resolves.toEqual({ status: "error" });
  });
});

describe("useFilePreviewUrl", () => {
  beforeEach(() => {
    vi.mocked(getFilePreviewUrl).mockReset();
    vi.mocked(getNode).mockReset();
  });

  it("patches node collections without refetching after legacy preview metadata is discovered", async () => {
    const queryClient = createTestQueryClient();
    const invalidate = vi.spyOn(queryClient, "invalidateQueries");
    const imageNode = fileNode({
      id: "legacy-image",
      parent_id: "folder-1",
      detected_media_type: undefined,
      preview_available: undefined
    });
    vi.mocked(getFilePreviewUrl).mockResolvedValue(previewUrl());
    const page = { limit: 100, returned: 1, has_more: false, next_cursor: null };
    queryClient.setQueryData(queryKeys.recent("space-1"), {
      pages: [{ nodes: [imageNode], page }],
      pageParams: [null]
    });
    queryClient.setQueryData(queryKeys.children("space-1", "folder-1"), {
      pages: [{
        parent: { id: "folder-1", path: "/docs" },
        children: [imageNode],
        page
      }],
      pageParams: [null]
    });

    const { result } = renderHook(() => useFilePreviewUrl(imageNode), {
      wrapper: createQueryWrapper(queryClient)
    });

    await waitFor(() => expect(result.current.isSuccess).toBe(true));
    expect(getFilePreviewUrl).toHaveBeenCalledWith(mockClient, "space-1", "legacy-image", "image");
    expect(invalidate).not.toHaveBeenCalled();
    expect(queryClient.getQueryData<{ pages: Array<{ nodes: RestNode[] }> }>(
      queryKeys.recent("space-1")
    )?.pages[0]?.nodes[0]).toMatchObject({
      preview_available: true,
      file_preview_kind: "image"
    });
    expect(queryClient.getQueryData<{ pages: Array<{ children: RestNode[] }> }>(
      queryKeys.children("space-1", "folder-1")
    )?.pages[0]?.children[0]).toMatchObject({
      preview_available: true,
      file_preview_kind: "image"
    });
  });

  it("shares a preview URL across stale node snapshots of the same immutable file", async () => {
    const queryClient = createTestQueryClient();
    const olderNode = fileNode({ updated_at: "2026-06-13T00:00:00Z" });
    const newerNode = fileNode({ updated_at: "2026-06-14T00:00:00Z" });
    vi.mocked(getFilePreviewUrl).mockResolvedValue(previewUrl());

    const first = renderHook(() => useFilePreviewUrl(olderNode), {
      wrapper: createQueryWrapper(queryClient)
    });
    await waitFor(() => expect(first.result.current.isSuccess).toBe(true));
    const cachedPreview = queryClient.getQueryCache().find({
      queryKey: queryKeys.filePreviewUrl("space-1", "file-1", "image"),
      exact: true
    });
    expect(cachedPreview?.options.gcTime).toBe(15 * 60 * 1_000);

    const second = renderHook(() => useFilePreviewUrl(newerNode), {
      wrapper: createQueryWrapper(queryClient)
    });
    await waitFor(() => expect(second.result.current.isSuccess).toBe(true));

    expect(getFilePreviewUrl).toHaveBeenCalledTimes(1);
  });

  it("uses the PDF endpoint and cache key for PDF file previews", async () => {
    const queryClient = createTestQueryClient();
    const pdfNode = fileNode({
      name: "document.pdf",
      media_type: "application/pdf",
      detected_media_type: "application/pdf",
      preview_available: false,
      file_preview_kind: "pdf"
    });
    vi.mocked(getFilePreviewUrl).mockResolvedValue({
      url: "https://storage.example/document.pdf",
      media_type: "application/pdf",
      expires_at: "2026-06-13T00:15:00Z"
    });

    const { result } = renderHook(() => useFilePreviewUrl(pdfNode), {
      wrapper: createQueryWrapper(queryClient)
    });

    await waitFor(() => expect(result.current.isSuccess).toBe(true));

    expect(getFilePreviewUrl).toHaveBeenCalledWith(mockClient, "space-1", "file-1", "pdf");
    expect(queryClient.getQueryData(
      queryKeys.filePreviewUrl("space-1", "file-1", "pdf")
    )).toMatchObject({ media_type: "application/pdf" });
  });

  it("clears stale PDF preview metadata when the backend rejects it", async () => {
    const queryClient = createTestQueryClient();
    const pdfNode = fileNode({
      name: "document.pdf",
      media_type: "application/pdf",
      detected_media_type: "application/pdf",
      preview_available: false,
      file_preview_kind: "pdf"
    });
    vi.mocked(getFilePreviewUrl).mockRejectedValue(new ApiError("not previewable", 404));
    vi.mocked(getNode).mockResolvedValue({
      ...pdfNode,
      preview_available: false,
      file_preview_kind: undefined
    });

    const { result } = renderHook(() => useFilePreviewUrl(pdfNode), {
      wrapper: createQueryWrapper(queryClient)
    });

    await waitFor(() => expect(result.current.isError).toBe(true));

    expect(queryClient.getQueryData<RestNode>(
      queryKeys.node("space-1", "file-1")
    )).toMatchObject({
      preview_available: false,
      file_preview_kind: undefined
    });
  });

  it("refreshes the node after a declared PDF is detected as an image", async () => {
    const queryClient = createTestQueryClient();
    const declaredPdf = fileNode({
      name: "document.pdf",
      media_type: "application/pdf",
      detected_media_type: undefined,
      preview_available: undefined,
      file_preview_kind: undefined
    });
    const detectedImage = {
      ...declaredPdf,
      detected_media_type: "image/png",
      preview_available: true,
      file_preview_kind: "image" as const
    };
    vi.mocked(getFilePreviewUrl).mockRejectedValue(new ApiError("not a PDF", 404));
    vi.mocked(getNode).mockResolvedValue(detectedImage);

    const { result } = renderHook(() => useFilePreviewUrl(declaredPdf), {
      wrapper: createQueryWrapper(queryClient)
    });

    await waitFor(() => expect(result.current.isError).toBe(true));

    expect(getNode).toHaveBeenCalledWith(mockClient, "space-1", "file-1");
    const refreshedNode = queryClient.getQueryData<RestNode>(
      queryKeys.node("space-1", "file-1")
    );
    expect(refreshedNode).toMatchObject({
      detected_media_type: "image/png",
      preview_available: true,
      file_preview_kind: "image"
    });
    expect(filePreviewKindForNode(refreshedNode!)).toBe("image");
  });
});

describe("filePreviewKindForNode", () => {
  it("uses backend preview kind when available", () => {
    const imageNode = fileNode({ preview_available: true, file_preview_kind: "image" });
    const pdfNode = fileNode({ preview_available: false, file_preview_kind: "pdf" });

    expect(filePreviewKindForNode(imageNode)).toBe("image");
    expect(filePreviewKindForNode(pdfNode)).toBe("pdf");
  });

  it("keeps legacy image probing and recognizes declared PDFs before discovery", () => {
    expect(filePreviewKindForNode(fileNode({ preview_available: undefined }))).toBe("image");
    expect(filePreviewKindForNode(fileNode({
      media_type: "application/pdf",
      preview_available: undefined
    }))).toBe("pdf");
  });

  it("does not preview encrypted or known unsupported files", () => {
    expect(filePreviewKindForNode(fileNode({ encryption_mode: "client" }))).toBeNull();
    expect(filePreviewKindForNode(fileNode({ preview_available: false }))).toBeNull();
  });
});

function createQueryWrapper(queryClient = createTestQueryClient()) {
  return function QueryWrapper({ children }: { children: ReactNode }) {
    return <QueryClientProvider client={queryClient}>{children}</QueryClientProvider>;
  };
}

function previewUrl() {
  return {
    url: "https://storage.example/preview",
    media_type: "image/png",
    expires_at: "2026-06-13T00:15:00Z"
  };
}

function batchPreview(path: string, nodeId = "image-1") {
  return {
    path,
    status: "ready" as const,
    node_id: nodeId,
    media_type: "image/png",
    url: "https://storage.example/preview",
    expires_at: "2026-06-13T00:15:00Z"
  };
}

function fileNode(overrides: Partial<RestNode>): RestNode {
  return makeRestNode({
    ...sourceNode,
    id: "file-1",
    kind: "file",
    name: "image.png",
    path: "/docs/image.png",
    media_type: "image/png",
    encryption_mode: "none",
    ...overrides
  });
}
