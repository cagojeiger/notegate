import {
  MutationObserver,
  QueryClientProvider,
  type InfiniteData,
  type QueryClient
} from "@tanstack/react-query";
import { act, renderHook } from "@testing-library/react";
import type { ReactNode } from "react";
import { describe, expect, it, vi } from "vitest";

import { deleteNode, moveNode, updateNodeSearchPolicy, updateNodeWriteLock } from "../../api/nodes";
import { queryKeys } from "../../api/queryKeys";
import { updateTextEncryption } from "../../api/text";
import type { ChildrenResponse, RestNode } from "../../api/types";
import { makeRestNode } from "../../test/fixtures";
import { createTestQueryClient } from "../../test/queryClient";
import {
  createUpdateNodeWriteLockMutationOptions,
  useDeleteNodeMutation,
  useMoveNodeMutation,
  useUpdateNodeSearchPolicyMutation,
  useUpdateNodeWriteLockMutation,
  useUpdateTextEncryptionMutation
} from "./useWorkbenchNodeQueries";

vi.mock("../../api/ApiProvider", () => ({
  useApiClient: () => ({})
}));

vi.mock("../../api/nodes", () => ({
  createNode: vi.fn(),
  deleteNode: vi.fn(),
  moveNode: vi.fn(),
  revealNode: vi.fn(),
  updateNode: vi.fn(),
  updateNodeSearchPolicy: vi.fn(),
  updateNodeWriteLock: vi.fn()
}));

vi.mock("../../api/text", () => ({
  updateTextEncryption: vi.fn()
}));

