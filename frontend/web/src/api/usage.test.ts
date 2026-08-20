import { describe, expect, it } from "vitest";

import { createMockApiClient } from "../test/apiClient";
import { ApiError } from "./errors";
import { getCurrentUserUsage, requestSpaceUsageCheck } from "./usage";

describe("usage api", () => {
  it("loads the current user's usage", async () => {
    const client = createMockApiClient();
    client.get.mockResolvedValue({ tier: "tier0", spaces: [] });

    await getCurrentUserUsage(client);

    expect(client.get).toHaveBeenCalledWith("/api/v1/me/usage");
  });

  it("normalizes the previous usage response", async () => {
    const client = createMockApiClient();
    client.get.mockResolvedValue({
      tier: "tier0",
      spaces: [{
        id: "space-1",
        name: "Personal",
        items: { used: 1, limit: 10 },
        text_bytes: { used: 2, limit: 20 },
        file_bytes: { used: 3, limit: 30 },
        reconciliation_pending: true
      }]
    });

    const usage = await getCurrentUserUsage(client);

    expect(usage.spaces[0]?.reconciliation).toEqual({
      status: "pending",
      availability: { can_trigger: false, reason: "pending", retry_at: null }
    });
  });

  it("requests a usage check for one space", async () => {
    const client = createMockApiClient();
    client.post.mockResolvedValue({
      result: "accepted",
      availability: { can_trigger: false, reason: "pending", retry_at: null }
    });

    await requestSpaceUsageCheck(client, "space-1");

    expect(client.post).toHaveBeenCalledWith("/api/v1/spaces/space-1/actions/reconcile-usage");
  });

  it("falls back to the previous usage command and normalizes its response", async () => {
    const client = createMockApiClient();
    client.post
      .mockRejectedValueOnce(new ApiError("api route not found", 404, "not_found"))
      .mockResolvedValueOnce({ status: "queued", job_id: "job-1" });

    await expect(requestSpaceUsageCheck(client, "space-1")).resolves.toEqual({
      result: "accepted",
      availability: { can_trigger: false, reason: "pending", retry_at: null }
    });
    expect(client.post).toHaveBeenNthCalledWith(
      2,
      "/api/v1/spaces/space-1/usage/reconcile"
    );
  });
});
