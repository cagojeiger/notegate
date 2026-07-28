import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { ApiError } from "../../api/errors";
import { useUiStore } from "../../stores/uiStore";
import { makeRestNode } from "../../test/fixtures";
import { OpenedNodeGuard } from "./OpenedNodeGuard";
import type { useNodeFreshness } from "./useEditorQueries";

type NodeFreshnessQuery = ReturnType<typeof useNodeFreshness>;
type NodeFreshnessQueryMock = Pick<NodeFreshnessQuery, "data" | "error">;

const editorQueryMocks = vi.hoisted(() => ({
  useNodeFreshness: vi.fn<
    (...args: Parameters<typeof useNodeFreshness>) => NodeFreshnessQueryMock
  >()
}));

vi.mock("./useEditorQueries", () => ({
  useNodeFreshness: editorQueryMocks.useNodeFreshness
}));

const node = makeRestNode();

function renderGuard() {
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  return render(
    <QueryClientProvider client={queryClient}>
      <OpenedNodeGuard node={node}>{(freshNode) => <span>{freshNode.name}</span>}</OpenedNodeGuard>
    </QueryClientProvider>
  );
}

describe("OpenedNodeGuard", () => {
  beforeEach(() => {
    useUiStore.setState(useUiStore.getInitialState(), true);
    useUiStore.getState().openInActiveGroup(node);
  });

  it("updates opened editor groups with the latest node stat", async () => {
    editorQueryMocks.useNodeFreshness.mockReturnValue({
      data: { ...node, name: "renamed.md" },
      error: null
    });

    renderGuard();

    expect(screen.getByText("renamed.md")).toBeInTheDocument();
    await waitFor(() => expect(useUiStore.getState().editorGroups[0].node?.name).toBe("renamed.md"));
  });

  it("propagates an inherited write lock into an already opened editor group", async () => {
    const locked = {
      ...node,
      effective_write_locked: true,
      write_lock_sources: [
        { node_id: "folder-1", name: "Policies", path: "/Policies" }
      ]
    };
    editorQueryMocks.useNodeFreshness.mockReturnValue({
      data: locked,
      error: null
    });

    renderGuard();

    await waitFor(() => {
      expect(useUiStore.getState().editorGroups[0].node).toMatchObject({
        effective_write_locked: true,
        write_lock_sources: locked.write_lock_sources
      });
    });
  });

  it("clears an opened editor group when the node was deleted elsewhere", async () => {
    editorQueryMocks.useNodeFreshness.mockReturnValue({
      data: undefined,
      error: new ApiError("not found", 404)
    });

    renderGuard();

    await waitFor(() => expect(useUiStore.getState().editorGroups[0].node).toBeNull());
  });
});
