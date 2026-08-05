import { describe, expect, it } from "vitest";

import { createMockApiClient } from "../test/apiClient";
import {
  batchListChildren,
  deleteNode,
  listChildren,
  listNodes,
  moveNode,
  resolveNodePath,
  updateNode,
  updateNodeSearchPolicy,
  updateNodeWriteLock
} from "./nodes";

describe("nodes api", () => {
  it("resolves node paths with URLSearchParams encoding", async () => {
    const client = createMockApiClient();
    client.get.mockResolvedValue({ id: "node-1" });

    await resolveNodePath(client, "space-1", "/Policies/Access Control #1.md");

    expect(client.get).toHaveBeenCalledWith("/api/v1/spaces/space-1/paths/resolve?path=%2FPolicies%2FAccess+Control+%231.md");
  });

  it("requests a bounded first children page for multiple parents", async () => {
    const client = createMockApiClient();
    client.post.mockResolvedValue({ results: [] });

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
    const client = createMockApiClient();
    client.get.mockResolvedValue({
      parent: { id: "root-1", path: "/" },
      children: [
        { id: "node-1", name: "node-1", kind: "text" },
        { id: "node-2", name: "node-2", kind: "text", effective_write_locked: true }
      ],
      page: { limit: 100, returned: 2, has_more: false, next_cursor: null }
    });

    const response = await listChildren(client, "space-1", "root-1");

    expect(client.get).toHaveBeenCalledWith(
      "/api/v1/spaces/space-1/nodes/root-1/children?limit=100&view=summary"
    );
    expect(response.children[0]?.space_id).toBe("space-1");
    expect(response.children[0]?.effective_write_locked).toBe(false);
    expect(response.children[1]?.effective_write_locked).toBe(true);
  });

  it("continues compact Recent pages with the opaque cursor", async () => {
    const client = createMockApiClient();
    client.get.mockResolvedValue({
      nodes: [{ id: "node-51", name: "node-51", kind: "text" }],
      page: { limit: 50, returned: 1, has_more: false, next_cursor: null }
    });

    const response = await listNodes(client, "space-1", {
      sort: "updated_at_desc",
      cursor: "cursor-50"
    });

    expect(client.get).toHaveBeenCalledWith(
      "/api/v1/spaces/space-1/nodes?limit=50&sort=updated_at_desc&view=summary&cursor=cursor-50"
    );
    expect(response.nodes[0]?.space_id).toBe("space-1");
  });

  it("sends the latest revision with every node mutation", async () => {
    const client = createMockApiClient();
    client.patch.mockResolvedValue({});
    client.put.mockResolvedValue({});
    client.post.mockResolvedValue({});
    client.delete.mockResolvedValue(undefined);

    await updateNode(client, "space-1", "node-1", {
      name: "renamed.md",
      expected_revision: 7
    });
    await updateNodeSearchPolicy(client, "space-1", "node-1", false, 7);
    await updateNodeWriteLock(client, "space-1", "node-1", true, 7);
    await moveNode(client, "space-1", "node-1", {
      new_parent_id: "folder-2",
      expected_revision: 7
    });
    await deleteNode(client, "space-1", "node-1", true, 7);

    expect(client.patch).toHaveBeenCalledWith(
      "/api/v1/spaces/space-1/nodes/node-1",
      { name: "renamed.md", expected_revision: 7 }
    );
    expect(client.put).toHaveBeenNthCalledWith(
      1,
      "/api/v1/spaces/space-1/nodes/node-1/search-policy",
      { enabled: false, expected_revision: 7 }
    );
    expect(client.put).toHaveBeenNthCalledWith(
      2,
      "/api/v1/spaces/space-1/nodes/node-1/write-lock",
      { enabled: true, expected_revision: 7 }
    );
    expect(client.post).toHaveBeenCalledWith(
      "/api/v1/spaces/space-1/nodes/node-1/move",
      { new_parent_id: "folder-2", expected_revision: 7 }
    );
    expect(client.delete).toHaveBeenCalledWith(
      "/api/v1/spaces/space-1/nodes/node-1?recursive=true&expected_revision=7"
    );
  });
});
