import { beforeEach, describe, expect, it } from "vitest";

import type { RestNode } from "../api/types";
import { MAX_EDITOR_GROUPS, useUiStore } from "./uiStore";

function resetStore() {
  useUiStore.setState(useUiStore.getInitialState(), true);
}

function node(id: string, name = `${id}.md`): RestNode {
  return {
    id,
    space_id: "space-1",
    parent_id: "root",
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
