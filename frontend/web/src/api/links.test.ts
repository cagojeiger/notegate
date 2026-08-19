import { describe, expect, it } from "vitest";

import { createMockApiClient } from "../test/apiClient";
import {
  getSpaceLinkIndexStatus,
  getNodeLinkStatus,
  listNodeLinks,
  requestNodeLinkSync,
  requestSpaceLinkReindex
} from "./links";

describe("links api", () => {
  it("loads one node projection status", async () => {
    const client = createMockApiClient();
    client.get.mockResolvedValue({ status: "idle" });

    await getNodeLinkStatus(client, "space-1", "node-1");

    expect(client.get).toHaveBeenCalledWith(
      "/api/v1/spaces/space-1/nodes/node-1/links"
    );
  });

  it("continues a link direction with an opaque cursor", async () => {
    const client = createMockApiClient();
    client.get.mockResolvedValue({ links: [] });

    await listNodeLinks(client, "space-1", "node-1", "incoming", "cursor-50");

    expect(client.get).toHaveBeenCalledWith(
      "/api/v1/spaces/space-1/nodes/node-1/links/incoming?limit=50&cursor=cursor-50"
    );
  });

  it("requests node and Space link rebuilds", async () => {
    const client = createMockApiClient();
    client.post.mockResolvedValue({ status: "accepted", job_id: null });

    await requestNodeLinkSync(client, "space-1", "node-1");
    await requestSpaceLinkReindex(client, "space-1");

    expect(client.post).toHaveBeenNthCalledWith(
      1,
      "/api/v1/spaces/space-1/nodes/node-1/links/sync"
    );
    expect(client.post).toHaveBeenNthCalledWith(
      2,
      "/api/v1/spaces/space-1/link-index/reindex"
    );
  });

  it("loads the Space link index status", async () => {
    const client = createMockApiClient();
    client.get.mockResolvedValue({ pending: true });

    await getSpaceLinkIndexStatus(client, "space-1");

    expect(client.get).toHaveBeenCalledWith(
      "/api/v1/spaces/space-1/link-index/status"
    );
  });
});
