import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import type { RestNode } from "../api/types";
import { MAX_EDITOR_GROUPS, WORKBENCH_LAYOUT } from "../shared/model/workbenchLayout";
import { makeRestNode } from "../test/fixtures";
import { bootstrapUiStore, createHydratedUiStore, createUiStore, useUiStore } from "./uiStore";
import type { UiStorePersistence } from "./uiStorePersistence";
import { MAX_EDITOR_NAVIGATION_ENTRIES, resetEditorGroupsState, type EditorGroupState } from "./uiStoreReducers";
import { WORKBENCH_PANEL_STATE_KEY } from "./workbenchStorage";

function resetStore() {
  useUiStore.setState(useUiStore.getInitialState(), true);
}

function node(id: string, name = `${id}.md`, spaceId = "space-1"): RestNode {
  return makeRestNode({
    id,
    space_id: spaceId,
    parent_id: `${spaceId}-root`,
    name,
    path: `/${name}`,
    byte_len: 12,
    line_count: 1
  });
}

function fakePersistence(overrides: Partial<UiStorePersistence> = {}): UiStorePersistence {
  return {
    loadTheme: vi.fn((): "light" | "dark" => "light"),
    applyTheme: vi.fn(),
    saveTheme: vi.fn(),
    loadLastActiveSpaceId: vi.fn(() => null),
    saveLastActiveSpaceId: vi.fn(),
    loadSpaceWorkbench: vi.fn((_spaceId, nextGroupId) => resetEditorGroupsState({ nextGroupId })),
    saveSpaceWorkbench: vi.fn(),
    loadPanelState: vi.fn(() => ({ primarySidebarOpen: true, auxiliaryOpen: true })),
    savePanelState: vi.fn(),
    ...overrides
  };
}

