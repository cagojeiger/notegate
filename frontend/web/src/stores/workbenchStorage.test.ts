import { describe, expect, it, vi } from "vitest";

import type { RestNode } from "../api/types";
import { makeRestNode } from "../test/fixtures";
import { MAX_WORKBENCH_SNAPSHOTS, WORKBENCH_INDEX_KEY, WORKBENCH_PANEL_STATE_KEY, clearPersistedSpaceWorkbench, clearPersistedWorkbenches, persistSpaceWorkbench, persistWorkbenchPanelState, restoreSpaceWorkbench, workbenchSpaceKey } from "./workbenchStorage";

describe("workbenchStorage", () => {
  it("restores compatible nodes and rejects nodes from the wrong space or malformed snapshots", () => {
    const first = node("node-1");
    const legacyFirst: Partial<RestNode> = { ...first };
    delete legacyFirst.search_enabled;
    delete legacyFirst.write_locked;
    delete legacyFirst.effective_write_locked;
    delete legacyFirst.write_lock_sources;
    const wrongSpaceNode = node("node-2", "wrong.md", "other-space");
    const malformedNode = { ...node("node-3"), created_by: undefined };
    saveSnapshot([
      { node: legacyFirst, mode: "edit" },
      { node: wrongSpaceNode, mode: "edit" },
      { node: malformedNode, mode: "edit" }
    ], 9);

    const state = restoreSpaceWorkbench("space-1", 0);

    expect(state.activeGroupIndex).toBe(2);
    expect(state.editorGroups).toMatchObject([
      {
        node: { ...first, search_enabled: true, write_locked: false, write_lock_sources: [] },
        mode: "edit"
      },
      { node: null, mode: "preview" },
      { node: null, mode: "preview" }
    ]);
  });

  it("derives effective write-lock state when restoring an older snapshot", () => {
    const first = node("node-1");
    const legacyLocked: Partial<RestNode> = {
      ...first,
      write_locked: false,
      write_lock_sources: [{ node_id: "folder-1", name: "Policies", path: "/Policies" }]
    };
    delete legacyLocked.effective_write_locked;
    saveSnapshot([{ node: legacyLocked, mode: "preview" }]);

    const state = restoreSpaceWorkbench("space-1", 0);

    expect(state.editorGroups[0]?.node?.effective_write_locked).toBe(true);
  });

  it("restores valid navigation history and drops entries from other spaces", () => {
    saveSnapshot([{
      node: node("node-3"),
      mode: "preview",
      back: [
        { spaceId: "space-1", nodeId: "node-1", nameSnapshot: "one.md", kind: "text" },
        { spaceId: "other-space", nodeId: "node-2", nameSnapshot: "two.md", kind: "text" }
      ],
      forward: [
        { spaceId: "space-1", nodeId: "node-4", nameSnapshot: "four.md", kind: "text" }
      ]
    }]);

    const group = restoreSpaceWorkbench("space-1", 0).editorGroups[0];

    expect(group.back.map((entry) => entry.nodeId)).toEqual(["node-1"]);
    expect(group.forward.map((entry) => entry.nodeId)).toEqual(["node-4"]);
  });

  it("falls back to an empty workbench when browser storage is unavailable", () => {
    vi.spyOn(Storage.prototype, "getItem").mockImplementation(() => {
      throw new DOMException("blocked", "SecurityError");
    });
    vi.spyOn(Storage.prototype, "removeItem").mockImplementation(() => {
      throw new DOMException("blocked", "SecurityError");
    });

    expect(restoreSpaceWorkbench("space-1", 0)).toMatchObject({
      activeGroupIndex: 0,
      nextGroupId: 1,
      editorGroups: [{ id: 0, node: null, mode: "preview", back: [], forward: [] }]
    });
  });

  it("clears one persisted space snapshot", () => {
    persistSpaceWorkbench("space-1", [{ id: 1, node: node("node-1"), mode: "preview", back: [], forward: [] }], 0);

    clearPersistedSpaceWorkbench("space-1");

    expect(window.localStorage.getItem(workbenchSpaceKey("space-1"))).toBeNull();
  });

  it("clears saved workspace snapshots and panel visibility together", () => {
    persistSpaceWorkbench("space-1", [{ id: 1, node: node("node-1"), mode: "preview", back: [], forward: [] }], 0);
    persistWorkbenchPanelState({ primarySidebarOpen: true, auxiliaryOpen: false });

    expect(window.localStorage.getItem(workbenchSpaceKey("space-1"))).not.toBeNull();
    expect(window.localStorage.getItem(WORKBENCH_PANEL_STATE_KEY)).not.toBeNull();

    clearPersistedWorkbenches();

    expect(window.localStorage.getItem(workbenchSpaceKey("space-1"))).toBeNull();
    expect(window.localStorage.getItem(WORKBENCH_PANEL_STATE_KEY)).toBeNull();
  });

  it("keeps only the most recent persisted space snapshots", () => {
    const now = vi.spyOn(Date, "now");

    for (let index = 0; index < MAX_WORKBENCH_SNAPSHOTS + 2; index += 1) {
      const spaceId = `space-${index}`;
      now.mockReturnValue(index);
      persistSpaceWorkbench(spaceId, [{ id: index, node: node(`node-${index}`, `${index}.md`, spaceId), mode: "preview", back: [], forward: [] }], 0);
    }

    const storedIndex = JSON.parse(window.localStorage.getItem(WORKBENCH_INDEX_KEY) ?? "{}") as { spaces: { spaceId: string }[] };
    expect(storedIndex.spaces).toHaveLength(MAX_WORKBENCH_SNAPSHOTS);
    expect(window.localStorage.getItem(workbenchSpaceKey("space-0"))).toBeNull();
    expect(window.localStorage.getItem(workbenchSpaceKey("space-1"))).toBeNull();
    expect(window.localStorage.getItem(workbenchSpaceKey("space-2"))).not.toBeNull();
  });
});

function saveSnapshot(groups: unknown[], activeGroupIndex = 0) {
  window.localStorage.setItem(workbenchSpaceKey("space-1"), JSON.stringify({
    version: 1,
    spaceId: "space-1",
    updatedAt: 1,
    groups,
    activeGroupIndex
  }));
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
