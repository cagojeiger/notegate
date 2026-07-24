import { QueryClient } from "@tanstack/react-query";
import { describe, expect, it, vi } from "vitest";

import {
  applyExternalFileChanges,
  invalidateFolderSubtree,
  invalidateNodeLists,
  invalidateSpaceResources,
  removeMarkdownImageNodeQueries,
  removeDeletedNodeQueries,
  removeDeletedSpaceQueries
} from "./queryInvalidation";
import { queryKeys } from "./queryKeys";

describe("query invalidation", () => {
  it("invalidates only Recent and the affected parent folders for a node change", () => {
    const queryClient = new QueryClient();
    const invalidateQueries = vi.spyOn(queryClient, "invalidateQueries");

    invalidateNodeLists(queryClient, "space-1", ["parent-1", "parent-2", "parent-1", null]);

    expect(invalidateQueries).toHaveBeenNthCalledWith(1, {
      queryKey: queryKeys.recent("space-1"),
      exact: true
    });
    expect(invalidateQueries).toHaveBeenNthCalledWith(2, {
      queryKey: queryKeys.children("space-1", "parent-1")
    });
    expect(invalidateQueries).toHaveBeenNthCalledWith(3, {
      queryKey: queryKeys.children("space-1", "parent-2")
    });
    expect(invalidateQueries).toHaveBeenCalledTimes(3);
  });

  it("can refresh a space subtree without invalidating the spaces list", () => {
    const queryClient = new QueryClient();
    const invalidateQueries = vi.spyOn(queryClient, "invalidateQueries");

    invalidateSpaceResources(queryClient, "space-1");

    expect(invalidateQueries).toHaveBeenCalledOnce();
    expect(invalidateQueries).toHaveBeenCalledWith({ queryKey: ["spaces", "space-1"] });
  });

  it("invalidates descendant-bearing cache families after a folder path change", () => {
    const queryClient = new QueryClient();
    const invalidateQueries = vi.spyOn(queryClient, "invalidateQueries");
    const pathKey = queryKeys.markdownImageNode("space-1", "/folder/image.png");
    queryClient.setQueryData(pathKey, { id: "image-1" });

    invalidateFolderSubtree(queryClient, "space-1");

    expect(invalidateQueries).toHaveBeenNthCalledWith(1, {
      queryKey: queryKeys.recent("space-1"),
      exact: true
    });
    expect(invalidateQueries).toHaveBeenNthCalledWith(2, {
      queryKey: queryKeys.childrenFamily("space-1")
    });
    expect(invalidateQueries).toHaveBeenNthCalledWith(3, {
      queryKey: queryKeys.nodes("space-1")
    });
    expect(invalidateQueries).toHaveBeenCalledTimes(3);
    expect(queryClient.getQueryData(pathKey)).toBeUndefined();
  });

  it("coalesces multiple external changes into one list refresh per affected parent", async () => {
    const queryClient = new QueryClient();
    const invalidateQueries = vi.spyOn(queryClient, "invalidateQueries");

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
    expect(invalidateQueries).toHaveBeenCalledWith({
      queryKey: queryKeys.recent("space-1"),
      exact: true
    });
    expect(
      invalidateQueries.mock.calls.filter(
        ([filters]) =>
          JSON.stringify(filters?.queryKey) ===
          JSON.stringify(queryKeys.children("space-1", "parent-1"))
      )
    ).toHaveLength(1);
  });

  it("falls back to the children family when an external event has no parent context", async () => {
    const queryClient = new QueryClient();
    const invalidateQueries = vi.spyOn(queryClient, "invalidateQueries");

    await applyExternalFileChanges(queryClient, "space-1", [
      delta(11, "text-1", [])
    ]);

    expect(invalidateQueries).toHaveBeenCalledWith({
      queryKey: queryKeys.childrenFamily("space-1")
    });
  });

  it("drops descendant content caches after an external recursive folder delete", async () => {
    const queryClient = new QueryClient();
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
    const queryClient = new QueryClient();
    const previewKey = queryKeys.filePreviewUrl("space-1", "file-1");
    queryClient.setQueryData(previewKey, { url: "https://storage.example/preview" });

    invalidateSpaceResources(queryClient, "space-1");

    expect(previewKey).toEqual(["file-preview-urls", "space-1", "file-1"]);
    expect(queryClient.getQueryState(previewKey)?.isInvalidated).toBe(false);
  });

  it("removes path resolution caches only for the changed space", () => {
    const queryClient = new QueryClient();
    const changed = queryKeys.markdownImageNode("space-1", "/old/image.png");
    const other = queryKeys.markdownImageNode("space-2", "/other/image.png");
    queryClient.setQueryData(changed, { id: "image-1" });
    queryClient.setQueryData(other, { id: "image-2" });

    removeMarkdownImageNodeQueries(queryClient, "space-1");

    expect(queryClient.getQueryData(changed)).toBeUndefined();
    expect(queryClient.getQueryData(other)).toEqual({ id: "image-2" });
  });

  it("removes only the deleted node resources for a non-recursive delete", async () => {
    const queryClient = new QueryClient();
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
      queryKeys.metadata("space-1", "file-1"),
      queryKeys.markdownImageNode("space-1", "/file-1"),
      queryKeys.filePreviewUrl("space-1", "file-1")
    ];
    deletedKeys.forEach((queryKey) => queryClient.setQueryData(queryKey, { cached: true }));
    const unrelatedPreviewKey = queryKeys.filePreviewUrl("space-1", "file-2");
    queryClient.setQueryData(unrelatedPreviewKey, { cached: true });

    await removeDeletedNodeQueries(queryClient, deletedNode, false);

    deletedKeys.forEach((queryKey) => expect(queryClient.getQueryData(queryKey)).toBeUndefined());
    expect(queryClient.getQueryData(unrelatedPreviewKey)).toEqual({ cached: true });
  });

  it("removes resource and preview queries only for the deleted space", async () => {
    const queryClient = new QueryClient();
    const deletedSpaceNode = queryKeys.node("space-1", "file-1");
    const otherSpaceNode = queryKeys.node("space-2", "file-2");
    const deletedSpacePreview = queryKeys.filePreviewUrl("space-1", "file-1");
    const otherSpacePreview = queryKeys.filePreviewUrl("space-2", "file-2");
    queryClient.setQueryData(deletedSpaceNode, { cached: true });
    queryClient.setQueryData(otherSpaceNode, { cached: true });
    queryClient.setQueryData(deletedSpacePreview, { cached: true });
    queryClient.setQueryData(otherSpacePreview, { cached: true });

    await removeDeletedSpaceQueries(queryClient, "space-1");

    expect(queryClient.getQueryData(deletedSpaceNode)).toBeUndefined();
    expect(queryClient.getQueryData(deletedSpacePreview)).toBeUndefined();
    expect(queryClient.getQueryData(otherSpaceNode)).toEqual({ cached: true });
    expect(queryClient.getQueryData(otherSpacePreview)).toEqual({ cached: true });
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
