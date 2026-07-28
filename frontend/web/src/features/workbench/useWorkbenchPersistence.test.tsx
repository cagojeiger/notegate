import { act, renderHook } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { useUiStore } from "../../stores/uiStore";
import { makeRestNode, makeSpace } from "../../test/fixtures";
import {
  useWorkbenchPersistence,
  type WorkbenchPersistence
} from "./useWorkbenchPersistence";

describe("useWorkbenchPersistence", () => {
  beforeEach(() => {
    useUiStore.setState(useUiStore.getInitialState(), true);
  });

  it("saves the theme without requiring an active space", () => {
    const { persistence, saveTheme, saveLastActiveSpaceId, saveSpaceWorkbench } =
      createPersistence();

    renderHook(() => useWorkbenchPersistence("dark", null, null, persistence));

    expect(saveTheme).toHaveBeenCalledWith("dark");
    expect(saveLastActiveSpaceId).not.toHaveBeenCalled();
    expect(saveSpaceWorkbench).not.toHaveBeenCalled();
  });

  it("selects the resolved space without saving the previous workbench under it", () => {
    const activeSpace = makeSpace({
      id: "space-2",
      root_node_id: "root-2"
    });
    const { persistence, saveLastActiveSpaceId, saveSpaceWorkbench } =
      createPersistence();

    renderHook(() =>
      useWorkbenchPersistence("light", activeSpace, "space-1", persistence)
    );

    expect(useUiStore.getState().activeSpaceId).toBe(activeSpace.id);
    expect(useUiStore.getState().expandedFolderIds).toContain(
      activeSpace.root_node_id
    );
    expect(saveLastActiveSpaceId).toHaveBeenCalledWith(activeSpace.id);
    expect(saveSpaceWorkbench).not.toHaveBeenCalled();
  });

  it("saves the current editor groups once the selected space matches", () => {
    const activeSpace = makeSpace();
    const node = makeRestNode({ space_id: activeSpace.id });
    useUiStore.setState({
      activeSpaceId: activeSpace.id,
      activeGroupIndex: 0
    });
    const { persistence, saveSpaceWorkbench } = createPersistence();

    renderHook(() =>
      useWorkbenchPersistence(
        "light",
        activeSpace,
        activeSpace.id,
        persistence
      )
    );
    saveSpaceWorkbench.mockClear();

    act(() => useUiStore.getState().openInActiveGroup(node));

    const editorGroups = useUiStore.getState().editorGroups;

    expect(saveSpaceWorkbench).toHaveBeenCalledWith(
      activeSpace.id,
      editorGroups,
      0
    );
    expect(saveSpaceWorkbench).toHaveBeenCalledOnce();
  });
});

function createPersistence() {
  const saveTheme = vi.fn();
  const saveLastActiveSpaceId = vi.fn();
  const saveSpaceWorkbench = vi.fn();
  const persistence = {
    saveTheme,
    saveLastActiveSpaceId,
    saveSpaceWorkbench
  } satisfies WorkbenchPersistence;
  return {
    persistence,
    saveTheme,
    saveLastActiveSpaceId,
    saveSpaceWorkbench
  };
}
