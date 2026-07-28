import { QueryClient } from "@tanstack/react-query";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { batchResolveFilePreviews } from "../../api/files";
import { queryKeys } from "../../api/queryKeys";
import type { RestNode } from "../../api/types";
import { createMockApiClient } from "../../test/apiClient";
import { createMarkdownPreviewBatcher } from "./markdownPreviewBatcher";

vi.mock("../../api/files", () => ({
  batchResolveFilePreviews: vi.fn()
}));

describe("createMarkdownPreviewBatcher", () => {
  beforeEach(() => {
    vi.mocked(batchResolveFilePreviews).mockReset();
  });

  it("deduplicates simultaneous requests for the same path", async () => {
    vi.mocked(batchResolveFilePreviews).mockResolvedValue({
      results: [ready("/image.png", "node-1")]
    });
    const load = createMarkdownPreviewBatcher(
      createMockApiClient(),
      new QueryClient(),
      "space-1"
    );

    const [first, second] = await Promise.all([
      load("/image.png"),
      load("/image.png")
    ]);

    expect(first).toEqual(second);
    expect(batchResolveFilePreviews).toHaveBeenCalledOnce();
    expect(batchResolveFilePreviews).toHaveBeenCalledWith(
      expect.anything(),
      "space-1",
      ["/image.png"]
    );
  });

  it("rejects the whole batch before caching an out-of-order response", async () => {
    vi.mocked(batchResolveFilePreviews).mockResolvedValue({
      results: [
        ready("/second.png", "node-2"),
        ready("/first.png", "node-1")
      ]
    });
    const queryClient = new QueryClient();
    const load = createMarkdownPreviewBatcher(
      createMockApiClient(),
      queryClient,
      "space-1"
    );

    const results = await Promise.allSettled([
      load("/first.png"),
      load("/second.png")
    ]);

    expect(results.every(({ status }) => status === "rejected")).toBe(true);
    expect(queryClient.getQueryData(
      queryKeys.filePreviewUrl("space-1", "node-1", "image")
    )).toBeUndefined();
    expect(queryClient.getQueryData(
      queryKeys.filePreviewUrl("space-1", "node-2", "image")
    )).toBeUndefined();
  });

  it("partitions more than 64 paths without losing order", async () => {
    vi.mocked(batchResolveFilePreviews).mockImplementation(
      async (_client, _spaceId, paths) => ({
        results: paths.map((path, index) => ready(path, `${path}-${index}`))
      })
    );
    const load = createMarkdownPreviewBatcher(
      createMockApiClient(),
      new QueryClient(),
      "space-1"
    );
    const paths = Array.from({ length: 65 }, (_, index) => `/image-${index}.png`);

    const results = await Promise.all(paths.map(load));

    expect(results.map(({ path }) => path)).toEqual(paths);
    expect(vi.mocked(batchResolveFilePreviews).mock.calls[0]?.[2]).toHaveLength(64);
    expect(vi.mocked(batchResolveFilePreviews).mock.calls[1]?.[2]).toHaveLength(1);
  });

  it("preserves PDF detail preview metadata when Markdown image resolution rejects it", async () => {
    vi.mocked(batchResolveFilePreviews).mockResolvedValue({
      results: [{
        path: "/document.pdf",
        status: "unsupported",
        node_id: "pdf-1",
        media_type: "application/pdf",
        url: null,
        expires_at: null
      }]
    });
    const queryClient = new QueryClient();
    queryClient.setQueryData(
      queryKeys.node("space-1", "pdf-1"),
      { id: "pdf-1", space_id: "space-1", kind: "file" } as RestNode
    );
    const load = createMarkdownPreviewBatcher(
      createMockApiClient(),
      queryClient,
      "space-1"
    );

    await load("/document.pdf");

    expect(queryClient.getQueryData<RestNode>(
      queryKeys.node("space-1", "pdf-1")
    )).toMatchObject({
      detected_media_type: "application/pdf",
      preview_available: false,
      file_preview_kind: "pdf"
    });
  });
});

function ready(path: string, nodeId: string) {
  return {
    path,
    status: "ready" as const,
    node_id: nodeId,
    media_type: "image/png",
    url: `https://storage.example/${nodeId}`,
    expires_at: "2026-06-13T00:15:00Z"
  };
}
