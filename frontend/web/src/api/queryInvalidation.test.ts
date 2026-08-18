import { InfiniteQueryObserver, QueryObserver } from "@tanstack/react-query";
import { describe, expect, it, vi } from "vitest";

import { createTestQueryClient } from "../test/queryClient";
import {
  applyExternalFileChanges,
  invalidateAgentsList,
  invalidateFolderSubtree,
  invalidateNodeLists,
  invalidateRecentNodes,
  invalidateSpaceLinks,
  invalidateSpaceResources,
  invalidateSpacesList,
  invalidateWriteLockState,
  removeMarkdownImageQueries,
  removeMarkdownImagePreviewQuery,
  removeDeletedNodeQueries,
  removeDeletedSpaceQueries
} from "./queryInvalidation";
import { queryKeys } from "./queryKeys";

describe("query invalidation", () => {
  it("refetches the spaces list without refetching active space descendants", async () => {
    const queryClient = createTestQueryClient();
    const spacesQuery = vi.fn().mockResolvedValue({ spaces: [] });
    const nodeQuery = vi.fn().mockResolvedValue({ id: "node-1" });
    const textQuery = vi.fn().mockResolvedValue({ content: "text" });
    const spacesObserver = new QueryObserver(queryClient, {
      queryKey: queryKeys.spaces,
      queryFn: spacesQuery,
      staleTime: Number.POSITIVE_INFINITY
    });
    const nodeObserver = new QueryObserver(queryClient, {
      queryKey: queryKeys.node("space-1", "node-1"),
      queryFn: nodeQuery,
      staleTime: Number.POSITIVE_INFINITY
    });
    const textObserver = new QueryObserver(queryClient, {
      queryKey: queryKeys.text("space-1", "node-1"),
      queryFn: textQuery,
      staleTime: Number.POSITIVE_INFINITY
    });
    queryClient.setQueryData(queryKeys.spaces, { spaces: [] });
    queryClient.setQueryData(queryKeys.node("space-1", "node-1"), { id: "node-1" });
    queryClient.setQueryData(queryKeys.text("space-1", "node-1"), { content: "text" });
    const unsubscribe = [
      spacesObserver.subscribe(() => undefined),
      nodeObserver.subscribe(() => undefined),
      textObserver.subscribe(() => undefined)
    ];

    invalidateSpacesList(queryClient);

    await vi.waitFor(() => expect(spacesQuery).toHaveBeenCalledOnce());
    expect(nodeQuery).not.toHaveBeenCalled();
    expect(textQuery).not.toHaveBeenCalled();
    unsubscribe.forEach((stopObserving) => stopObserving());
  });

  it("refetches the agents list without refetching active agent keys", async () => {
    const queryClient = createTestQueryClient();
    const agentsQuery = vi.fn().mockResolvedValue({ agents: [] });
    const agentKeysQuery = vi.fn().mockResolvedValue({ keys: [] });
    const agentsObserver = new QueryObserver(queryClient, {
      queryKey: queryKeys.agents,
      queryFn: agentsQuery,
      staleTime: Number.POSITIVE_INFINITY
    });
    const agentKeysObserver = new QueryObserver(queryClient, {
      queryKey: queryKeys.agentKeys("agent-1"),
      queryFn: agentKeysQuery,
      staleTime: Number.POSITIVE_INFINITY
    });
    queryClient.setQueryData(queryKeys.agents, { agents: [] });
    queryClient.setQueryData(queryKeys.agentKeys("agent-1"), { keys: [] });
    const unsubscribe = [
      agentsObserver.subscribe(() => undefined),
      agentKeysObserver.subscribe(() => undefined)
    ];

    invalidateAgentsList(queryClient);

    await vi.waitFor(() => expect(agentsQuery).toHaveBeenCalledOnce());
    expect(agentKeysQuery).not.toHaveBeenCalled();
    unsubscribe.forEach((stopObserving) => stopObserving());
  });

  it("invalidates only Recent and the affected parent folders for a node change", () => {
    const queryClient = createTestQueryClient();
    const resetQueries = vi.spyOn(queryClient, "resetQueries");

    invalidateNodeLists(queryClient, "space-1", ["parent-1", "parent-2", "parent-1", null]);

    expect(resetQueries).toHaveBeenNthCalledWith(1, {
      queryKey: queryKeys.recent("space-1"),
      exact: true
    });
    expect(resetQueries).toHaveBeenNthCalledWith(2, {
      queryKey: queryKeys.children("space-1", "parent-1")
    });
    expect(resetQueries).toHaveBeenNthCalledWith(3, {
      queryKey: queryKeys.children("space-1", "parent-2")
    });
    expect(resetQueries).toHaveBeenCalledTimes(3);
    expect(
      queryClient.getQueryData(queryKeys.childrenRevision("space-1"))
    ).toBe(1);
  });

  it("resets a multi-page Recent cache and refetches only its first page", async () => {
    const queryClient = createTestQueryClient();
    const key = queryKeys.recent("space-1");
    const page = (id: string, hasMore: boolean, nextCursor: string | null) => ({
      nodes: [{ id }],
      page: { limit: 50, returned: 1, has_more: hasMore, next_cursor: nextCursor }
    });
    queryClient.setQueryData(key, {
      pages: [
        page("node-1", true, "cursor-1"),
        page("node-2", true, "cursor-2"),
        page("node-3", false, null)
      ],
      pageParams: [null, "cursor-1", "cursor-2"]
    });
    const queryFn = vi.fn().mockResolvedValue(page("fresh-1", true, "fresh-cursor"));
    const observer = new InfiniteQueryObserver(queryClient, {
      queryKey: key,
      queryFn,
      initialPageParam: null as string | null,
      getNextPageParam: (lastPage) =>
        lastPage.page.has_more ? lastPage.page.next_cursor : undefined,
      staleTime: Number.POSITIVE_INFINITY
    });
    const unsubscribe = observer.subscribe(() => undefined);

    invalidateRecentNodes(queryClient, "space-1");

    await vi.waitFor(() => expect(queryFn).toHaveBeenCalledOnce());
    await vi.waitFor(() =>
      expect(observer.getCurrentResult().data?.pages).toHaveLength(1)
    );
    expect(queryFn).toHaveBeenCalledWith(
      expect.objectContaining({ pageParam: null })
    );
    unsubscribe();
  });

  it("resets an affected multi-page folder and refetches only its first page", async () => {
    const queryClient = createTestQueryClient();
    const key = queryKeys.children("space-1", "folder-1");
    const page = (id: string, hasMore: boolean, nextCursor: string | null) => ({
      parent: { id: "folder-1", path: "/folder" },
      children: [{ id }],
      page: { limit: 100, returned: 1, has_more: hasMore, next_cursor: nextCursor }
    });
    queryClient.setQueryData(key, {
      pages: [
        page("node-1", true, "cursor-1"),
        page("node-2", true, "cursor-2"),
        page("node-3", false, null)
      ],
      pageParams: [null, "cursor-1", "cursor-2"]
    });
    const queryFn = vi.fn().mockResolvedValue(page("fresh-1", true, "fresh-cursor"));
    const observer = new InfiniteQueryObserver(queryClient, {
      queryKey: key,
      queryFn,
      initialPageParam: null as string | null,
      getNextPageParam: (lastPage) =>
        lastPage.page.has_more ? lastPage.page.next_cursor : undefined,
      staleTime: Number.POSITIVE_INFINITY
    });
    const unsubscribe = observer.subscribe(() => undefined);

    invalidateNodeLists(queryClient, "space-1", ["folder-1"]);

    await vi.waitFor(() => expect(queryFn).toHaveBeenCalledOnce());
    await vi.waitFor(() =>
      expect(observer.getCurrentResult().data?.pages).toHaveLength(1)
    );
    expect(queryFn).toHaveBeenCalledWith(
      expect.objectContaining({ pageParam: null })
    );
    unsubscribe();
  });

  it("can refresh a space subtree without invalidating the spaces list", () => {
    const queryClient = createTestQueryClient();
    const invalidateQueries = vi.spyOn(queryClient, "invalidateQueries");

    invalidateSpaceResources(queryClient, "space-1");

    expect(invalidateQueries).toHaveBeenCalledOnce();
    expect(invalidateQueries).toHaveBeenCalledWith({ queryKey: ["spaces", "space-1"] });
  });

  it("refreshes link status without discarding cached link pages", () => {
    const queryClient = createTestQueryClient();
    const statusKey = queryKeys.nodeLinkStatus("space-1", "node-1");
    const listKey = queryKeys.nodeLinkList("space-1", "node-1", "incoming");
    const cachedPages = { pages: [{ links: [{ node_id: "source-1" }] }], pageParams: [null] };
    queryClient.setQueryData(statusKey, { status: "idle" });
    queryClient.setQueryData(listKey, cachedPages);

    invalidateSpaceLinks(queryClient, "space-1");

    expect(queryClient.getQueryData(statusKey)).toBeUndefined();
    expect(queryClient.getQueryData(listKey)).toEqual(cachedPages);
    expect(queryClient.getQueryState(listKey)?.isInvalidated).toBe(true);
  });

  it("invalidates descendant-bearing cache families after a folder path change", () => {
    const queryClient = createTestQueryClient();
    const invalidateQueries = vi.spyOn(queryClient, "invalidateQueries");
    const resetQueries = vi.spyOn(queryClient, "resetQueries");
    const pathKey = queryKeys.markdownImagePreview("space-1", "/folder/image.png");
    const canonicalNodeKey = queryKeys.node("space-1", "child-1");
    queryClient.setQueryData(pathKey, { id: "image-1" });
    queryClient.setQueryData(canonicalNodeKey, { id: "child-1" });

    invalidateFolderSubtree(queryClient, "space-1");

    expect(resetQueries).toHaveBeenNthCalledWith(1, {
      queryKey: queryKeys.recent("space-1"),
      exact: true
    });
    expect(resetQueries).toHaveBeenNthCalledWith(2, {
      queryKey: queryKeys.childrenFamily("space-1")
    });
    expect(invalidateQueries).toHaveBeenNthCalledWith(1, {
      queryKey: queryKeys.nodes("space-1")
    });
    expect(resetQueries).toHaveBeenCalledTimes(2);
    expect(invalidateQueries).toHaveBeenCalledOnce();
    expect(queryClient.getQueryData(pathKey)).toBeUndefined();
    expect(queryClient.getQueryState(canonicalNodeKey)?.isInvalidated).toBe(
      true
    );
    expect(
      queryClient.getQueryData(queryKeys.childrenRevision("space-1"))
    ).toBe(1);
  });

  it("coalesces multiple external changes into one list refresh per affected parent", async () => {
    const queryClient = createTestQueryClient();
    const invalidateQueries = vi.spyOn(queryClient, "invalidateQueries");
    const resetQueries = vi.spyOn(queryClient, "resetQueries");

    await applyExternalFileChanges(queryClient, "space-1", [
      delta(11, "text-1", ["parent-1"]),
      delta(12, "text-2", ["parent-1"])
    ]);

    expect(invalidateQueries).toHaveBeenCalledWith({
      queryKey: queryKeys.node("space-1", "text-1"),
      exact: true
    });
    expect(invalidateQueries).toHaveBeenCalledWith({
      queryKey: queryKeys.node("space-1", "text-2"),
      exact: true
    });
    expect(resetQueries).toHaveBeenCalledWith({
      queryKey: queryKeys.linkStatuses("space-1")
    });
    expect(invalidateQueries).toHaveBeenCalledWith({
      queryKey: queryKeys.linkLists("space-1"),
      refetchType: "none"
    });
    expect(resetQueries).toHaveBeenCalledWith({
      queryKey: queryKeys.recent("space-1"),
      exact: true
    });
    expect(
      resetQueries.mock.calls.filter(
        ([filters]) =>
          JSON.stringify(filters?.queryKey) ===
          JSON.stringify(queryKeys.children("space-1", "parent-1"))
      )
    ).toHaveLength(1);
    expect(
      queryClient.getQueryData(queryKeys.childrenRevision("space-1"))
    ).toBe(1);
  });

  it("invalidates descendant node details after an external folder write-lock change", async () => {
    const queryClient = createTestQueryClient();
    const invalidateQueries = vi.spyOn(queryClient, "invalidateQueries");
    const resetQueries = vi.spyOn(queryClient, "resetQueries");

    await applyExternalFileChanges(queryClient, "space-1", [{
      ...delta(11, "folder-1", ["root-1"]),
      item_kind: "folder",
      write_lock_changed: true
    }]);

    expect(invalidateQueries).toHaveBeenCalledWith({
      queryKey: queryKeys.nodes("space-1")
    });
    expect(resetQueries).toHaveBeenCalledWith({
      queryKey: queryKeys.recent("space-1"),
      exact: true
    });
    expect(resetQueries).toHaveBeenCalledWith({
      queryKey: queryKeys.childrenFamily("space-1")
    });
    expect(
      queryClient.getQueryData(queryKeys.childrenRevision("space-1"))
    ).toBe(1);
  });

  it("refreshes every summary and detail affected by an inherited write lock", () => {
    const queryClient = createTestQueryClient();
    const invalidateQueries = vi.spyOn(queryClient, "invalidateQueries");
    const resetQueries = vi.spyOn(queryClient, "resetQueries");

    invalidateWriteLockState(queryClient, "space-1");

    expect(resetQueries).toHaveBeenNthCalledWith(1, {
      queryKey: queryKeys.recent("space-1"),
      exact: true
    });
    expect(resetQueries).toHaveBeenNthCalledWith(2, {
      queryKey: queryKeys.childrenFamily("space-1")
    });
    expect(invalidateQueries).toHaveBeenCalledWith({
      queryKey: queryKeys.nodes("space-1")
    });
    expect(
      queryClient.getQueryData(queryKeys.childrenRevision("space-1"))
    ).toBe(1);
  });

  it("falls back to the children family when an external event has no parent context", async () => {
    const queryClient = createTestQueryClient();
    const resetQueries = vi.spyOn(queryClient, "resetQueries");

    await applyExternalFileChanges(queryClient, "space-1", [
      delta(11, "text-1", [])
    ]);

    expect(resetQueries).toHaveBeenCalledWith({
      queryKey: queryKeys.childrenFamily("space-1")
    });
    expect(
      queryClient.getQueryData(queryKeys.childrenRevision("space-1"))
    ).toBe(1);
  });

  it("drops descendant content caches after an external recursive folder delete", async () => {
    const queryClient = createTestQueryClient();
    const deletedContent = queryKeys.text("space-1", "child-1");
    const unrelatedContent = queryKeys.text("space-2", "child-2");
    queryClient.setQueryData(deletedContent, { content: "deleted" });
    queryClient.setQueryData(unrelatedContent, { content: "keep" });

    await applyExternalFileChanges(queryClient, "space-1", [{
      ...delta(11, "folder-1", ["root-1"]),
      op_type: "item.delete",
      item_kind: "folder",
      path_changed: true,
      subtree_changed: true
    }]);

    expect(queryClient.getQueryData(deletedContent)).toBeUndefined();
    expect(queryClient.getQueryData(unrelatedContent)).toEqual({ content: "keep" });
  });

  it("keeps file preview URLs outside space resource invalidation", () => {
    const queryClient = createTestQueryClient();
    const previewKey = queryKeys.filePreviewUrl("space-1", "file-1", "image");
    queryClient.setQueryData(previewKey, { url: "https://storage.example/preview" });

    invalidateSpaceResources(queryClient, "space-1");

    expect(previewKey).toEqual(["file-preview-urls", "space-1", "file-1", "image"]);
    expect(queryClient.getQueryState(previewKey)?.isInvalidated).toBe(false);
  });

  it("removes markdown preview caches only for the changed space", () => {
    const queryClient = createTestQueryClient();
    const changed = queryKeys.markdownImagePreview("space-1", "/old/image.png");
    const other = queryKeys.markdownImagePreview("space-2", "/other/image.png");
    queryClient.setQueryData(changed, { id: "image-1" });
    queryClient.setQueryData(other, { id: "image-2" });

    removeMarkdownImageQueries(queryClient, "space-1");

    expect(queryClient.getQueryData(changed)).toBeUndefined();
    expect(queryClient.getQueryData(other)).toEqual({ id: "image-2" });
  });

  it("removes only the affected markdown preview path for a local file move", () => {
    const queryClient = createTestQueryClient();
    const changed = queryKeys.markdownImagePreview("space-1", "/old/image.png");
    const sibling = queryKeys.markdownImagePreview("space-1", "/other/image.png");
    queryClient.setQueryData(changed, { id: "image-1" });
    queryClient.setQueryData(sibling, { id: "image-2" });

    removeMarkdownImagePreviewQuery(queryClient, "space-1", "/old/image.png");

    expect(queryClient.getQueryData(changed)).toBeUndefined();
    expect(queryClient.getQueryData(sibling)).toEqual({ id: "image-2" });
  });

  it("removes only the deleted node resources for a non-recursive delete", async () => {
    const queryClient = createTestQueryClient();
    const deletedNode = {
      id: "file-1",
      space_id: "space-1",
      kind: "file" as const,
      path: "/file-1"
    };
    const deletedKeys = [
      queryKeys.node("space-1", "file-1"),
      queryKeys.text("space-1", "file-1"),
      queryKeys.file("space-1", "file-1"),
      queryKeys.markdownImagePreview("space-1", "/file-1"),
      queryKeys.filePreviewUrl("space-1", "file-1", "image"),
      queryKeys.filePreviewUrl("space-1", "file-1", "pdf"),
      queryKeys.audioPreviewUrl("space-1", "file-1")
    ];
    deletedKeys.forEach((queryKey) => queryClient.setQueryData(queryKey, { cached: true }));
    const unrelatedPreviewKey = queryKeys.filePreviewUrl("space-1", "file-2", "image");
    queryClient.setQueryData(unrelatedPreviewKey, { cached: true });

    await removeDeletedNodeQueries(queryClient, deletedNode, false);

    deletedKeys.forEach((queryKey) => expect(queryClient.getQueryData(queryKey)).toBeUndefined());
    expect(queryClient.getQueryData(unrelatedPreviewKey)).toEqual({ cached: true });
  });

  it("removes resource and preview queries only for the deleted space", async () => {
    const queryClient = createTestQueryClient();
    const deletedSpaceNode = queryKeys.node("space-1", "file-1");
    const otherSpaceNode = queryKeys.node("space-2", "file-2");
    const deletedSpacePreview = queryKeys.filePreviewUrl("space-1", "file-1", "image");
    const deletedSpaceAudio = queryKeys.audioPreviewUrl("space-1", "file-1");
    const otherSpacePreview = queryKeys.filePreviewUrl("space-2", "file-2", "image");
    const otherSpaceAudio = queryKeys.audioPreviewUrl("space-2", "file-2");
    const deletedMarkdownPreview = queryKeys.markdownImagePreview("space-1", "/image.png");
    const otherMarkdownPreview = queryKeys.markdownImagePreview("space-2", "/image.png");
    queryClient.setQueryData(deletedSpaceNode, { cached: true });
    queryClient.setQueryData(otherSpaceNode, { cached: true });
    queryClient.setQueryData(deletedSpacePreview, { cached: true });
    queryClient.setQueryData(deletedSpaceAudio, { cached: true });
    queryClient.setQueryData(otherSpacePreview, { cached: true });
    queryClient.setQueryData(otherSpaceAudio, { cached: true });
    queryClient.setQueryData(deletedMarkdownPreview, { status: "ready" });
    queryClient.setQueryData(otherMarkdownPreview, { status: "ready" });

    await removeDeletedSpaceQueries(queryClient, "space-1");

    expect(queryClient.getQueryData(deletedSpaceNode)).toBeUndefined();
    expect(queryClient.getQueryData(deletedSpacePreview)).toBeUndefined();
    expect(queryClient.getQueryData(deletedSpaceAudio)).toBeUndefined();
    expect(queryClient.getQueryData(deletedMarkdownPreview)).toBeUndefined();
    expect(queryClient.getQueryData(otherSpaceNode)).toEqual({ cached: true });
    expect(queryClient.getQueryData(otherSpacePreview)).toEqual({ cached: true });
    expect(queryClient.getQueryData(otherSpaceAudio)).toEqual({ cached: true });
    expect(queryClient.getQueryData(otherMarkdownPreview)).toEqual({ status: "ready" });
  });
});

function delta(id: number, nodeId: string, parentIds: string[]) {
  return {
    id,
    node_id: nodeId,
    op_type: "text.write",
    item_kind: "text" as const,
    affected_parent_ids: parentIds,
    parent_scope_known: parentIds.length > 0,
    path_changed: false,
    subtree_changed: false
  };
}