describe("useUiStore", () => {
  beforeEach(resetStore);
  afterEach(() => {
    delete document.documentElement.dataset.theme;
  });

  it("toggles theme and sidebar state", () => {
    expect(useUiStore.getState().theme).toBe("light");
    useUiStore.getState().toggleTheme();
    expect(useUiStore.getState().theme).toBe("dark");

    expect(useUiStore.getState().primarySidebarOpen).toBe(true);
    useUiStore.getState().togglePrimarySidebar();
    expect(useUiStore.getState().primarySidebarOpen).toBe(false);

    expect(useUiStore.getState().auxiliaryOpen).toBe(true);
    useUiStore.getState().toggleAuxiliary();
    expect(useUiStore.getState().auxiliaryOpen).toBe(false);
    expect(JSON.parse(window.localStorage.getItem(WORKBENCH_PANEL_STATE_KEY) ?? "{}")).toMatchObject({
      primarySidebarOpen: false,
      auxiliaryOpen: false
    });
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

  it("opens a node in a specific editor group without changing focus", () => {
    const first = node("node-1");
    const second = node("node-2");
    const target = node("node-3");

    useUiStore.getState().openInActiveGroup(first);
    useUiStore.getState().openInNewGroup(second);
    useUiStore.getState().openInGroup(0, target);

    const state = useUiStore.getState();
    expect(state.activeGroupIndex).toBe(1);
    expect(state.editorGroups[0]).toMatchObject({ node: target, mode: "preview" });
    expect(state.editorGroups[1]).toMatchObject({ node: second, mode: "preview" });
  });

  it("keeps back and forward history per editor group", () => {
    const first = node("node-1");
    const second = node("node-2");
    const third = node("node-3");
    const fourth = node("node-4");

    useUiStore.getState().openInActiveGroup(first);
    useUiStore.getState().openInActiveGroup(second);
    useUiStore.getState().openInActiveGroup(third);

    let group = useUiStore.getState().editorGroups[0];
    expect(group.back.map((entry) => entry.nodeId)).toEqual([first.id, second.id]);
    expect(group.forward).toEqual([]);

    expect(useUiStore.getState().navigateGroup(group.id, "back", second.id, second)).toBe(true);
    group = useUiStore.getState().editorGroups[0];
    expect(group.node?.id).toBe(second.id);
    expect(group.back.map((entry) => entry.nodeId)).toEqual([first.id]);
    expect(group.forward.map((entry) => entry.nodeId)).toEqual([third.id]);

    expect(useUiStore.getState().navigateGroup(group.id, "forward", third.id, third)).toBe(true);
    group = useUiStore.getState().editorGroups[0];
    expect(group.node?.id).toBe(third.id);
    expect(group.back.map((entry) => entry.nodeId)).toEqual([first.id, second.id]);
    expect(group.forward).toEqual([]);

    expect(useUiStore.getState().navigateGroup(group.id, "back", second.id, second)).toBe(true);
    group = useUiStore.getState().editorGroups[0];
    useUiStore.getState().openInActiveGroup(second);
    expect(useUiStore.getState().editorGroups[0]).toMatchObject({
      back: group.back,
      forward: group.forward
    });

    useUiStore.getState().openInActiveGroup(fourth);
    group = useUiStore.getState().editorGroups[0];
    expect(group.back.map((entry) => entry.nodeId)).toEqual([first.id, second.id]);
    expect(group.forward).toEqual([]);
  });

  it("caps each editor group at fifty visited nodes", () => {
    for (let index = 0; index < MAX_EDITOR_NAVIGATION_ENTRIES + 10; index += 1) {
      useUiStore.getState().openInActiveGroup(node(`node-${index}`));
    }

    const group = useUiStore.getState().editorGroups[0];
    expect(group.back).toHaveLength(MAX_EDITOR_NAVIGATION_ENTRIES - 1);
    expect(group.back[0]?.nodeId).toBe("node-10");
    expect(group.node?.id).toBe("node-59");
  });

  it("starts new groups with independent navigation history", () => {
    const first = node("node-1");
    const second = node("node-2");
    const third = node("node-3");

    useUiStore.getState().openInActiveGroup(first);
    useUiStore.getState().openInActiveGroup(second);
    useUiStore.getState().openInNewGroup(third);

    const [firstGroup, secondGroup] = useUiStore.getState().editorGroups;
    expect(firstGroup.back.map((entry) => entry.nodeId)).toEqual([first.id]);
    expect(secondGroup).toMatchObject({ node: third, back: [], forward: [] });
  });

  it("updates and removes navigation snapshots with their node", () => {
    const first = node("node-1");
    const second = node("node-2");
    const third = node("node-3");

    useUiStore.getState().openInActiveGroup(first);
    useUiStore.getState().openInActiveGroup(second);
    useUiStore.getState().openInActiveGroup(third);
    useUiStore.getState().updateGroupsNode({ ...first, name: "renamed.md", path: "/renamed.md" });
    useUiStore.getState().clearGroupsWithNode(second.id);

    const group = useUiStore.getState().editorGroups[0];
    expect(group.back).toEqual([
      expect.objectContaining({ nodeId: first.id, nameSnapshot: "renamed.md" })
    ]);
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

  it("keeps expanded folders scoped to the active space", () => {
    useUiStore.getState().setActiveSpaceId("space-a");
    useUiStore.getState().addExpanded(["space-a-root", "space-a-folder"]);

    useUiStore.getState().setActiveSpaceId("space-b");
    expect([...useUiStore.getState().expandedFolderIds]).toEqual([]);

    useUiStore.getState().addExpanded(["space-b-root"]);
    useUiStore.getState().setActiveSpaceId("space-a");
    expect(useUiStore.getState().expandedFolderIds).toEqual(
      new Set(["space-a-root", "space-a-folder"])
    );

    useUiStore.getState().setActiveSpaceId("space-b");
    expect(useUiStore.getState().expandedFolderIds).toEqual(
      new Set(["space-b-root"])
    );
  });

  it("hydrates theme, last active space, workbench, and panels through an isolated store", () => {
    const first = node("node-1");
    const second = node("node-2");
    const persistence = fakePersistence({
      loadTheme: vi.fn((): "light" | "dark" => "dark"),
      loadLastActiveSpaceId: vi.fn(() => "space-1"),
      loadSpaceWorkbench: vi.fn((): EditorGroupState => ({
        editorGroups: [
          { id: 0, node: first, mode: "edit", back: [], forward: [] },
          { id: 1, node: second, mode: "preview", back: [], forward: [] }
        ],
        activeGroupIndex: 0,
        nextGroupId: 2
      })),
      loadPanelState: vi.fn(() => ({
        primarySidebarOpen: true,
        auxiliaryOpen: false
      }))
    });
    const unhydratedStore = createUiStore(persistence);

    expect(persistence.loadTheme).not.toHaveBeenCalled();
    expect(unhydratedStore.getState().activeSpaceId).toBeNull();
    const store = createHydratedUiStore(persistence);

    const state = store.getState();
    expect(state.theme).toBe("dark");
    expect(state.activeSpaceId).toBe("space-1");
    expect(state.activeGroupIndex).toBe(0);
    expect(state.editorGroups).toHaveLength(2);
    expect(state.editorGroups[0]).toMatchObject({ id: 0, node: first, mode: "edit" });
    expect(state.editorGroups[1]).toMatchObject({ id: 1, node: second, mode: "preview" });
    expect(state.nextGroupId).toBe(2);
    expect(state.primarySidebarOpen).toBe(true);
    expect(state.auxiliaryOpen).toBe(false);
    expect(persistence.loadSpaceWorkbench).toHaveBeenCalledWith("space-1", 0);
    expect(persistence.applyTheme).toHaveBeenCalledWith("dark");
  });

  it("bootstraps the production store before consumers read it", () => {
    window.localStorage.setItem("notegate.theme", "dark");
    window.localStorage.setItem("notegate.lastActiveSpaceId", "space-2");
    window.localStorage.setItem(WORKBENCH_PANEL_STATE_KEY, JSON.stringify({
      version: 1,
      primarySidebarOpen: false,
      auxiliaryOpen: true
    }));

    bootstrapUiStore();

    expect(useUiStore.getState()).toMatchObject({
      theme: "dark",
      activeSpaceId: "space-2",
      primarySidebarOpen: false,
      auxiliaryOpen: true
    });
    expect(document.documentElement.dataset.theme).toBe("dark");
  });

  it("keeps independently created stores isolated", () => {
    const firstStore = createUiStore(fakePersistence());
    const secondStore = createUiStore(fakePersistence());

    firstStore.getState().toggleTheme();
    firstStore.getState().addGroup();

    expect(firstStore.getState()).toMatchObject({
      theme: "dark",
      activeGroupIndex: 1
    });
    expect(secondStore.getState()).toMatchObject({
      theme: "light",
      activeGroupIndex: 0
    });
    expect(secondStore.getState().editorGroups).toHaveLength(1);
  });

  it("clamps resizable layout values", () => {
    useUiStore.getState().setPrimaryWidth(100);
    expect(useUiStore.getState().primaryWidth).toBe(WORKBENCH_LAYOUT.minPrimaryWidth);
    useUiStore.getState().setPrimaryWidth(900);
    expect(useUiStore.getState().primaryWidth).toBe(WORKBENCH_LAYOUT.maxPrimaryWidth);

    useUiStore.getState().setTreeRatio(0.05);
    expect(useUiStore.getState().treeRatio).toBe(WORKBENCH_LAYOUT.minTreeRatio);
    useUiStore.getState().setTreeRatio(0.95);
    expect(useUiStore.getState().treeRatio).toBe(WORKBENCH_LAYOUT.maxTreeRatio);
  });
});
