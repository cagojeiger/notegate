import { QueryClient } from "@tanstack/react-query";
import { describe, expect, it, vi } from "vitest";

import { invalidateSpace, invalidateSpaceResources } from "./queryInvalidation";

describe("query invalidation", () => {
  it("invalidates the spaces list exactly and only the changed space subtree", () => {
    const queryClient = new QueryClient();
    const invalidateQueries = vi.spyOn(queryClient, "invalidateQueries");

    invalidateSpace(queryClient, "space-1");

    expect(invalidateQueries).toHaveBeenNthCalledWith(1, { queryKey: ["spaces"], exact: true });
    expect(invalidateQueries).toHaveBeenNthCalledWith(2, { queryKey: ["spaces", "space-1"] });
  });

  it("can refresh a space subtree without invalidating the spaces list", () => {
    const queryClient = new QueryClient();
    const invalidateQueries = vi.spyOn(queryClient, "invalidateQueries");

    invalidateSpaceResources(queryClient, "space-1");

    expect(invalidateQueries).toHaveBeenCalledOnce();
    expect(invalidateQueries).toHaveBeenCalledWith({ queryKey: ["spaces", "space-1"] });
  });
});
