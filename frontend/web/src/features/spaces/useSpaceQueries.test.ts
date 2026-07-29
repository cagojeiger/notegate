import { MutationObserver, QueryClientProvider } from "@tanstack/react-query";
import { act, renderHook } from "@testing-library/react";
import { createElement, type ReactNode } from "react";
import { describe, expect, it, vi } from "vitest";

import { queryKeys } from "../../api/queryKeys";
import type { SpacesListResponse } from "../../api/types";
import { makeSpace } from "../../test/fixtures";
import { createTestQueryClient } from "../../test/queryClient";
import {
  createSpaceMutationOptions,
  useReorderSpacesMutation
} from "./useSpaceQueries";

const apiClient = vi.hoisted(() => ({ post: vi.fn() }));

vi.mock("../../api/ApiProvider", () => ({
  useApiClient: () => apiClient
}));

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
