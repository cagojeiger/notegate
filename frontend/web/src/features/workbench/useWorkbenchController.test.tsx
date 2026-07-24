import { renderHook } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

import type { Me, RestNode, Space } from "../../api/types";
import { useOpenedNodeCache } from "../editor/useEditorQueries";
import { useUiStore } from "../../stores/uiStore";
import { useWorkbenchController } from "./useWorkbenchController";

vi.mock("../editor/useEditorQueries", () => ({
  useOpenedNodeCache: vi.fn()
}));

vi.mock("../../shared/hooks/useMediaQuery", () => ({
  useIsMobile: () => false
}));

vi.mock("./useWorkbenchActions", () => ({
  useWorkbenchActions: () => ({ settingsOpen: false, dialog: null, actions: {} })
}));

vi.mock("./useWorkbenchPersistence", () => ({
  useWorkbenchPersistence: vi.fn()
}));

vi.mock("./useWorkbenchQueries", () => ({
  useSpacesQuery: () => ({ data: { spaces: [space] }, isLoading: false, isError: false, error: null })
}));

const space: Space = {
  id: "space-1",
  name: "Daily",
  sort_order: 0,
  permission: "write",
  root_node_id: "root-1",
  created_at: "2026-07-24T00:00:00Z",
  updated_at: "2026-07-24T00:00:00Z"
};

const node: RestNode = {
  id: "node-1",
  space_id: space.id,
  parent_id: space.root_node_id,
  name: "renamed.md",
  kind: "text",
  path: "/renamed.md",
  sort_order: 0,
  metadata: {},
  has_children: false,
  created_by: { id: "user-1", kind: "user", display_name: "User" },
  updated_by: { id: "user-1", kind: "user", display_name: "User" },
  created_at: "2026-07-24T00:00:00Z",
  updated_at: "2026-07-24T00:00:00Z"
};

const me: Me = {
  account: { id: "user-1", kind: "user", display_name: "User" },
  user: { email: "user@example.com" },
  capabilities: { can_create_space: true, can_manage_agents: true }
};

describe("useWorkbenchController", () => {
  beforeEach(() => {
    useUiStore.setState(useUiStore.getInitialState(), true);
    useUiStore.getState().setActiveSpaceId(space.id);
    useUiStore.getState().openInActiveGroup(node);
  });

  it("resolves the active node detail from the query-owned node reference", () => {
    vi.mocked(useOpenedNodeCache).mockReturnValue({ data: node } as never);

    const { result } = renderHook(() => useWorkbenchController({ me, onSignOut: vi.fn() }));

    expect(useOpenedNodeCache).toHaveBeenCalledWith({ nodeId: node.id, spaceId: node.space_id });
    expect(result.current.activeNode).toBe(node);
    expect(result.current.editorGroups[0].nodeRef).toEqual({ nodeId: node.id, spaceId: node.space_id });
  });
});
