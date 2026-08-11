import { describe, expect, it } from "vitest";

import { createMockApiClient } from "../test/apiClient";
import { getCurrentUserUsage, requestSpaceUsageCheck } from "./usage";

describe("usage api", () => {
  it("loads the current user's usage", async () => {
    const client = createMockApiClient();
    client.get.mockResolvedValue({ tier: "tier0", spaces: [] });

    await getCurrentUserUsage(client);

    expect(client.get).toHaveBeenCalledWith("/api/v1/me/usage");
  });

  it("requests a usage check for one space", async () => {
    const client = createMockApiClient();
    client.post.mockResolvedValue({ status: "queued", job_id: "job-1" });

    await requestSpaceUsageCheck(client, "space-1");

    expect(client.post).toHaveBeenCalledWith("/api/v1/spaces/space-1/usage/reconcile");
  });
});
