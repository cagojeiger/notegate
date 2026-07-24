import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { ApiError } from "../../api/errors";
import type { RestNode } from "../../api/types";
import { useUiStore } from "../../stores/uiStore";
import { OpenedNodeGuard } from "./OpenedNodeGuard";
import { useOpenedNodeQuery } from "./useEditorQueries";

vi.mock("./useEditorQueries", () => ({
  useOpenedNodeQuery: vi.fn()
}));

const node: RestNode = {
  id: "node-1",
  space_id: "space-1",
  parent_id: "root-1",
  name: "note.md",
  kind: "text",
  path: "/note.md",
  sort_order: 0,
  metadata: {},
  has_children: false,
  created_by: { id: "user-1", kind: "user", display_name: "User" },
  updated_by: { id: "user-1", kind: "user", display_name: "User" },
  created_at: "2026-06-13T00:00:00Z",
  updated_at: "2026-06-13T00:00:00Z"
};

function renderGuard() {
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  return render(
    <QueryClientProvider client={queryClient}>
      <OpenedNodeGuard nodeRef={{ nodeId: node.id, spaceId: node.space_id }}>{(freshNode) => <span>{freshNode.name}</span>}</OpenedNodeGuard>
    </QueryClientProvider>
  );
}

describe("OpenedNodeGuard", () => {
  beforeEach(() => {
    useUiStore.setState(useUiStore.getInitialState(), true);
    useUiStore.getState().openInActiveGroup(node);
  });

  it("renders the authoritative node from React Query without copying it into UI state", () => {
    vi.mocked(useOpenedNodeQuery).mockReturnValue({ data: { ...node, name: "renamed.md" }, error: null, isLoading: false, isError: false } as never);

    renderGuard();

    expect(screen.getByText("renamed.md")).toBeInTheDocument();
    expect(useUiStore.getState().editorGroups[0].nodeRef).toEqual({ nodeId: node.id, spaceId: node.space_id });
  });

  it("clears an opened editor group when the node was deleted elsewhere", async () => {
    vi.mocked(useOpenedNodeQuery).mockReturnValue({ data: undefined, error: new ApiError("not found", 404), isLoading: false, isError: true } as never);

    renderGuard();

    await waitFor(() => expect(useUiStore.getState().editorGroups[0].nodeRef).toBeNull());
  });
});
