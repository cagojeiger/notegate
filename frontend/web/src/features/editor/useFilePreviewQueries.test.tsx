import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { renderHook, waitFor } from "@testing-library/react";
import type { ReactNode } from "react";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { ApiError } from "../../api/errors";
import { getFilePreviewUrl } from "../../api/files";
import { resolveNodePath } from "../../api/nodes";
import { queryKeys } from "../../api/queryKeys";
import type { RestNode } from "../../api/types";
import { useFilePreviewUrl, useMarkdownImageLoader } from "./useFilePreviewQueries";

const mockClient = vi.hoisted(() => ({}));

vi.mock("../../api/ApiProvider", () => ({
  useApiClient: () => mockClient
}));

vi.mock("../../api/files", () => ({
  filePreviewStaleTime: vi.fn(() => 60_000),
  getFilePreviewUrl: vi.fn()
}));

vi.mock("../../api/nodes", () => ({
  resolveNodePath: vi.fn()
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
    vi.mocked(resolveNodePath).mockReset();
    vi.mocked(getFilePreviewUrl).mockReset();
  });

  it("resolves markdown image paths and requests preview URLs", async () => {
    const imageNode = fileNode({ id: "image-1", path: "/docs/assets/diagram.png", media_type: "image/png" });
    vi.mocked(resolveNodePath).mockResolvedValue(imageNode);
    vi.mocked(getFilePreviewUrl).mockResolvedValue(previewUrl());

    const { result } = renderHook(() => useMarkdownImageLoader(sourceNode), { wrapper: createQueryWrapper() });

    await expect(result.current("/docs/assets/diagram.png")).resolves.toEqual({ status: "loaded", url: "https://storage.example/preview" });
    expect(resolveNodePath).toHaveBeenCalledWith(mockClient, "space-1", "/docs/assets/diagram.png");
    expect(getFilePreviewUrl).toHaveBeenCalledWith(mockClient, "space-1", "image-1");
  });

  it("reuses cached node resolution and preview URLs for repeated markdown image loads", async () => {
    const imageNode = fileNode({ id: "image-1", path: "/docs/assets/diagram.png", media_type: "image/png" });
    vi.mocked(resolveNodePath).mockResolvedValue(imageNode);
    vi.mocked(getFilePreviewUrl).mockResolvedValue(previewUrl());

    const { result } = renderHook(() => useMarkdownImageLoader(sourceNode), { wrapper: createQueryWrapper() });

    await expect(result.current("/docs/assets/diagram.png")).resolves.toEqual({ status: "loaded", url: "https://storage.example/preview" });
    await expect(result.current("/docs/assets/diagram.png")).resolves.toEqual({ status: "loaded", url: "https://storage.example/preview" });
    expect(resolveNodePath).toHaveBeenCalledTimes(1);
    expect(getFilePreviewUrl).toHaveBeenCalledTimes(1);
  });

  it("refreshes a cached preview URL when an image load asks for recovery", async () => {
    const imageNode = fileNode({ id: "image-1", path: "/docs/assets/diagram.png", media_type: "image/png" });
    vi.mocked(resolveNodePath).mockResolvedValue(imageNode);
    vi.mocked(getFilePreviewUrl)
      .mockResolvedValueOnce(previewUrl())
      .mockResolvedValueOnce({ ...previewUrl(), url: "https://storage.example/refreshed" });
    const { result } = renderHook(() => useMarkdownImageLoader(sourceNode), { wrapper: createQueryWrapper() });

    await expect(result.current("/docs/assets/diagram.png")).resolves.toEqual({
      status: "loaded",
      url: "https://storage.example/preview"
    });
    await expect(result.current("/docs/assets/diagram.png", { forceRefresh: true })).resolves.toEqual({
      status: "loaded",
      url: "https://storage.example/refreshed"
    });
    expect(resolveNodePath).toHaveBeenCalledTimes(1);
    expect(getFilePreviewUrl).toHaveBeenCalledTimes(2);
  });

  it("returns not-found when the image path does not resolve", async () => {
    vi.mocked(resolveNodePath).mockRejectedValue(new ApiError("missing", 404));

    const { result } = renderHook(() => useMarkdownImageLoader(sourceNode), { wrapper: createQueryWrapper() });

    await expect(result.current("/docs/assets/missing.png")).resolves.toEqual({ status: "not-found" });
    expect(getFilePreviewUrl).not.toHaveBeenCalled();
  });

  it("does not download resolved nodes that are not displayable images", async () => {
    const { result } = renderHook(() => useMarkdownImageLoader(sourceNode), { wrapper: createQueryWrapper() });

    vi.mocked(resolveNodePath).mockResolvedValueOnce({ ...sourceNode, path: "/docs/note.md" });
    await expect(result.current("/docs/note.md")).resolves.toEqual({ status: "unsupported" });

    vi.mocked(resolveNodePath).mockResolvedValueOnce(fileNode({
      id: "text-file-1",
      path: "/docs/file.txt",
      media_type: "text/plain",
      detected_media_type: "text/plain",
      preview_available: false
    }));
    await expect(result.current("/docs/file.txt")).resolves.toEqual({ status: "unsupported" });

    vi.mocked(resolveNodePath).mockResolvedValueOnce(fileNode({
      id: "fake-image-1",
      path: "/docs/document.png",
      media_type: "image/png",
      detected_media_type: "application/pdf",
      preview_available: false
    }));
    await expect(result.current("/docs/document.png")).resolves.toEqual({ status: "unsupported" });

    vi.mocked(resolveNodePath).mockResolvedValueOnce(fileNode({ id: "encrypted-1", path: "/docs/secret.png", encryption_mode: "client", media_type: "image/png" }));
    await expect(result.current("/docs/secret.png")).resolves.toEqual({ status: "unsupported" });

    vi.mocked(resolveNodePath).mockResolvedValueOnce(fileNode({ id: "other-space-1", space_id: "space-2", path: "/docs/other.png", media_type: "image/png" }));
    await expect(result.current("/docs/other.png")).resolves.toEqual({ status: "unsupported" });

    expect(getFilePreviewUrl).not.toHaveBeenCalled();
  });

  it("uses detected image bytes even when the client declared another media type", async () => {
    vi.mocked(resolveNodePath).mockResolvedValue(fileNode({
      id: "detected-image-1",
      media_type: "application/octet-stream",
      detected_media_type: "image/webp",
      preview_available: true
    }));
    vi.mocked(getFilePreviewUrl).mockResolvedValue({ ...previewUrl(), media_type: "image/webp" });
    const { result } = renderHook(() => useMarkdownImageLoader(sourceNode), { wrapper: createQueryWrapper() });

    await expect(result.current("/docs/image.webp")).resolves.toEqual({
      status: "loaded",
      url: "https://storage.example/preview"
    });
    expect(getFilePreviewUrl).toHaveBeenCalledTimes(1);
  });

  it("reports an unsupported image when server verification rejects it", async () => {
    vi.mocked(resolveNodePath).mockResolvedValue(fileNode({
      media_type: "text/plain",
      detected_media_type: undefined,
      preview_available: undefined
    }));
    vi.mocked(getFilePreviewUrl).mockRejectedValue(new ApiError("not previewable", 404));
    const { result } = renderHook(() => useMarkdownImageLoader(sourceNode), { wrapper: createQueryWrapper() });

    await expect(result.current("/docs/image.png")).resolves.toEqual({ status: "unsupported" });
  });
});

