import { describe, expect, it, vi } from "vitest";

import type { ApiClient } from "./client";
import { listAuditEvents, listFileChangeEvents } from "./events";

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
});
