import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { renderHook, waitFor } from "@testing-library/react";
import type { PropsWithChildren } from "react";
import { describe, expect, it, vi } from "vitest";

import type { ApiClient } from "../../api/client";
import type { RestNode } from "../../api/types";
import { useNodeFreshness } from "../editor/useEditorQueries";
import { useNodeChildrenQuery, useRecentNodesQuery } from "../nodes/useNodeQueries";

const get = vi.fn((path: string) => {
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
  return Promise.resolve(node);
});
const client = { get } as unknown as ApiClient;

vi.mock("../../api/ApiProvider", () => ({
  useApiClient: () => client
}));

describe("workspace resource freshness", () => {
  it("does not attach independent polling intervals to tree, recent, or opened-node queries", async () => {
    const queryClient = new QueryClient({
      defaultOptions: { queries: { retry: false } }
    });
    const wrapper = ({ children }: PropsWithChildren) => (
      <QueryClientProvider client={queryClient}>{children}</QueryClientProvider>
    );

    renderHook(() => {
      useNodeChildrenQuery("space-1", "root-1", true);
      useRecentNodesQuery("space-1");
      useNodeFreshness(node);
    }, { wrapper });

    await waitFor(() => expect(get).toHaveBeenCalledTimes(3));

    const resourceQueries = queryClient.getQueryCache().findAll({
      queryKey: ["spaces", "space-1"]
    });
    expect(resourceQueries).toHaveLength(3);
    expect(resourceQueries.every((query) => !Object.prototype.hasOwnProperty.call(query.options, "refetchInterval"))).toBe(true);
  });
});

const node: RestNode = {
  id: "node-1",
  space_id: "space-1",
  parent_id: "root-1",
  name: "note.md",
  kind: "text",
  path: "/note.md",
  sort_order: 0,
  metadata: {},
  has_children: false,
  created_by: { id: "user-1", kind: "user", display_name: "User" },
  updated_by: { id: "user-1", kind: "user", display_name: "User" },
  created_at: "2026-07-24T00:00:00Z",
  updated_at: "2026-07-24T00:00:00Z"
};

function page() {
  return {
    limit: 50,
    returned: 0,
    has_more: false,
    next_cursor: null
  };
}
