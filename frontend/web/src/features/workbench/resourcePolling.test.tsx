import { QueryClientProvider } from "@tanstack/react-query";
import { renderHook, waitFor } from "@testing-library/react";
import type { PropsWithChildren } from "react";
import { describe, expect, it, vi } from "vitest";

import type { ApiClient } from "../../api/client";
import { queryKeys } from "../../api/queryKeys";
import { createMockApiClient } from "../../test/apiClient";
import { makeRestNode } from "../../test/fixtures";
import { createTestQueryClient } from "../../test/queryClient";
import { useNodeFreshness, useTextDocument } from "../editor/useEditorQueries";
import { useNodeChildrenQuery, useRecentNodesQuery } from "../nodes/useNodeQueries";

const apiClientState = vi.hoisted(
  (): { client: ApiClient | null } => ({ client: null })
);

vi.mock("../../api/ApiProvider", () => ({
  useApiClient: () => apiClientState.client!
}));

const client = createMockApiClient();
client.get.mockImplementation((path: string) => {
  if (path.includes("/children")) {
    return Promise.resolve({
      parent: { id: "root-1", path: "/" },
      children: [],
      page: page()
    });
  }
  if (path.includes("?limit=50")) {
    return Promise.resolve({ nodes: [], page: page() });
  }
  if (path.includes("/text/")) {
    return Promise.resolve({
      node: { id: node.id, path: node.path },
      text: {
        node_id: node.id,
        storage_format: "plain",
        content: "# Note",
        content_sha256: node.content_sha256,
        byte_len: 6,
        line_count: 1,
        start_line: 1,
        end_line: 1,
        returned_lines: 1,
        truncated: false,
        next_start_line: null,
        updated_by: node.updated_by,
        updated_at: node.updated_at
      }
    });
  }
  return Promise.resolve(node);
});
const get = client.get;
apiClientState.client = client;

describe("workspace resource freshness", () => {
  it("lets Space change sync own focus refresh without adding per-resource polling", async () => {
    const queryClient = createTestQueryClient({
      defaultOptions: {
        queries: {
          refetchOnWindowFocus: true,
          refetchOnReconnect: true,
          staleTime: 5_000
        }
      }
    });
    const wrapper = ({ children }: PropsWithChildren) => (
      <QueryClientProvider client={queryClient}>{children}</QueryClientProvider>
    );

    renderHook(() => {
      useNodeChildrenQuery("space-1", "root-1", true);
      useRecentNodesQuery("space-1");
      useNodeFreshness(node);
      useTextDocument(node);
    }, { wrapper });

    await waitFor(() => expect(get).toHaveBeenCalledTimes(4));

    const resourceQueries = queryClient.getQueryCache().findAll({
      queryKey: ["spaces", "space-1"]
    });
    expect(resourceQueries).toHaveLength(4);
    expect(resourceQueries.every((query) => !Object.prototype.hasOwnProperty.call(query.options, "refetchInterval"))).toBe(true);
    expect(queryClient.getQueryCache().find({
      queryKey: queryKeys.node(node.space_id, node.id),
      exact: true
    })?.observers[0]?.options.refetchOnWindowFocus).toBe(false);
    expect(queryClient.getQueryCache().find({
      queryKey: queryKeys.text(node.space_id, node.id),
      exact: true
    })?.observers[0]?.options.refetchOnWindowFocus).toBe(false);
    expect(queryClient.getQueryCache().find({
      queryKey: queryKeys.node(node.space_id, node.id),
      exact: true
    })?.observers[0]?.options.refetchOnReconnect).toBe(true);
    expect(queryClient.getQueryCache().find({
      queryKey: queryKeys.text(node.space_id, node.id),
      exact: true
    })?.observers[0]?.options.refetchOnReconnect).toBe(true);
    expect(queryClient.getQueryCache().find({
      queryKey: queryKeys.children(node.space_id, "root-1"),
      exact: true
    })?.observers[0]?.options.staleTime).toBe(Number.POSITIVE_INFINITY);
    expect(queryClient.getQueryCache().find({
      queryKey: queryKeys.recent(node.space_id),
      exact: true
    })?.observers[0]?.options.staleTime).toBe(Number.POSITIVE_INFINITY);
  });
});

const node = makeRestNode();

function page() {
  return {
    limit: 50,
    returned: 0,
    has_more: false,
    next_cursor: null
  };
}
