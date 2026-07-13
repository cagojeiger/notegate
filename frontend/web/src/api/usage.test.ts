import { describe, expect, it, vi } from "vitest";

import type { ApiClient } from "./client";
import { getCurrentUserUsage, requestSpaceUsageCheck } from "./usage";

describe("usage api", () => {
  it("loads the current user's usage", async () => {
    const get = vi.fn().mockResolvedValue({ tier: "tier0", spaces: [] });
    const client = { get } as unknown as ApiClient;

    await getCurrentUserUsage(client);

    expect(get).toHaveBeenCalledWith("/api/v1/me/usage");
  });

  it("requests a usage check for one space", async () => {
    const post = vi.fn().mockResolvedValue({ status: "queued" });
    const client = { post } as unknown as ApiClient;

    await requestSpaceUsageCheck(client, "space-1");

    expect(post).toHaveBeenCalledWith("/api/v1/spaces/space-1/usage/reconcile");
  });
});
