import { MutationObserver, QueryClient } from "@tanstack/react-query";
import { describe, expect, it, vi } from "vitest";

import { queryKeys } from "../../api/queryKeys";
import type { SpacesListResponse } from "../../api/types";
import { makeSpace } from "../../test/fixtures";
import { createSpaceMutationOptions } from "./useSpaceQueries";

describe("createSpaceMutationOptions", () => {
  it("updates the spaces cache before notifying the caller", async () => {
    const queryClient = new QueryClient();
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
    expect(invalidateQueries).toHaveBeenCalledWith({ queryKey: queryKeys.spaces });
    expect(invalidateQueries).toHaveBeenCalledWith({ queryKey: queryKeys.auditEvents });
  });

  it("seeds an empty spaces cache before notifying the caller", async () => {
    const queryClient = new QueryClient();
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
