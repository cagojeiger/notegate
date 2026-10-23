import { MutationObserver, QueryClientProvider } from "@tanstack/react-query";
import { act, renderHook, waitFor } from "@testing-library/react";
import { createElement, type ReactNode } from "react";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { queryKeys } from "../../api/queryKeys";
import type { Space, SpacesListResponse } from "../../api/types";
import { makeSpace } from "../../test/fixtures";
import { createTestQueryClient } from "../../test/queryClient";
import {
  createSpaceMutationOptions,
  useCreateSpaceMutation,
  useReorderSpacesMutation,
  useDeleteSpaceMutation
} from "./useSpaceQueries";

const apiClient = vi.hoisted(() => ({ post: vi.fn(), delete: vi.fn(), current: true }));

vi.mock("../../api/ApiProvider", () => ({
  useApiClient: () => apiClient,
  useIsCurrentApiCache: () => () => apiClient.current
}));
beforeEach(() => { apiClient.current = true; });

describe("createSpaceMutationOptions", () => {
  it("updates the spaces cache before notifying the caller", async () => {
    const queryClient = createTestQueryClient();
    const existingSpace = makeSpace({ id: "existing", sort_order: 10 });
    const createdSpace = makeSpace({ id: "created", sort_order: 5 });
    queryClient.setQueryData<SpacesListResponse>(queryKeys.spaces, {
      spaces: [existingSpace],
      page: { limit: 100, returned: 1, has_more: false, next_cursor: null }
    });
    const createRequest = vi.fn().mockResolvedValue(createdSpace);
    const onCreated = vi.fn(() => {
      expect(queryClient.getQueryData<SpacesListResponse>(queryKeys.spaces)).toEqual({
        spaces: [createdSpace, existingSpace],
        page: { limit: 100, returned: 2, has_more: false, next_cursor: null }
      });
    });
    const invalidateQueries = vi.spyOn(queryClient, "invalidateQueries");
    const observer = new MutationObserver(
      queryClient,
      createSpaceMutationOptions(createRequest, queryClient, onCreated)
    );

    await observer.mutate("Created");

    expect(createRequest.mock.calls[0]?.[0]).toBe("Created");
    expect(onCreated).toHaveBeenCalledWith(createdSpace);
    expect(invalidateQueries).toHaveBeenCalledWith({
      queryKey: queryKeys.spaces,
      exact: true
    });
    expect(invalidateQueries).toHaveBeenCalledWith({ queryKey: queryKeys.usage });
    expect(invalidateQueries).toHaveBeenCalledWith({ queryKey: queryKeys.auditEvents });
  });

  it("seeds an empty spaces cache before notifying the caller", async () => {
    const queryClient = createTestQueryClient();
    const createdSpace = makeSpace({ id: "created", sort_order: 0 });
    const onCreated = vi.fn(() => {
      expect(queryClient.getQueryData<SpacesListResponse>(queryKeys.spaces)).toEqual({
        spaces: [createdSpace],
        page: { limit: 100, returned: 1, has_more: false, next_cursor: null }
      });
    });
    const observer = new MutationObserver(
      queryClient,
      createSpaceMutationOptions(
        vi.fn().mockResolvedValue(createdSpace),
        queryClient,
        onCreated
      )
    );

    await observer.mutate("Created");

    expect(onCreated).toHaveBeenCalledWith(createdSpace);
  });

  it("does not select a newly created space after its authenticated cache is replaced", async () => {
    const queryClient = createTestQueryClient();
    const createdSpace = makeSpace({ id: "old-owner-space" });
    let finish!: (space: Space) => void;
    apiClient.post.mockReturnValueOnce(new Promise<Space>((resolve) => { finish = resolve; }));
    const onCreated = vi.fn();
    const wrapper = ({ children }: { children: ReactNode }) => createElement(QueryClientProvider, { client: queryClient }, children);
    const { result } = renderHook(() => useCreateSpaceMutation(onCreated), { wrapper });
    let pending!: Promise<Space>;
    act(() => { pending = result.current.mutateAsync("Old owner's space"); });
    await waitFor(() => expect(apiClient.post).toHaveBeenCalled());
    apiClient.current = false;
    await act(async () => { finish(createdSpace); await pending; });
    expect(onCreated).not.toHaveBeenCalled();
  });
});

describe("useReorderSpacesMutation", () => {
  it("cancels only the exact spaces list before applying the optimistic order", async () => {
    const queryClient = createTestQueryClient();
    const first = makeSpace({ id: "first", sort_order: 1_000 });
    const second = makeSpace({ id: "second", sort_order: 2_000 });
    queryClient.setQueryData<SpacesListResponse>(queryKeys.spaces, {
      spaces: [first, second],
      page: { limit: 100, returned: 2, has_more: false, next_cursor: null }
    });
    const cancelQueries = vi.spyOn(queryClient, "cancelQueries");
    apiClient.post.mockResolvedValue(undefined);
    const wrapper = ({ children }: { children: ReactNode }) =>
      createElement(QueryClientProvider, { client: queryClient }, children);
    const { result } = renderHook(() => useReorderSpacesMutation(), { wrapper });

    await act(async () => {
      await result.current.mutateAsync({ spaces: [second, first] });
    });

    expect(cancelQueries).toHaveBeenCalledWith({
      queryKey: queryKeys.spaces,
      exact: true
    });
  });
});

describe("useDeleteSpaceMutation", () => {
  it("refreshes trash and usage and removes cached resources after deletion", async () => {
    const queryClient = createTestQueryClient();
    queryClient.setQueryData(queryKeys.node("space-1", "note-1"), { id: "note-1" });
    queryClient.setQueryData(queryKeys.usage, { total: 100 });
    queryClient.setQueryData(queryKeys.trash, { pages: [] });
    const onDeleted = vi.fn();
    apiClient.delete.mockResolvedValue(undefined);
    const wrapper = ({ children }: { children: ReactNode }) =>
      createElement(QueryClientProvider, { client: queryClient }, children);
    const { result } = renderHook(() => useDeleteSpaceMutation(onDeleted), { wrapper });
    await act(async () => { await result.current.mutateAsync("space-1"); });
    expect(queryClient.getQueryData(queryKeys.node("space-1", "note-1"))).toBeUndefined();
    expect(queryClient.getQueryState(queryKeys.usage)?.isInvalidated).toBe(true);
    expect(queryClient.getQueryState(queryKeys.trash)?.isInvalidated).toBe(true);
    expect(onDeleted).toHaveBeenCalledWith("space-1");
  });

  it("rechecks cache ownership after asynchronous deletion cleanup before changing the workspace", async () => {
    const queryClient = createTestQueryClient();
    let finishCleanup!: () => void;
    const cleanup = new Promise<void>((resolve) => { finishCleanup = resolve; });
    const cancel = vi.spyOn(queryClient, "cancelQueries").mockReturnValue(cleanup);
    apiClient.delete.mockResolvedValueOnce(undefined);
    const onDeleted = vi.fn();
    const wrapper = ({ children }: { children: ReactNode }) => createElement(QueryClientProvider, { client: queryClient }, children);
    const { result } = renderHook(() => useDeleteSpaceMutation(onDeleted), { wrapper });
    let pending!: Promise<void>;
    act(() => { pending = result.current.mutateAsync("space-1"); });
    await waitFor(() => expect(cancel).toHaveBeenCalled());
    apiClient.current = false;
    await act(async () => { finishCleanup(); await pending; });
    expect(onDeleted).not.toHaveBeenCalled();
  });
});
