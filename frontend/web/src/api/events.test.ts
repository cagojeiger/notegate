import { describe, expect, it, vi } from "vitest";

import type { ApiClient } from "./client";
import { drainFileChanges, listAuditEvents, listFileChangeEvents } from "./events";

describe("events api", () => {
  it("lists audit events with pagination", async () => {
    const client = { get: vi.fn().mockResolvedValue({ events: [] }) } as unknown as ApiClient;

    await listAuditEvents(client, "cursor-1");

    expect(client.get).toHaveBeenCalledWith("/api/v1/me/audit-events?limit=50&cursor=cursor-1");
  });

  it("lists file change events with node filtering", async () => {
    const client = { get: vi.fn().mockResolvedValue({ events: [] }) } as unknown as ApiClient;

    await listFileChangeEvents(client, "space-1", { nodeId: "node-1", cursor: "cursor-2" });

    expect(client.get).toHaveBeenCalledWith("/api/v1/spaces/space-1/file-change-events?limit=50&node_id=node-1&cursor=cursor-2");
  });

  it("allows event history to request one item", async () => {
    const client = { get: vi.fn().mockResolvedValue({ events: [] }) } as unknown as ApiClient;

    await listFileChangeEvents(client, "space-1", { limit: 1 });

    expect(client.get).toHaveBeenCalledWith("/api/v1/spaces/space-1/file-change-events?limit=1");
  });

  it("drains forward sync pages without skipping intermediate changes", async () => {
    const client = {
      get: vi.fn()
        .mockResolvedValueOnce(syncResponse([change(11), change(12)], 12, true))
        .mockResolvedValueOnce(syncResponse([change(13)], 13, false))
    } as unknown as ApiClient;

    const result = await drainFileChanges(client, "space-1", 10);

    expect(client.get).toHaveBeenNthCalledWith(
      1,
      "/api/v1/spaces/space-1/file-change-sync?limit=100&after_id=10"
    );
    expect(client.get).toHaveBeenNthCalledWith(
      2,
      "/api/v1/spaces/space-1/file-change-sync?limit=100&after_id=12"
    );
    expect(result.changes.map((item) => item.id)).toEqual([11, 12, 13]);
    expect(result.next_after_id).toBe(13);
  });

  it("drops partial pages when the server requires a resync", async () => {
    const client = {
      get: vi.fn()
        .mockResolvedValueOnce(syncResponse([change(11)], 11, true))
        .mockResolvedValueOnce({
          ...syncResponse([], 20, false),
          resync_required: true
        })
    } as unknown as ApiClient;

    const result = await drainFileChanges(client, "space-1", 10);

    expect(result).toMatchObject({
      changes: [],
      next_after_id: 20,
      resync_required: true
    });
  });

  it("rejects a paginated response that does not advance its token", async () => {
    const client = {
      get: vi.fn()
        .mockResolvedValueOnce(syncResponse([change(11)], 11, true))
        .mockResolvedValueOnce(syncResponse([change(11)], 11, true))
    } as unknown as ApiClient;

    await expect(drainFileChanges(client, "space-1", 10))
      .rejects.toThrow("file change sync token did not advance");
  });
});

function syncResponse(changes: ReturnType<typeof change>[], nextAfterId: number, hasMore: boolean) {
  return {
    changes,
    next_after_id: nextAfterId,
    has_more: hasMore,
    resync_required: false
  };
}

function change(id: number) {
  return {
    id,
    node_id: `node-${id}`,
    op_type: "text.write",
    item_kind: "text",
    affected_parent_ids: ["parent-1"],
    parent_scope_known: true,
    path_changed: false,
    subtree_changed: false
  };
}