describe("useFilePreviewUrl", () => {
  beforeEach(() => {
    vi.mocked(getFilePreviewUrl).mockReset();
  });

  it("refreshes node collections after legacy preview metadata is discovered", async () => {
    const queryClient = createQueryClient();
    const invalidate = vi.spyOn(queryClient, "invalidateQueries");
    const imageNode = fileNode({
      id: "legacy-image",
      parent_id: "folder-1",
      detected_media_type: undefined,
      preview_available: undefined
    });
    vi.mocked(getFilePreviewUrl).mockResolvedValue(previewUrl());

    const { result } = renderHook(() => useFilePreviewUrl(imageNode), {
      wrapper: createQueryWrapper(queryClient)
    });

    await waitFor(() => expect(result.current.isSuccess).toBe(true));
    expect(invalidate).toHaveBeenCalledWith({ queryKey: queryKeys.node("space-1", "legacy-image") });
    expect(invalidate).toHaveBeenCalledWith({ queryKey: queryKeys.recent("space-1") });
    expect(invalidate).toHaveBeenCalledWith({ queryKey: queryKeys.children("space-1", "folder-1") });
    expect(queryClient.getQueryData(queryKeys.markdownImageNode("space-1", "/docs/image.png")))
      .toMatchObject({ detected_media_type: "image/png", preview_available: true });
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
