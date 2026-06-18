import { beforeEach, describe, expect, it, vi } from "vitest";

import type { RestNode } from "../api/types";
import { MAX_EDITOR_GROUPS, useUiStore } from "./uiStore";
import { MAX_WORKBENCH_SNAPSHOTS, WORKBENCH_INDEX_KEY, clearPersistedSpaceWorkbench, persistSpaceWorkbench, workbenchSpaceKey } from "./workbenchStorage";

function resetStore() {
  useUiStore.setState(useUiStore.getInitialState(), true);
}

function node(id: string, name = `${id}.md`, spaceId = "space-1"): RestNode {
  return {
    id,
    space_id: spaceId,
    parent_id: `${spaceId}-root`,
    name,
    kind: "text",
    path: `/${name}`,
    sort_order: 0,
    metadata: {},
    has_children: false,
    created_by: { id: "user-1", kind: "user", display_name: "User" },
    updated_by: { id: "user-1", kind: "user", display_name: "User" },
    created_at: "2026-06-13T00:00:00Z",
    updated_at: "2026-06-13T00:00:00Z",
    byte_len: 12,
    line_count: 1
  };
}

describe("useUiStore", () => {
  beforeEach(resetStore);

  it("toggles theme and sidebar state", () => {
    expect(useUiStore.getState().theme).toBe("light");
    useUiStore.getState().toggleTheme();
    expect(useUiStore.getState().theme).toBe("dark");

    expect(useUiStore.getState().primarySidebarOpen).toBe(true);
    useUiStore.getState().togglePrimarySidebar();
    expect(useUiStore.getState().primarySidebarOpen).toBe(false);
  });

  it("caps editor groups at the product maximum", () => {
    for (let i = 0; i < MAX_EDITOR_GROUPS + 2; i += 1) {
      useUiStore.getState().addGroup();
    }

    expect(useUiStore.getState().editorGroups).toHaveLength(MAX_EDITOR_GROUPS);
    expect(useUiStore.getState().activeGroupIndex).toBe(MAX_EDITOR_GROUPS - 1);
  });

  it("opens nodes in the active group and resets group mode to preview", () => {
    const first = node("node-1");
    const second = node("node-2");

    useUiStore.getState().openInActiveGroup(first);
    useUiStore.getState().setGroupMode(0, "edit");
    useUiStore.getState().addGroup();
    useUiStore.getState().openInActiveGroup(second);

    const state = useUiStore.getState();
    expect(state.editorGroups[0]).toMatchObject({ node: first, mode: "edit" });
    expect(state.editorGroups[1]).toMatchObject({ node: second, mode: "preview" });
  });

  it("opens a node directly in a new editor group", () => {
    const first = node("node-1");
    const second = node("node-2");

    useUiStore.getState().openInActiveGroup(first);
    useUiStore.getState().openInNewGroup(second);

    const state = useUiStore.getState();
    expect(state.editorGroups).toHaveLength(2);
    expect(state.activeGroupIndex).toBe(1);
    expect(state.editorGroups[0]).toMatchObject({ node: first });
    expect(state.editorGroups[1]).toMatchObject({ node: second, mode: "preview" });
  });

  it("closes editor groups without removing the last group", () => {
    useUiStore.getState().addGroup();
    useUiStore.getState().addGroup();
    useUiStore.getState().focusGroup(2);

    useUiStore.getState().closeGroup(1);
    expect(useUiStore.getState().editorGroups).toHaveLength(2);
    expect(useUiStore.getState().activeGroupIndex).toBe(1);

    useUiStore.getState().closeGroup(1);
    useUiStore.getState().closeGroup(0);
    expect(useUiStore.getState().editorGroups).toHaveLength(1);
    expect(useUiStore.getState().activeGroupIndex).toBe(0);
  });

  it("restores editor groups separately for each active space", () => {
    const first = node("space-a-node-1", "a-1.md", "space-a");
    const second = node("space-a-node-2", "a-2.md", "space-a");
    const third = node("space-b-node-1", "b-1.md", "space-b");

    useUiStore.getState().setActiveSpaceId("space-a");
    useUiStore.getState().openInActiveGroup(first);
    useUiStore.getState().setGroupMode(0, "edit");
    useUiStore.getState().openInNewGroup(second);

    useUiStore.getState().setActiveSpaceId("space-b");
    expect(useUiStore.getState().editorGroups).toMatchObject([{ node: null, mode: "preview" }]);

    useUiStore.getState().openInActiveGroup(third);
    useUiStore.getState().setActiveSpaceId("space-a");

    let state = useUiStore.getState();
    expect(state.activeGroupIndex).toBe(1);
    expect(state.editorGroups).toHaveLength(2);
    expect(state.editorGroups[0]).toMatchObject({ node: first, mode: "edit" });
    expect(state.editorGroups[1]).toMatchObject({ node: second, mode: "preview" });

    useUiStore.getState().setActiveSpaceId("space-b");

    state = useUiStore.getState();
    expect(state.activeGroupIndex).toBe(0);
    expect(state.editorGroups).toHaveLength(1);
    expect(state.editorGroups[0]).toMatchObject({ node: third, mode: "preview" });
  });

  it("restores a persisted workbench snapshot when activating a space", () => {
    const first = node("node-1");
    const wrongSpaceNode = node("node-2", "wrong.md", "other-space");
    const malformedNode = { ...node("node-3"), created_by: undefined };
    window.localStorage.setItem(workbenchSpaceKey("space-1"), JSON.stringify({
      version: 1,
      spaceId: "space-1",
      updatedAt: 1,
      groups: [
        { node: first, mode: "edit" },
        { node: wrongSpaceNode, mode: "edit" },
        { node: malformedNode, mode: "edit" }
      ],
      activeGroupIndex: 9
    }));

    useUiStore.getState().setActiveSpaceId("space-1");

    const state = useUiStore.getState();
    expect(state.activeGroupIndex).toBe(2);
    expect(state.editorGroups).toHaveLength(3);
    expect(state.editorGroups[0]).toMatchObject({ node: first, mode: "edit" });
    expect(state.editorGroups[1]).toMatchObject({ node: null, mode: "preview" });
    expect(state.editorGroups[2]).toMatchObject({ node: null, mode: "preview" });
  });

  it("can clear a deleted active space snapshot after leaving the space", () => {
    const opened = node("node-1");

    useUiStore.getState().setActiveSpaceId("space-1");
    useUiStore.getState().openInActiveGroup(opened);
    useUiStore.getState().setActiveSpaceId(null);
    clearPersistedSpaceWorkbench("space-1");

    expect(window.localStorage.getItem(workbenchSpaceKey("space-1"))).toBeNull();
  });

  it("keeps only the most recent persisted space snapshots", () => {
    const now = vi.spyOn(Date, "now");

    for (let index = 0; index < MAX_WORKBENCH_SNAPSHOTS + 2; index += 1) {
      const spaceId = `space-${index}`;
      now.mockReturnValue(index);
      persistSpaceWorkbench(spaceId, [{ id: index, node: node(`node-${index}`, `${index}.md`, spaceId), mode: "preview" }], 0);
    }

    const storedIndex = JSON.parse(window.localStorage.getItem(WORKBENCH_INDEX_KEY) ?? "{}") as { spaces: { spaceId: string }[] };
    expect(storedIndex.spaces).toHaveLength(MAX_WORKBENCH_SNAPSHOTS);
    expect(window.localStorage.getItem(workbenchSpaceKey("space-0"))).toBeNull();
    expect(window.localStorage.getItem(workbenchSpaceKey("space-1"))).toBeNull();
    expect(window.localStorage.getItem(workbenchSpaceKey("space-2"))).not.toBeNull();
  });

  it("clamps resizable layout values", () => {
    useUiStore.getState().setPrimaryWidth(100);
    expect(useUiStore.getState().primaryWidth).toBe(220);
    useUiStore.getState().setPrimaryWidth(900);
    expect(useUiStore.getState().primaryWidth).toBe(520);

    useUiStore.getState().setTreeRatio(0.05);
    expect(useUiStore.getState().treeRatio).toBe(0.2);
    useUiStore.getState().setTreeRatio(0.95);
    expect(useUiStore.getState().treeRatio).toBe(0.82);
  });
});
