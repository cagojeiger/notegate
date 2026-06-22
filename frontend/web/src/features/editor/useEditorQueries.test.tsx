import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { renderHook } from "@testing-library/react";
import type { ReactNode } from "react";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { ApiError } from "../../api/errors";
import { downloadFile } from "../../api/files";
import { resolveNodePath } from "../../api/nodes";
import type { RestNode } from "../../api/types";
import { useMarkdownImageLoader } from "./useEditorQueries";

const mockClient = vi.hoisted(() => ({}));

vi.mock("../../api/ApiProvider", () => ({
  useApiClient: () => mockClient
}));

vi.mock("../../api/files", () => ({
  downloadFile: vi.fn()
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
    vi.mocked(downloadFile).mockReset();
  });

  it("resolves markdown image paths and downloads image files", async () => {
    const blob = new Blob(["image"], { type: "image/png" });
    const imageNode = fileNode({ id: "image-1", path: "/docs/assets/diagram.png", media_type: "image/png" });
    vi.mocked(resolveNodePath).mockResolvedValue(imageNode);
    vi.mocked(downloadFile).mockResolvedValue(blob);

    const { result } = renderHook(() => useMarkdownImageLoader(sourceNode), { wrapper: createQueryWrapper() });

    await expect(result.current("/docs/assets/diagram.png")).resolves.toEqual({ status: "loaded", blob });
    expect(resolveNodePath).toHaveBeenCalledWith(mockClient, "space-1", "/docs/assets/diagram.png");
    expect(downloadFile).toHaveBeenCalledWith(mockClient, "space-1", "image-1");
  });

  it("reuses cached node resolution and blobs for repeated markdown image loads", async () => {
    const blob = new Blob(["image"], { type: "image/png" });
    const imageNode = fileNode({ id: "image-1", path: "/docs/assets/diagram.png", media_type: "image/png", content_sha256: "sha-1" });
    vi.mocked(resolveNodePath).mockResolvedValue(imageNode);
    vi.mocked(downloadFile).mockResolvedValue(blob);

    const { result } = renderHook(() => useMarkdownImageLoader(sourceNode), { wrapper: createQueryWrapper() });

    await expect(result.current("/docs/assets/diagram.png")).resolves.toEqual({ status: "loaded", blob });
    await expect(result.current("/docs/assets/diagram.png")).resolves.toEqual({ status: "loaded", blob });
    expect(resolveNodePath).toHaveBeenCalledTimes(1);
    expect(downloadFile).toHaveBeenCalledTimes(1);
  });

  it("returns not-found when the image path does not resolve", async () => {
    vi.mocked(resolveNodePath).mockRejectedValue(new ApiError("missing", 404));

    const { result } = renderHook(() => useMarkdownImageLoader(sourceNode), { wrapper: createQueryWrapper() });

    await expect(result.current("/docs/assets/missing.png")).resolves.toEqual({ status: "not-found" });
    expect(downloadFile).not.toHaveBeenCalled();
  });

  it("does not download resolved nodes that are not displayable images", async () => {
    const { result } = renderHook(() => useMarkdownImageLoader(sourceNode), { wrapper: createQueryWrapper() });

    vi.mocked(resolveNodePath).mockResolvedValueOnce({ ...sourceNode, path: "/docs/note.md" });
    await expect(result.current("/docs/note.md")).resolves.toEqual({ status: "unsupported" });

    vi.mocked(resolveNodePath).mockResolvedValueOnce(fileNode({ id: "text-file-1", path: "/docs/file.txt", media_type: "text/plain" }));
    await expect(result.current("/docs/file.txt")).resolves.toEqual({ status: "unsupported" });

    vi.mocked(resolveNodePath).mockResolvedValueOnce(fileNode({ id: "encrypted-1", path: "/docs/secret.png", encryption_mode: "client", media_type: "image/png" }));
    await expect(result.current("/docs/secret.png")).resolves.toEqual({ status: "unsupported" });

    vi.mocked(resolveNodePath).mockResolvedValueOnce(fileNode({ id: "other-space-1", space_id: "space-2", path: "/docs/other.png", media_type: "image/png" }));
    await expect(result.current("/docs/other.png")).resolves.toEqual({ status: "unsupported" });

    expect(downloadFile).not.toHaveBeenCalled();
  });
});

function createQueryWrapper() {
  const queryClient = new QueryClient({
    defaultOptions: {
      queries: {
        retry: false
      }
    }
  });

  return function QueryWrapper({ children }: { children: ReactNode }) {
    return <QueryClientProvider client={queryClient}>{children}</QueryClientProvider>;
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
    content_sha256: "sha",
    ...overrides
  };
}
