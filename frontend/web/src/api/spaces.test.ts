import { describe, expect, it } from "vitest";

import { createMockApiClient } from "../test/apiClient";
import { reorderSpaces } from "./spaces";

describe("reorderSpaces", () => {
  it("sends all order changes in one request", async () => {
    const client = createMockApiClient();
    client.post.mockResolvedValue(undefined);

    await reorderSpaces(client, [
      { spaceId: "space-2", sort_order: 1000 },
      { spaceId: "space-1", sort_order: 2000 }
    ]);

    expect(client.post).toHaveBeenCalledOnce();
    expect(client.post).toHaveBeenCalledWith("/api/v1/spaces:reorder", {
      updates: [
        { space_id: "space-2", sort_order: 1000 },
        { space_id: "space-1", sort_order: 2000 }
      ]
    });
  });
});
