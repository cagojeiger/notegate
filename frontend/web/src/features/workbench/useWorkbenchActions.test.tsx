import { act, renderHook } from "@testing-library/react";
import type { ComponentType, ReactNode } from "react";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { ApiProvider } from "../../api/ApiProvider";
import { persistWorkbenchPanelState, persistSpaceWorkbench, workbenchSpaceKey } from "../../stores/workbenchStorage";
import { useUiStore } from "../../stores/uiStore";
import { makeRestNode } from "../../test/fixtures";
import { useWorkbenchActions } from "./useWorkbenchActions";

const mocks = vi.hoisted(() => ({
  logout: vi.fn()
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

function renderActions(onSignOut = vi.fn(), wrapper?: ComponentType<{ children: ReactNode }>) {
  return {
    onSignOut,
    ...renderHook(() => useWorkbenchActions({
      activeSpace: null,
      activeNode: null,
      inspectedNode: null,
      canCreateSpace: false,
      canWriteActiveSpace: false,
      canManageActiveSpace: false,
      onSignOut
    }), { wrapper })
  };
}

function persistBrowserWorkspace() {
  persistSpaceWorkbench("space-1", [{ id: 1, node: null, mode: "preview", back: [], forward: [] }], 0);
  persistWorkbenchPanelState({ primarySidebarOpen: true, auxiliaryOpen: false });
}

describe("useWorkbenchActions", () => {
  beforeEach(() => {
    mocks.logout.mockReset().mockResolvedValue(undefined);
    useUiStore.setState(useUiStore.getInitialState(), true);
  });

  it("clears browser workspace metadata after logout", async () => {
    persistBrowserWorkspace();
    const { result, onSignOut } = renderActions();

    await act(async () => {
      await result.current.actions.handleSignOut();
    });

    expect(window.localStorage.getItem(workbenchSpaceKey("space-1"))).toBeNull();
    expect(window.localStorage.length).toBe(0);
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
    expect(onSignOut).toHaveBeenCalledTimes(1);
  });

  for (const outcome of ["success", "error"] as const) {
    it(`preserves the new owner's workbench when an earlier logout finishes with ${outcome}`, async () => {
      let owner = "a";
      let finish!: () => void;
      let fail!: (error: Error) => void;
      mocks.logout.mockImplementationOnce(() => new Promise<void>((resolve, reject) => {
        finish = resolve;
        fail = reject;
      }));
      const wrapper = ({ children }: { children: ReactNode }) => <ApiProvider authCacheKey={owner}>{children}</ApiProvider>;
      const onSignOut = vi.fn(() => useUiStore.getState().resetSessionWorkspace());
      const { result, rerender } = renderActions(onSignOut, wrapper);
      let settled!: Promise<unknown>;
      act(() => {
        settled = result.current.actions.handleSignOut().catch((error: unknown) => error);
      });
      expect(mocks.logout).toHaveBeenCalledTimes(1);

      owner = "b";
      rerender();
      act(() => {
        useUiStore.getState().resetSessionWorkspace();
        useUiStore.getState().setActiveSpaceId("space-b");
        useUiStore.getState().openInActiveGroup(makeRestNode({ id: "note-b", space_id: "space-b" }));
      });
      const current = useUiStore.getState();
      persistSpaceWorkbench("space-b", current.editorGroups, current.activeGroupIndex);
      persistWorkbenchPanelState({ primarySidebarOpen: false, auxiliaryOpen: true });
      const saved = window.localStorage.getItem(workbenchSpaceKey("space-b"));
      const storageSize = window.localStorage.length;
      const error = new Error("old logout failed");

      await act(async () => {
        if (outcome === "success") finish();
        else fail(error);
        expect(await settled).toBe(outcome === "success" ? undefined : error);
      });

      expect(onSignOut).not.toHaveBeenCalled();
      expect(useUiStore.getState().activeSpaceId).toBe("space-b");
      expect(useUiStore.getState().editorGroups).toBe(current.editorGroups);
      expect(window.localStorage.getItem(workbenchSpaceKey("space-b"))).toBe(saved);
      expect(window.localStorage.length).toBe(storageSize);
    });
  }
});
