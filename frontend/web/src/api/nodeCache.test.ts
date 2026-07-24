import { QueryClient, type InfiniteData } from "@tanstack/react-query";
import { describe, expect, it } from "vitest";

import { updateNodeCaches } from "./nodeCache";
import { queryKeys } from "./queryKeys";
import type { ChildrenResponse, RestNode, RestNodeListResponse } from "./types";

describe("updateNodeCaches", () => {
  it("updates every cached appearance without changing unrelated pages", () => {
    const queryClient = new QueryClient();
    const target = node("file-1");
    const unrelated = node("file-2");
    const recent: RestNodeListResponse = {
      nodes: [target, unrelated],
      page: page()
    };
    const children: InfiniteData<ChildrenResponse> = {
      pages: [
        { parent: { id: "root-1", path: "/" }, children: [target], page: page() },
        { parent: { id: "root-1", path: "/" }, children: [unrelated], page: page() }
      ],
      pageParams: [null, "next"]
    };
    queryClient.setQueryData(queryKeys.recent("space-1"), recent);
    queryClient.setQueryData(queryKeys.children("space-1", "root-1"), children);

    updateNodeCaches(queryClient, target, (current) => ({
      ...current,
      detected_media_type: "image/png",
      preview_available: true
    }));

    expect(queryClient.getQueryData<RestNode>(queryKeys.node("space-1", "file-1")))
      .toMatchObject({ detected_media_type: "image/png", preview_available: true });
    const updatedRecent = queryClient.getQueryData<RestNodeListResponse>(queryKeys.recent("space-1"));
    expect(updatedRecent?.nodes[0]).toMatchObject({ preview_available: true });
    expect(updatedRecent?.nodes[1]).toBe(unrelated);
    const updatedChildren = queryClient.getQueryData<InfiniteData<ChildrenResponse>>(
      queryKeys.children("space-1", "root-1")
    );
    expect(updatedChildren?.pages[0]?.children[0]).toMatchObject({ preview_available: true });
    expect(updatedChildren?.pages[1]).toBe(children.pages[1]);
  });

  it("does not create collection entries that were not already cached", () => {
    const queryClient = new QueryClient();
    const target = node("file-1");

    updateNodeCaches(queryClient, target, (current) => ({ ...current, preview_available: true }));

    expect(queryClient.getQueryData(queryKeys.node("space-1", "file-1"))).toMatchObject({
      preview_available: true
    });
    expect(queryClient.getQueryData(queryKeys.recent("space-1"))).toBeUndefined();
  });

  it("does not treat folder statistics as paginated children data", () => {
    const queryClient = new QueryClient();
    const target = node("file-1");
    const statKey = [...queryKeys.children("space-1", "root-1"), "stat"] as const;
    const stat = { parent: { id: "root-1", path: "/" }, children: [target], page: page() };
    queryClient.setQueryData(statKey, stat);

    updateNodeCaches(queryClient, target, (current) => ({ ...current, preview_available: true }));

    expect(queryClient.getQueryData(statKey)).toBe(stat);
  });
});

function node(id: string): RestNode {
  return {
    id,
    space_id: "space-1",
    parent_id: "root-1",
    name: `${id}.png`,
    kind: "file",
    path: `/${id}.png`,
    sort_order: 0,
    metadata: {},
    has_children: false,
    created_by: { id: "user-1", kind: "user", display_name: "User" },
    updated_by: { id: "user-1", kind: "user", display_name: "User" },
    created_at: "2026-06-13T00:00:00Z",
    updated_at: "2026-06-13T00:00:00Z"
  };
}

function page() {
  return {
    limit: 100,
    returned: 2,
    has_more: false,
    next_cursor: null
  };
}