describe("workbench node mutations", () => {
  it("updates search policy through its dedicated endpoint", async () => {
    const queryClient = createTestQueryClient();
    const current = node("text-1", "space-1", "text");
    const updated = {
      ...current,
      search_enabled: false
    };
    vi.mocked(updateNodeSearchPolicy).mockResolvedValue(updated);
    const onUpdated = vi.fn();
    const result = renderMutationHook(queryClient, () => useUpdateNodeSearchPolicyMutation(onUpdated));

    await act(async () => {
      await result.current.mutateAsync({
        node: current,
        enabled: false
      });
    });

    expect(updateNodeSearchPolicy).toHaveBeenCalledWith(
      expect.anything(),
      current.space_id,
      current.id,
      false,
      current.revision
    );
    expect(queryClient.getQueryData(queryKeys.node(current.space_id, current.id))).toEqual(updated);
    expect(onUpdated).toHaveBeenCalledWith(updated);
  });

  it("updates text encryption through the text policy endpoint", async () => {
    const queryClient = createTestQueryClient();
    const current = node("text-1", "space-1", "text");
    const updated = {
      ...current,
      text_at_rest_encryption: "server" as const
    };
    vi.mocked(updateTextEncryption).mockResolvedValue(updated);
    const invalidateQueries = vi.spyOn(queryClient, "invalidateQueries");
    const onUpdated = vi.fn();
    const result = renderMutationHook(queryClient, () => useUpdateTextEncryptionMutation(onUpdated));

    await act(async () => {
      await result.current.mutateAsync({
        node: current,
        enabled: true
      });
    });

    expect(updateTextEncryption).toHaveBeenCalledWith(
      expect.anything(),
      current.space_id,
      current.id,
      true,
      current.revision
    );
    expect(queryClient.getQueryData(queryKeys.node(current.space_id, current.id))).toEqual(updated);
    expect(invalidateQueries).toHaveBeenCalledWith({
      queryKey: queryKeys.text(current.space_id, current.id),
      exact: true
    });
    expect(onUpdated).toHaveBeenCalledWith(updated);
  });

  it("updates write-lock caches after calling the dedicated endpoint", async () => {
    const queryClient = createTestQueryClient();
    const current = node("text-1", "space-1", "text");
    const updated = {
      ...current,
      write_locked: true,
      effective_write_locked: true,
      write_lock_sources: [{ node_id: current.id, name: current.name, path: current.path }]
    };
    vi.mocked(updateNodeWriteLock).mockResolvedValue(updated);
    const invalidateQueries = vi.spyOn(queryClient, "invalidateQueries");
    const resetQueries = vi.spyOn(queryClient, "resetQueries");
    const onUpdated = vi.fn();
    const result = renderMutationHook(queryClient, () => useUpdateNodeWriteLockMutation(onUpdated));

    await act(async () => {
      await result.current.mutateAsync({ node: current, enabled: true });
    });

    expect(updateNodeWriteLock).toHaveBeenCalledWith(
      expect.anything(),
      current.space_id,
      current.id,
      true,
      current.revision
    );
    expect(queryClient.getQueryData(queryKeys.node(current.space_id, current.id))).toEqual(updated);
    expect(invalidateQueries).toHaveBeenCalledWith({
      queryKey: queryKeys.fileChangeEventsFamily(current.space_id)
    });
    expect(resetQueries).toHaveBeenCalledWith({
      queryKey: queryKeys.recent(current.space_id),
      exact: true
    });
    expect(onUpdated).toHaveBeenCalledWith(updated);
  });

  it("shows a direct write lock in cached tree rows while the request is pending", async () => {
    const queryClient = createTestQueryClient();
    const current = node("text-1", "space-1", "text");
    const updated = {
      ...current,
      write_locked: true,
      effective_write_locked: true,
      write_lock_sources: [{ node_id: current.id, name: current.name, path: current.path }]
    };
    let resolveUpdate: (node: RestNode) => void = () => undefined;
    const updateRequest = new Promise<RestNode>((resolve) => {
      resolveUpdate = resolve;
    });
    queryClient.setQueryData<InfiniteData<ChildrenResponse>>(
      queryKeys.children(current.space_id, current.parent_id!),
      {
        pages: [{
          parent: { id: current.parent_id!, path: "/" },
          children: [current],
          page: { limit: 50, returned: 1, has_more: false, next_cursor: null }
        }],
        pageParams: [null]
      }
    );
    const onUpdated = vi.fn();
    const observer = observeWriteLockMutation(
      queryClient,
      vi.fn().mockReturnValue(updateRequest),
      onUpdated
    );

    const mutation = observer.mutate({ node: current, enabled: true });

    await vi.waitFor(() => {
      const cached = queryClient.getQueryData<InfiniteData<ChildrenResponse>>(
        queryKeys.children(current.space_id, current.parent_id!)
      );
      expect(cached?.pages[0]?.children[0]?.effective_write_locked).toBe(true);
      expect(onUpdated).toHaveBeenCalledWith(updated);
    });

    resolveUpdate(updated);
    await mutation;
    expect(observer.getCurrentResult().isSuccess).toBe(true);
  });

  it("restores the previous write-lock state when an optimistic request fails", async () => {
    const queryClient = createTestQueryClient();
    const current = node("text-1", "space-1", "text");
    const onUpdated = vi.fn();
    const observer = observeWriteLockMutation(
      queryClient,
      vi.fn().mockRejectedValue(new Error("update failed")),
      onUpdated
    );

    await expect(observer.mutate({ node: current, enabled: true })).rejects.toThrow("update failed");

    expect(queryClient.getQueryData(queryKeys.node(current.space_id, current.id))).toEqual(current);
    expect(onUpdated).toHaveBeenLastCalledWith(current);
  });

  it("invalidates descendant node details when a folder lock changes", async () => {
    const queryClient = createTestQueryClient();
    const current = node("folder-1", "space-1", "folder");
    const updated = {
      ...current,
      write_locked: true,
      effective_write_locked: true,
      write_lock_sources: [{ node_id: current.id, name: current.name, path: current.path }]
    };
    const invalidateQueries = vi.spyOn(queryClient, "invalidateQueries");
    const resetQueries = vi.spyOn(queryClient, "resetQueries");
    const observer = observeWriteLockMutation(
      queryClient,
      vi.fn().mockResolvedValue(updated)
    );

    await observer.mutate({ node: current, enabled: true });

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
    expect(invalidateQueries).toHaveBeenCalledWith({
      queryKey: queryKeys.fileChangeEventsFamily("space-1")
    });
  });

  it("removes every preview URL cached for a recursively deleted folder", async () => {
    const queryClient = createTestQueryClient();
    const folder = node("folder-1", "space-1", "folder");
    const invalidateQueries = vi.spyOn(queryClient, "invalidateQueries");
    const resetQueries = vi.spyOn(queryClient, "resetQueries");
    queryClient.setQueryData(queryKeys.filePreviewUrl("space-1", "child-1", "image"), { url: "child" });
    queryClient.setQueryData(queryKeys.filePreviewUrl("space-1", "child-1", "pdf"), { url: "child-pdf" });
    queryClient.setQueryData(queryKeys.filePreviewUrl("space-1", "other-1", "image"), { url: "other" });
    queryClient.setQueryData(queryKeys.filePreviewUrl("space-2", "file-2", "pdf"), { url: "separate" });
    vi.mocked(deleteNode).mockResolvedValue(undefined);

    const result = renderMutationHook(queryClient, () => useDeleteNodeMutation(vi.fn()));

    await act(async () => {
      await result.current.mutateAsync({ node: folder, recursive: true });
    });

    expect(deleteNode).toHaveBeenCalledWith(
      expect.anything(),
      folder.space_id,
      folder.id,
      true,
      folder.revision
    );
    expect(queryClient.getQueryData(queryKeys.filePreviewUrl("space-1", "child-1", "image"))).toBeUndefined();
    expect(queryClient.getQueryData(queryKeys.filePreviewUrl("space-1", "child-1", "pdf"))).toBeUndefined();
    expect(queryClient.getQueryData(queryKeys.filePreviewUrl("space-1", "other-1", "image"))).toBeUndefined();
    expect(queryClient.getQueryData(queryKeys.filePreviewUrl("space-2", "file-2", "pdf"))).toEqual({ url: "separate" });
    expect(resetQueries).toHaveBeenCalledWith({
      queryKey: queryKeys.recent("space-1"),
      exact: true
    });
    expect(resetQueries).toHaveBeenCalledWith({
      queryKey: queryKeys.childrenFamily("space-1")
    });
    expect(invalidateQueries).toHaveBeenCalledWith({
      queryKey: queryKeys.nodes("space-1")
    });
    expect(invalidateQueries).toHaveBeenCalledWith({
      queryKey: queryKeys.linkIndex("space-1")
    });
    expect(resetQueries).toHaveBeenCalledTimes(2);
    expect(invalidateQueries).toHaveBeenCalledTimes(2);
  });

  it("invalidates only the old and new parent when moving a node", async () => {
    const queryClient = createTestQueryClient();
    const source = node("file-1", "space-1", "file");
    const moved = { ...source, parent_id: "folder-2", path: "/folder-2/file-1" };
    vi.mocked(moveNode).mockResolvedValue(moved);
    const resetQueries = vi.spyOn(queryClient, "resetQueries");
    const result = renderMutationHook(queryClient, () => useMoveNodeMutation(vi.fn()));

    await act(async () => {
      await result.current.mutateAsync({ node: source, parentId: "folder-2" });
    });

    expect(moveNode).toHaveBeenCalledWith(
      expect.anything(),
      source.space_id,
      source.id,
      { new_parent_id: "folder-2", expected_revision: source.revision }
    );
    expect(resetQueries).toHaveBeenNthCalledWith(1, {
      queryKey: queryKeys.recent("space-1"),
      exact: true
    });
    expect(resetQueries).toHaveBeenNthCalledWith(2, {
      queryKey: queryKeys.children("space-1", "space-1-root")
    });
    expect(resetQueries).toHaveBeenNthCalledWith(3, {
      queryKey: queryKeys.children("space-1", "folder-2")
    });
    expect(resetQueries).toHaveBeenCalledTimes(3);
  });

  it("invalidates descendant cache families when moving a folder", async () => {
    const queryClient = createTestQueryClient();
    const source = node("folder-1", "space-1", "folder");
    const moved = { ...source, parent_id: "folder-2", path: "/folder-2/folder-1" };
    vi.mocked(moveNode).mockResolvedValue(moved);
    const invalidateQueries = vi.spyOn(queryClient, "invalidateQueries");
    const resetQueries = vi.spyOn(queryClient, "resetQueries");
    const result = renderMutationHook(queryClient, () => useMoveNodeMutation(vi.fn()));

    await act(async () => {
      await result.current.mutateAsync({ node: source, parentId: "folder-2" });
    });

    expect(resetQueries).toHaveBeenNthCalledWith(1, {
      queryKey: queryKeys.recent("space-1"),
      exact: true
    });
    expect(resetQueries).toHaveBeenNthCalledWith(2, {
      queryKey: queryKeys.childrenFamily("space-1")
    });
    expect(invalidateQueries).toHaveBeenNthCalledWith(1, {
      queryKey: queryKeys.linkIndex("space-1")
    });
    expect(invalidateQueries).toHaveBeenNthCalledWith(2, {
      queryKey: queryKeys.nodes("space-1")
    });
    expect(resetQueries).toHaveBeenCalledTimes(2);
    expect(invalidateQueries).toHaveBeenCalledTimes(2);
  });
});

function renderMutationHook<Result>(queryClient: QueryClient, callback: () => Result) {
  const wrapper = ({ children }: { children: ReactNode }) => (
    <QueryClientProvider client={queryClient}>{children}</QueryClientProvider>
  );
  return renderHook(callback, { wrapper }).result;
}

function observeWriteLockMutation(
  queryClient: QueryClient,
  updateRequest: (variables: { node: RestNode; enabled: boolean }) => Promise<RestNode>,
  onUpdated: (node: RestNode) => void = vi.fn()
) {
  return new MutationObserver(
    queryClient,
    createUpdateNodeWriteLockMutationOptions(updateRequest, queryClient, onUpdated)
  );
}

function node(id: string, spaceId: string, kind: RestNode["kind"]): RestNode {
  return makeRestNode({
    id,
    space_id: spaceId,
    parent_id: `${spaceId}-root`,
    name: id,
    kind,
    path: `/${id}`,
    has_children: kind === "folder"
  });
}
