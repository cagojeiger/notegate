import { describe, expect, it, vi } from "vitest";

import type { ApiClient } from "./client";
import { batchListChildren, listChildren, listNodes, resolveNodePath } from "./nodes";

describe("nodes api", () => {
  it("resolves node paths with URLSearchParams encoding", async () => {
    const client = { get: vi.fn().mockResolvedValue({ id: "node-1" }) } as unknown as ApiClient;

    await resolveNodePath(client, "space-1", "/Policies/Access Control #1.md");

    expect(client.get).toHaveBeenCalledWith("/api/v1/spaces/space-1/paths/resolve?path=%2FPolicies%2FAccess+Control+%231.md");
  });

  it("requests a bounded first children page for multiple parents", async () => {
    const client = { post: vi.fn().mockResolvedValue({ results: [] }) } as unknown as ApiClient;

    await batchListChildren(client, "space-1", ["root-1", "folder-1"]);

    expect(client.post).toHaveBeenCalledWith(
      "/api/v1/spaces/space-1/nodes:batchListChildren",
      {
        parent_ids: ["root-1", "folder-1"],
        limit: 100
      }
    );
  });

  it("opts into compact children and restores the route Space id", async () => {
    const client = {
      get: vi.fn().mockResolvedValue({
        parent: { id: "root-1", path: "/" },
        children: [{ id: "node-1", name: "node-1", kind: "text" }],
        page: { limit: 100, returned: 1, has_more: false, next_cursor: null }
      })
    } as unknown as ApiClient;

    const response = await listChildren(client, "space-1", "root-1");

    expect(client.get).toHaveBeenCalledWith(
      "/api/v1/spaces/space-1/nodes/root-1/children?limit=100&view=summary"
    );
    expect(response.children[0]?.space_id).toBe("space-1");
  });

  it("continues compact Recent pages with the opaque cursor", async () => {
    const client = {
      get: vi.fn().mockResolvedValue({
        nodes: [{ id: "node-51", name: "node-51", kind: "text" }],
        page: { limit: 50, returned: 1, has_more: false, next_cursor: null }
      })
    } as unknown as ApiClient;

    const response = await listNodes(client, "space-1", {
      sort: "updated_at_desc",
      cursor: "cursor-50"
    });

    expect(client.get).toHaveBeenCalledWith(
      "/api/v1/spaces/space-1/nodes?limit=50&sort=updated_at_desc&view=summary&cursor=cursor-50"
    );
    expect(response.nodes[0]?.space_id).toBe("space-1");
  });
});
