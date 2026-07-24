import { beforeEach, describe, expect, it } from "vitest";

import type { RestNode } from "../api/types";
import { useUiStore } from "../stores/uiStore";
import {
  LAST_ACTIVE_SPACE_KEY,
  WORKBENCH_PANEL_STATE_KEY,
  persistLastActiveSpace,
  persistSpaceWorkbench,
  persistWorkbenchPanelState,
  workbenchSpaceKey
} from "../stores/workbenchStorage";
import { clearAuthenticatedClientState, resetWorkbenchClientState } from "./clientSession";
import { writeDevApiKey } from "../auth/session";

describe("clearAuthenticatedClientState", () => {
  beforeEach(() => {
    useUiStore.setState(useUiStore.getInitialState(), true);
  });

  it("clears authenticated workbench memory and persistence while preserving the theme", () => {
    const openedNode = node();
    useUiStore.setState({
      theme: "dark",
      activeSpaceId: "space-1",
      editorGroups: [{ id: 7, nodeRef: { nodeId: openedNode.id, spaceId: openedNode.space_id }, mode: "edit" }],
      activeGroupIndex: 0,
      expandedFolderIds: new Set(["folder-1"]),
      primarySidebarOpen: false,
      auxiliaryOpen: false,
      mobileTreeOpen: true,
      toast: "Private document saved",
      saveState: "saved"
    });
    persistSpaceWorkbench("space-1", [{ id: 7, nodeRef: { nodeId: openedNode.id, spaceId: openedNode.space_id }, mode: "edit" }], 0);
    persistWorkbenchPanelState({ primarySidebarOpen: false, auxiliaryOpen: false });
    persistLastActiveSpace("space-1");
    writeDevApiKey("dev-key");

    clearAuthenticatedClientState();

    const state = useUiStore.getState();
    expect(state.theme).toBe("dark");
    expect(state.activeSpaceId).toBeNull();
    expect(state.editorGroups).toMatchObject([{ nodeRef: null, mode: "preview" }]);
    expect(state.expandedFolderIds.size).toBe(0);
    expect(state.primarySidebarOpen).toBe(true);
    expect(state.auxiliaryOpen).toBe(true);
    expect(state.mobileTreeOpen).toBe(false);
    expect(state.toast).toBeNull();
    expect(state.saveState).toBe("idle");
    expect(window.localStorage.getItem(workbenchSpaceKey("space-1"))).toBeNull();
    expect(window.localStorage.getItem(WORKBENCH_PANEL_STATE_KEY)).toBeNull();
    expect(window.localStorage.getItem(LAST_ACTIVE_SPACE_KEY)).toBeNull();
    expect(window.sessionStorage.getItem("notegate.devApiKey")).toBeNull();
  });

  it("resets workbench state without removing a newly authenticated API key", () => {
    useUiStore.setState({ activeSpaceId: "space-1" });
    persistLastActiveSpace("space-1");
    writeDevApiKey("new-dev-key");

    resetWorkbenchClientState();

    expect(useUiStore.getState().activeSpaceId).toBeNull();
    expect(window.localStorage.getItem(LAST_ACTIVE_SPACE_KEY)).toBeNull();
    expect(window.sessionStorage.getItem("notegate.devApiKey")).toBe("new-dev-key");
  });
});

function node(): RestNode {
  return {
    id: "node-1",
    space_id: "space-1",
    parent_id: "space-1-root",
    name: "private.md",
    kind: "text",
    path: "/private.md",
    sort_order: 0,
    metadata: {},
    has_children: false,
    created_by: { id: "user-1", kind: "user", display_name: "User" },
    updated_by: { id: "user-1", kind: "user", display_name: "User" },
    created_at: "2026-07-24T00:00:00Z",
    updated_at: "2026-07-24T00:00:00Z"
  };
}
