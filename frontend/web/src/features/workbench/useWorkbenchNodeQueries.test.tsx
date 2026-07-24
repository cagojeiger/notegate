import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { act, renderHook } from "@testing-library/react";
import type { ReactNode } from "react";
import { describe, expect, it, vi } from "vitest";

import { deleteNode, updateNode } from "../../api/nodes";
import { queryKeys } from "../../api/queryKeys";
import type { RestNode } from "../../entities/node/model";
import { useDeleteNodeMutation, useUpdateNodeMutation } from "./useWorkbenchNodeQueries";

vi.mock("../../api/ApiProvider", () => ({
  useApiClient: () => ({})
}));

vi.mock("../../api/nodes", () => ({
  createNode: vi.fn(),
  deleteNode: vi.fn(),
  moveNode: vi.fn(),
  revealNode: vi.fn(),
  updateNode: vi.fn()
}));

describe("useDeleteNodeMutation", () => {
  it("removes every preview URL cached for a recursively deleted folder", async () => {
    const queryClient = new QueryClient({
      defaultOptions: { queries: { retry: false }, mutations: { retry: false } }
    });
    const folder = node("folder-1", "space-1", "folder");
    queryClient.setQueryData(queryKeys.filePreviewUrl("space-1", "child-1"), { url: "child" });
    queryClient.setQueryData(queryKeys.filePreviewUrl("space-1", "other-1"), { url: "other" });
    queryClient.setQueryData(queryKeys.filePreviewUrl("space-2", "file-2"), { url: "separate" });
    vi.mocked(deleteNode).mockResolvedValue(undefined);

    const wrapper = ({ children }: { children: ReactNode }) => (
      <QueryClientProvider client={queryClient}>{children}</QueryClientProvider>
    );
    const { result } = renderHook(() => useDeleteNodeMutation(vi.fn()), { wrapper });

    await act(async () => {
      await result.current.mutateAsync({ node: folder, recursive: true });
    });

    expect(queryClient.getQueryData(queryKeys.filePreviewUrl("space-1", "child-1"))).toBeUndefined();
    expect(queryClient.getQueryData(queryKeys.filePreviewUrl("space-1", "other-1"))).toBeUndefined();
    expect(queryClient.getQueryData(queryKeys.filePreviewUrl("space-2", "file-2"))).toEqual({ url: "separate" });
  });
});

describe("useUpdateNodeMutation", () => {
  it("writes the updated node to the authoritative query cache", async () => {
    const queryClient = new QueryClient({
      defaultOptions: { queries: { retry: false }, mutations: { retry: false } }
    });
    const original = node("node-1", "space-1", "text");
    const renamed = { ...original, name: "renamed.md", path: "/renamed.md" };
    vi.mocked(updateNode).mockResolvedValue(renamed);

    const wrapper = ({ children }: { children: ReactNode }) => (
      <QueryClientProvider client={queryClient}>{children}</QueryClientProvider>
    );
    const { result } = renderHook(() => useUpdateNodeMutation(), { wrapper });

    await act(async () => {
      await result.current.mutateAsync({ node: original, name: renamed.name });
    });

    expect(queryClient.getQueryData(queryKeys.node("space-1", "node-1"))).toEqual(renamed);
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
    has_children: kind === "folder",
    created_by: { id: "user-1", kind: "user", display_name: "User" },
    updated_by: { id: "user-1", kind: "user", display_name: "User" },
    created_at: "2026-06-13T00:00:00Z",
    updated_at: "2026-06-13T00:00:00Z"
  };
}
