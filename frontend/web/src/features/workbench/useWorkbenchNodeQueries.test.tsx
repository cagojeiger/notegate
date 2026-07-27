import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { act, renderHook } from "@testing-library/react";
import type { ReactNode } from "react";
import { describe, expect, it, vi } from "vitest";

import { deleteNode, moveNode, updateNodeSearchPolicy, updateNodeWriteLock } from "../../api/nodes";
import { queryKeys } from "../../api/queryKeys";
import { updateTextEncryption } from "../../api/text";
import type { RestNode } from "../../api/types";
import {
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
    const queryClient = new QueryClient({
      defaultOptions: { queries: { retry: false }, mutations: { retry: false } }
    });
    const current = node("text-1", "space-1", "text");
    const updated = {
      ...current,
      search_enabled: false
    };
    vi.mocked(updateNodeSearchPolicy).mockResolvedValue(updated);
    const onUpdated = vi.fn();
    const wrapper = ({ children }: { children: ReactNode }) => (
      <QueryClientProvider client={queryClient}>{children}</QueryClientProvider>
    );
    const { result } = renderHook(() => useUpdateNodeSearchPolicyMutation(onUpdated), { wrapper });

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
      false
    );
    expect(queryClient.getQueryData(queryKeys.node(current.space_id, current.id))).toEqual(updated);
    expect(onUpdated).toHaveBeenCalledWith(updated);
  });

  it("updates text encryption through the text policy endpoint", async () => {
    const queryClient = new QueryClient({
      defaultOptions: { queries: { retry: false }, mutations: { retry: false } }
    });
    const current = node("text-1", "space-1", "text");
    const updated = {
      ...current,
      text_at_rest_encryption: "server" as const
    };
    vi.mocked(updateTextEncryption).mockResolvedValue(updated);
    const invalidateQueries = vi.spyOn(queryClient, "invalidateQueries");
    const onUpdated = vi.fn();
    const wrapper = ({ children }: { children: ReactNode }) => (
      <QueryClientProvider client={queryClient}>{children}</QueryClientProvider>
    );
    const { result } = renderHook(() => useUpdateTextEncryptionMutation(onUpdated), { wrapper });

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
      true
    );
    expect(queryClient.getQueryData(queryKeys.node(current.space_id, current.id))).toEqual(updated);
    expect(invalidateQueries).toHaveBeenCalledWith({
      queryKey: queryKeys.text(current.space_id, current.id),
      exact: true
    });
    expect(onUpdated).toHaveBeenCalledWith(updated);
  });

  it("updates the direct write lock through its dedicated endpoint", async () => {
    const queryClient = new QueryClient({
      defaultOptions: { queries: { retry: false }, mutations: { retry: false } }
    });
    const current = node("text-1", "space-1", "text");
    const updated = {
      ...current,
      write_locked: true,
      effective_write_locked: true,
      write_lock_sources: [{ node_id: current.id, name: current.name, path: current.path }]
    };
    vi.mocked(updateNodeWriteLock).mockResolvedValue(updated);
    const invalidateQueries = vi.spyOn(queryClient, "invalidateQueries");
    const onUpdated = vi.fn();
    const wrapper = ({ children }: { children: ReactNode }) => (
      <QueryClientProvider client={queryClient}>{children}</QueryClientProvider>
    );
    const { result } = renderHook(() => useUpdateNodeWriteLockMutation(onUpdated), { wrapper });

    await act(async () => {
      await result.current.mutateAsync({ node: current, enabled: true });
    });

    expect(updateNodeWriteLock).toHaveBeenCalledWith(
      expect.anything(),
      current.space_id,
      current.id,
      true
    );
    expect(queryClient.getQueryData(queryKeys.node(current.space_id, current.id))).toEqual(updated);
    expect(invalidateQueries).toHaveBeenCalledWith({
      queryKey: queryKeys.fileChangeEventsFamily(current.space_id)
    });
    expect(onUpdated).toHaveBeenCalledWith(updated);
  });

  it("invalidates descendant node details when a folder lock changes", async () => {
    const queryClient = new QueryClient({
      defaultOptions: { queries: { retry: false }, mutations: { retry: false } }
    });
    const current = node("folder-1", "space-1", "folder");
    const updated = {
      ...current,
      write_locked: true,
      effective_write_locked: true,
      write_lock_sources: [{ node_id: current.id, name: current.name, path: current.path }]
    };
    vi.mocked(updateNodeWriteLock).mockResolvedValue(updated);
    const invalidateQueries = vi.spyOn(queryClient, "invalidateQueries");
    const resetQueries = vi.spyOn(queryClient, "resetQueries");
    const wrapper = ({ children }: { children: ReactNode }) => (
      <QueryClientProvider client={queryClient}>{children}</QueryClientProvider>
    );
    const { result } = renderHook(() => useUpdateNodeWriteLockMutation(vi.fn()), { wrapper });

    await act(async () => {
      await result.current.mutateAsync({ node: current, enabled: true });
    });

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
  });

  it("removes every preview URL cached for a recursively deleted folder", async () => {
    const queryClient = new QueryClient({
      defaultOptions: { queries: { retry: false }, mutations: { retry: false } }
    });
    const folder = node("folder-1", "space-1", "folder");
    const invalidateQueries = vi.spyOn(queryClient, "invalidateQueries");
    const resetQueries = vi.spyOn(queryClient, "resetQueries");
    queryClient.setQueryData(queryKeys.filePreviewUrl("space-1", "child-1", "image"), { url: "child" });
    queryClient.setQueryData(queryKeys.filePreviewUrl("space-1", "child-1", "pdf"), { url: "child-pdf" });
    queryClient.setQueryData(queryKeys.filePreviewUrl("space-1", "other-1", "image"), { url: "other" });
    queryClient.setQueryData(queryKeys.filePreviewUrl("space-2", "file-2", "pdf"), { url: "separate" });
    vi.mocked(deleteNode).mockResolvedValue(undefined);

    const wrapper = ({ children }: { children: ReactNode }) => (
      <QueryClientProvider client={queryClient}>{children}</QueryClientProvider>
    );
    const { result } = renderHook(() => useDeleteNodeMutation(vi.fn()), { wrapper });

    await act(async () => {
      await result.current.mutateAsync({ node: folder, recursive: true });
    });

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
    expect(resetQueries).toHaveBeenCalledTimes(2);
    expect(invalidateQueries).toHaveBeenCalledOnce();
  });

  it("invalidates only the old and new parent when moving a node", async () => {
    const queryClient = new QueryClient({
      defaultOptions: { queries: { retry: false }, mutations: { retry: false } }
    });
    const source = node("file-1", "space-1", "file");
    const moved = { ...source, parent_id: "folder-2", path: "/folder-2/file-1" };
    vi.mocked(moveNode).mockResolvedValue(moved);
    const resetQueries = vi.spyOn(queryClient, "resetQueries");
    const wrapper = ({ children }: { children: ReactNode }) => (
      <QueryClientProvider client={queryClient}>{children}</QueryClientProvider>
    );
    const { result } = renderHook(() => useMoveNodeMutation(vi.fn()), { wrapper });

    await act(async () => {
      await result.current.mutateAsync({ node: source, parentId: "folder-2" });
    });

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
    const queryClient = new QueryClient({
      defaultOptions: { queries: { retry: false }, mutations: { retry: false } }
    });
    const source = node("folder-1", "space-1", "folder");
    const moved = { ...source, parent_id: "folder-2", path: "/folder-2/folder-1" };
    vi.mocked(moveNode).mockResolvedValue(moved);
    const invalidateQueries = vi.spyOn(queryClient, "invalidateQueries");
    const resetQueries = vi.spyOn(queryClient, "resetQueries");
    const wrapper = ({ children }: { children: ReactNode }) => (
      <QueryClientProvider client={queryClient}>{children}</QueryClientProvider>
    );
    const { result } = renderHook(() => useMoveNodeMutation(vi.fn()), { wrapper });

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
      queryKey: queryKeys.nodes("space-1")
    });
    expect(resetQueries).toHaveBeenCalledTimes(2);
    expect(invalidateQueries).toHaveBeenCalledOnce();
  });
});

function node(id: string, spaceId: string, kind: RestNode["kind"]): RestNode {
  return {
    id,
    space_id: spaceId,
    parent_id: `${spaceId}-root`,
    name: id,
    kind,
    path: `/${id}`,
    sort_order: 0,
    metadata: {},
    search_enabled: true,
    write_locked: false,
    effective_write_locked: false,
    write_lock_sources: [],
    has_children: kind === "folder",
    created_by: { id: "user-1", kind: "user", display_name: "User" },
    updated_by: { id: "user-1", kind: "user", display_name: "User" },
    created_at: "2026-06-13T00:00:00Z",
    updated_at: "2026-06-13T00:00:00Z"
  };
}
