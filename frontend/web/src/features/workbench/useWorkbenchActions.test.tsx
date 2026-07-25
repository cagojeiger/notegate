import { act, renderHook } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { writeDevApiKey } from "../../auth/session";
import { persistWorkbenchPanelState, persistSpaceWorkbench, workbenchSpaceKey } from "../../stores/workbenchStorage";
import { useWorkbenchActions } from "./useWorkbenchActions";

const mocks = vi.hoisted(() => ({
  logout: vi.fn()
}));

vi.mock("../../shared/hooks/usePointerDrag", () => ({
  usePointerDrag: () => vi.fn()
}));

vi.mock("./useWorkbenchNodeActions", () => ({
  useWorkbenchNodeActions: () => ({})
}));

vi.mock("./useWorkbenchSpaceActions", () => ({
  useWorkbenchSpaceActions: () => ({})
}));

vi.mock("./useWorkbenchQueries", () => ({
  useLogout: () => mocks.logout
}));

function renderActions(onSignOut = vi.fn()) {
  return {
    onSignOut,
    ...renderHook(() => useWorkbenchActions({
      activeSpace: null,
      activeNode: null,
      canCreateSpace: false,
      canWriteActiveSpace: false,
      primaryWidth: 280,
      onSignOut
    }))
  };
}

function persistBrowserWorkspace() {
  persistSpaceWorkbench("space-1", [{ id: 1, node: null, mode: "preview" }], 0);
  persistWorkbenchPanelState({ primarySidebarOpen: true, auxiliaryOpen: false });
  writeDevApiKey("dev-key");
}

describe("useWorkbenchActions", () => {
  beforeEach(() => {
    mocks.logout.mockReset().mockResolvedValue(undefined);
  });

  it("clears browser workspace metadata after logout", async () => {
    persistBrowserWorkspace();
    const { result, onSignOut } = renderActions();

    await act(async () => {
      await result.current.actions.handleSignOut();
    });

    expect(window.localStorage.getItem(workbenchSpaceKey("space-1"))).toBeNull();
    expect(window.localStorage.length).toBe(0);
    expect(window.sessionStorage.getItem("notegate.devApiKey")).toBeNull();
    expect(onSignOut).toHaveBeenCalledTimes(1);
  });

  it("still clears browser workspace metadata when server logout fails", async () => {
    persistBrowserWorkspace();
    mocks.logout.mockRejectedValue(new Error("logout failed"));
    const { result, onSignOut } = renderActions();
    let error: unknown;

    await act(async () => {
      try {
        await result.current.actions.handleSignOut();
      } catch (caught) {
        error = caught;
      }
    });

    expect(error).toEqual(new Error("logout failed"));
    expect(window.localStorage.length).toBe(0);
    expect(window.sessionStorage.getItem("notegate.devApiKey")).toBeNull();
    expect(onSignOut).toHaveBeenCalledTimes(1);
  });
});
