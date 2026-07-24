import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { act, renderHook } from "@testing-library/react";
import type { ReactNode } from "react";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { queryKeys } from "../../api/queryKeys";
import { createSpace } from "../../api/spaces";
import type { SpacesListResponse } from "../../api/types";
import type { Space } from "../../entities/space/model";
import { useCreateSpaceMutation } from "./useWorkbenchSpaceQueries";

const mockClient = vi.hoisted(() => ({}));

vi.mock("../../api/ApiProvider", () => ({
  useApiClient: () => mockClient
}));

vi.mock("../../api/spaces", () => ({
  createSpace: vi.fn()
}));

describe("useCreateSpaceMutation", () => {
  beforeEach(() => {
    vi.mocked(createSpace).mockReset();
  });

  it("adds the created space to the cache before activating it", async () => {
    const queryClient = new QueryClient();
    const existingSpace = space("space-existing", 0);
    const createdSpace = space("space-created", 1);
    queryClient.setQueryData<SpacesListResponse>(queryKeys.spaces, {
      spaces: [existingSpace],
      page: { limit: 100, returned: 1, has_more: false, next_cursor: null }
    });
    vi.mocked(createSpace).mockResolvedValue(createdSpace);
    const onCreated = vi.fn(() => {
      expect(queryClient.getQueryData<SpacesListResponse>(queryKeys.spaces)?.spaces).toEqual([
        existingSpace,
        createdSpace
      ]);
    });
    const wrapper = ({ children }: { children: ReactNode }) => (
      <QueryClientProvider client={queryClient}>{children}</QueryClientProvider>
    );
    const { result } = renderHook(() => useCreateSpaceMutation(onCreated), { wrapper });

    await act(async () => {
      await result.current.mutateAsync("Created");
    });

    expect(createSpace).toHaveBeenCalledWith(mockClient, "Created");
    expect(onCreated).toHaveBeenCalledWith(createdSpace);
    expect(queryClient.getQueryData<SpacesListResponse>(queryKeys.spaces)?.page.returned).toBe(2);
  });

  it("seeds an empty cache before activating the created space", async () => {
    const queryClient = new QueryClient();
    const createdSpace = space("space-created", 0);
    vi.mocked(createSpace).mockResolvedValue(createdSpace);
    const onCreated = vi.fn(() => {
      expect(queryClient.getQueryData<SpacesListResponse>(queryKeys.spaces)?.spaces).toEqual([
        createdSpace
      ]);
    });
    const wrapper = ({ children }: { children: ReactNode }) => (
      <QueryClientProvider client={queryClient}>{children}</QueryClientProvider>
    );
    const { result } = renderHook(() => useCreateSpaceMutation(onCreated), { wrapper });

    await act(async () => {
      await result.current.mutateAsync("Created");
    });

    expect(onCreated).toHaveBeenCalledWith(createdSpace);
  });
});

function space(id: string, sortOrder: number): Space {
  return {
    id,
    name: id,
    sort_order: sortOrder,
    permission: "write",
    root_node_id: `${id}-root`,
    created_at: "2026-07-24T00:00:00Z",
    updated_at: "2026-07-24T00:00:00Z"
  };
}
