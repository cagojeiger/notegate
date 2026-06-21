import { describe, expect, it, vi } from "vitest";

import type { ApiClient } from "./client";
import { resolveNodePath } from "./nodes";

describe("nodes api", () => {
  it("resolves node paths with URLSearchParams encoding", async () => {
    const client = { get: vi.fn().mockResolvedValue({ id: "node-1" }) } as unknown as ApiClient;

    await resolveNodePath(client, "space-1", "/Policies/Access Control #1.md");

    expect(client.get).toHaveBeenCalledWith("/api/v1/spaces/space-1/paths/resolve?path=%2FPolicies%2FAccess+Control+%231.md");
  });
});
