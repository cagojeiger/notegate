import { describe, expect, it } from "vitest";

import { createMockApiClient } from "../test/apiClient";
import { ApiError } from "./errors";
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
    client.get.mockResolvedValue({
      status: "pending",
      space_pending: false,
      projected_at: null,
      failure_code: null,
      failed_at: null
    });

    const status = await getNodeLinkStatus(client, "space-1", "node-1");

    expect(client.get).toHaveBeenCalledWith(
      "/api/v1/spaces/space-1/nodes/node-1/links"
    );
    expect(status.availability).toEqual({
      can_trigger: false,
      reason: "pending",
      retry_at: null
    });
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
    client.post.mockResolvedValue({
      result: "accepted",
      availability: { can_trigger: false, reason: "pending", retry_at: null }
    });

    const nodeResponse = await requestNodeLinkSync(client, "space-1", "node-1");
    const spaceResponse = await requestSpaceLinkReindex(client, "space-1");

    expect(client.post).toHaveBeenNthCalledWith(
      1,
      "/api/v1/spaces/space-1/nodes/node-1/actions/reindex-links"
    );
    expect(client.post).toHaveBeenNthCalledWith(
      2,
      "/api/v1/spaces/space-1/actions/reindex-links"
    );
    expect(nodeResponse.result).toBe("accepted");
    expect(spaceResponse.availability.reason).toBe("pending");
  });

  it("falls back to the previous link command paths", async () => {
    const client = createMockApiClient();
    client.post
      .mockRejectedValueOnce(new ApiError("api route not found", 404, "not_found"))
      .mockResolvedValueOnce({ status: "accepted" })
      .mockRejectedValueOnce(new ApiError("api route not found", 404, "not_found"))
      .mockResolvedValueOnce({ status: "accepted" });

    await requestNodeLinkSync(client, "space-1", "node-1");
    await requestSpaceLinkReindex(client, "space-1");

    expect(client.post).toHaveBeenNthCalledWith(
      2,
      "/api/v1/spaces/space-1/nodes/node-1/links/sync"
    );
    expect(client.post).toHaveBeenNthCalledWith(
      4,
      "/api/v1/spaces/space-1/link-index/reindex"
    );
  });

  it("loads the Space link index status", async () => {
    const client = createMockApiClient();
    client.get.mockResolvedValue({
      status: "pending",
      availability: { can_trigger: false, reason: "pending", retry_at: null }
    });

    await getSpaceLinkIndexStatus(client, "space-1");

    expect(client.get).toHaveBeenCalledWith(
      "/api/v1/spaces/space-1/link-index"
    );
  });

  it("allows reindex against a backend without the Space status endpoint", async () => {
    const client = createMockApiClient();
    client.get.mockRejectedValue(new ApiError("api route not found", 404, "not_found"));

    await expect(getSpaceLinkIndexStatus(client, "space-1")).resolves.toEqual({
      status: "unknown",
      availability: { can_trigger: true, reason: null, retry_at: null }
    });
  });

  it("preserves resource not found errors", async () => {
    const client = createMockApiClient();
    const error = new ApiError("space not found", 404, "not_found");
    client.get.mockRejectedValue(error);

    await expect(getSpaceLinkIndexStatus(client, "space-1")).rejects.toBe(error);
  });

  it("does not retry a missing resource through the previous command path", async () => {
    const client = createMockApiClient();
    const error = new ApiError("space not found", 404, "not_found");
    client.post.mockRejectedValue(error);

    await expect(requestSpaceLinkReindex(client, "space-1")).rejects.toBe(error);
    expect(client.post).toHaveBeenCalledTimes(1);
  });
});
