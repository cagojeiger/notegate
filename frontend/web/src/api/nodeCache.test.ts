import { QueryClient, type InfiniteData } from "@tanstack/react-query";
import { describe, expect, it } from "vitest";

import { makeRestNode } from "../test/fixtures";
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
    const recentPages: InfiniteData<RestNodeListResponse> = {
      pages: [recent],
      pageParams: [null]
    };
    queryClient.setQueryData(queryKeys.recent("space-1"), recentPages);
    queryClient.setQueryData(queryKeys.children("space-1", "root-1"), children);

    updateNodeCaches(queryClient, target, (current) => ({
      ...current,
      detected_media_type: "image/png",
      preview_available: true,
      file_preview_kind: "image"
    }));

    expect(queryClient.getQueryData<RestNode>(queryKeys.node("space-1", "file-1")))
      .toMatchObject({
        detected_media_type: "image/png",
        preview_available: true,
        file_preview_kind: "image"
      });
    const updatedRecent = queryClient.getQueryData<InfiniteData<RestNodeListResponse>>(queryKeys.recent("space-1"));
    expect(updatedRecent?.pages[0]?.nodes[0]).toMatchObject({
      preview_available: true,
      file_preview_kind: "image"
    });
    expect(updatedRecent?.pages[0]?.nodes[1]).toBe(unrelated);
    const updatedChildren = queryClient.getQueryData<InfiniteData<ChildrenResponse>>(
      queryKeys.children("space-1", "root-1")
    );
    expect(updatedChildren?.pages[0]?.children[0]).toMatchObject({
      preview_available: true,
      file_preview_kind: "image"
    });
    expect(updatedChildren?.pages[1]).toBe(children.pages[1]);
    expect(queryClient.getQueryData(queryKeys.childrenRevision("space-1"))).toBe(1);
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

  it("preserves effective write-lock state in collection summaries", () => {
    const queryClient = new QueryClient();
    const target = node("file-1");
    queryClient.setQueryData(queryKeys.recent("space-1"), {
      pages: [{ nodes: [target], page: page() }],
      pageParams: [null]
    });

    updateNodeCaches(
      queryClient,
      { ...target, write_locked: true, effective_write_locked: true },
      (current) => ({ ...current, effective_write_locked: true })
    );

    const recent = queryClient.getQueryData<InfiniteData<RestNodeListResponse>>(
      queryKeys.recent("space-1")
    );
    expect(recent?.pages[0]?.nodes[0]?.effective_write_locked).toBe(true);
  });
});

function node(id: string): RestNode {
  return makeRestNode({
    id,
    name: `${id}.png`,
    kind: "file",
    path: `/${id}.png`
  });
}

function page() {
  return {
    limit: 100,
    returned: 2,
    has_more: false,
    next_cursor: null
  };
}
