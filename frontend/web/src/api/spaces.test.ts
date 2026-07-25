import { describe, expect, it, vi } from "vitest";

import type { ApiClient } from "./client";
import { reorderSpaces } from "./spaces";

describe("reorderSpaces", () => {
  it("sends all order changes in one request", async () => {
    const post = vi.fn().mockResolvedValue(undefined);
    const client = { post } as unknown as ApiClient;

    await reorderSpaces(client, [
      { spaceId: "space-2", sort_order: 1000 },
      { spaceId: "space-1", sort_order: 2000 }
    ]);

    expect(post).toHaveBeenCalledOnce();
    expect(post).toHaveBeenCalledWith("/api/v1/spaces:reorder", {
      updates: [
        { space_id: "space-2", sort_order: 1000 },
        { space_id: "space-1", sort_order: 2000 }
      ]
    });
  });
});
