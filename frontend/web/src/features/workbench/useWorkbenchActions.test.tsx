import { act, renderHook } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

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

describe("useWorkbenchActions", () => {
  beforeEach(() => {
    mocks.logout.mockReset().mockResolvedValue(undefined);
  });

  it("notifies the auth boundary after logout", async () => {
    const { result, onSignOut } = renderActions();

    await act(async () => {
      await result.current.actions.handleSignOut();
    });

    expect(onSignOut).toHaveBeenCalledTimes(1);
  });

  it("still notifies the auth boundary when server logout fails", async () => {
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
    expect(onSignOut).toHaveBeenCalledTimes(1);
  });
});
