import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { renderHook, waitFor } from "@testing-library/react";
import type { ReactNode } from "react";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { batchResolveFilePreviews, getFilePreviewUrl } from "../../api/files";
import { queryKeys } from "../../api/queryKeys";
import type { RestNode } from "../../api/types";
import { useFilePreviewUrl, useMarkdownImageLoader } from "./useFilePreviewQueries";

const mockClient = vi.hoisted(() => ({}));

vi.mock("../../api/ApiProvider", () => ({
  useApiClient: () => mockClient
}));

vi.mock("../../api/files", () => ({
  batchResolveFilePreviews: vi.fn(),
  filePreviewStaleTime: vi.fn(() => 60_000),
  getFilePreviewUrl: vi.fn()
}));

const sourceNode: RestNode = {
  id: "source-1",
  space_id: "space-1",
  parent_id: "root-1",
  name: "source.md",
  kind: "text",
  path: "/docs/source.md",
  sort_order: 0,
  metadata: {},
  has_children: false,
  created_by: { id: "user-1", kind: "user", display_name: "User" },
  updated_by: { id: "user-1", kind: "user", display_name: "User" },
  created_at: "2026-06-13T00:00:00Z",
  updated_at: "2026-06-13T00:00:00Z"
};

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
  });

  it("patches node collections without refetching after legacy preview metadata is discovered", async () => {
    const queryClient = createQueryClient();
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
    expect(invalidate).not.toHaveBeenCalled();
    expect(queryClient.getQueryData<{ pages: Array<{ nodes: RestNode[] }> }>(
      queryKeys.recent("space-1")
    )?.pages[0]?.nodes[0]).toMatchObject({ preview_available: true });
    expect(queryClient.getQueryData<{ pages: Array<{ children: RestNode[] }> }>(
      queryKeys.children("space-1", "folder-1")
    )?.pages[0]?.children[0]).toMatchObject({ preview_available: true });
  });

  it("shares a preview URL across stale node snapshots of the same immutable file", async () => {
    const queryClient = createQueryClient();
    const olderNode = fileNode({ updated_at: "2026-06-13T00:00:00Z" });
    const newerNode = fileNode({ updated_at: "2026-06-14T00:00:00Z" });
    vi.mocked(getFilePreviewUrl).mockResolvedValue(previewUrl());

    const first = renderHook(() => useFilePreviewUrl(olderNode), {
      wrapper: createQueryWrapper(queryClient)
    });
    await waitFor(() => expect(first.result.current.isSuccess).toBe(true));
    const cachedPreview = queryClient.getQueryCache().find({
      queryKey: queryKeys.filePreviewUrl("space-1", "file-1"),
      exact: true
    });
    expect(cachedPreview?.options.gcTime).toBe(15 * 60 * 1_000);

    const second = renderHook(() => useFilePreviewUrl(newerNode), {
      wrapper: createQueryWrapper(queryClient)
    });
    await waitFor(() => expect(second.result.current.isSuccess).toBe(true));

    expect(getFilePreviewUrl).toHaveBeenCalledTimes(1);
  });
});

function createQueryClient() {
  return new QueryClient({
    defaultOptions: {
      queries: {
        retry: false
      }
    }
  });
}

function createQueryWrapper(queryClient = createQueryClient()) {
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
  return {
    ...sourceNode,
    id: "file-1",
    kind: "file",
    name: "image.png",
    path: "/docs/image.png",
    media_type: "image/png",
    encryption_mode: "none",
    ...overrides
  };
}
