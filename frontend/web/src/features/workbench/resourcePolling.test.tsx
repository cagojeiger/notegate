import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { renderHook, waitFor } from "@testing-library/react";
import type { PropsWithChildren } from "react";
import { describe, expect, it, vi } from "vitest";

import type { ApiClient } from "../../api/client";
import { makeRestNode } from "../../test/fixtures";
import { createMockApiClient } from "../../test/apiClient";
import { useNodeFreshness } from "../editor/useEditorQueries";
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
  return Promise.resolve(node);
});
const get = client.get;
apiClientState.client = client;

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

const node = makeRestNode();

function page() {
  return {
    limit: 50,
    returned: 0,
    has_more: false,
    next_cursor: null
  };
}
