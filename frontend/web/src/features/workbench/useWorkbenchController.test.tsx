import { act, renderHook } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

import type { Me } from "../../api/types";
import { useUiStore } from "../../stores/uiStore";
import { useWorkbenchController } from "./useWorkbenchController";

vi.mock("@tanstack/react-query", () => ({
  useQuery: () => ({ data: undefined, error: null, isLoading: false })
}));

vi.mock("../../api/ApiProvider", () => ({ useApiClient: () => ({}) }));
vi.mock("../../shared/hooks/useMediaQuery", () => ({ useIsMobile: () => false }));
vi.mock("./useSpaceChangeSync", () => ({ useSpaceChangeSync: () => undefined }));
vi.mock("./useWorkbenchPersistence", () => ({ useWorkbenchPersistence: () => undefined }));
vi.mock("./useWorkbenchActions", () => ({
  useWorkbenchActions: () => ({ settingsOpen: false, dialog: null, actions: {} })
}));
vi.mock("./useWorkbenchQueries", () => ({
  useSpacesQuery: () => ({ data: { spaces: [] }, isLoading: false, isError: false, error: null })
}));

const me: Me = {
  account: { id: "user-1", kind: "user", display_name: "User" },
  user: { email: "user@example.com" },
  capabilities: { can_create_space: true, can_manage_agents: true }
};

describe("useWorkbenchController", () => {
  beforeEach(() => {
    useUiStore.setState(useUiStore.getInitialState(), true);
  });

  it("does not rerender the workbench for frame-local width updates", () => {
    let renderCount = 0;
    const { result } = renderHook(() => {
      renderCount += 1;
      return useWorkbenchController({ me, onSignOut: vi.fn() });
    });

    expect(renderCount).toBe(1);
    act(() => {
      useUiStore.getState().setAuxiliaryWidth(380);
      useUiStore.getState().setPrimaryWidth(360);
    });

    expect(renderCount).toBe(1);
    expect(result.current).not.toHaveProperty("auxiliaryWidth");
    expect(result.current).not.toHaveProperty("primaryWidth");
  });
});
