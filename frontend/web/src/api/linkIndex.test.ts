import { describe, expect, it } from "vitest";

import { createMockApiClient } from "../test/apiClient";
import { listNodeLinkReferences } from "./linkIndex";

describe("link index api", () => {
  it("pages incoming and outgoing references independently", async () => {
    const client = createMockApiClient();
    client.get.mockResolvedValue({ links: [] });

    await listNodeLinkReferences(client, "space-1", "node-1", "outgoing", "cursor-1");
    await listNodeLinkReferences(client, "space-1", "node-1", "incoming");

    expect(client.get).toHaveBeenNthCalledWith(
      1,
      "/api/v1/spaces/space-1/nodes/node-1/links/outgoing?limit=50&cursor=cursor-1"
    );
    expect(client.get).toHaveBeenNthCalledWith(
      2,
      "/api/v1/spaces/space-1/nodes/node-1/links/incoming?limit=50"
    );
  });
});
