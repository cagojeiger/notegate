import { QueryClient } from "@tanstack/react-query";
import { describe, expect, it, vi } from "vitest";

import {
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
