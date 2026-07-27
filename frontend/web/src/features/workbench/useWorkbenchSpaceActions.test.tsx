import { renderHook } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

import type { RestNode, Space } from "../../api/types";
import { useUiStore } from "../../stores/uiStore";
import { persistSpaceWorkbench, workbenchSpaceKey } from "../../stores/workbenchStorage";
import { makeRestNode, makeSpace } from "../../test/fixtures";
import type { AppDialog } from "./dialogs/dialogTypes";
import { useWorkbenchSpaceActions } from "./useWorkbenchSpaceActions";

vi.mock("./useWorkbenchQueries", () => ({
  useCreateSpaceMutation: vi.fn((onCreated: (space: Space) => void) => ({
    mutateAsync: vi.fn(async () => onCreated(space("created-space")))
  })),
  useDeleteSpaceMutation: vi.fn((onDeleted: (spaceId: string) => void) => ({
    mutateAsync: vi.fn(async (spaceId: string) => onDeleted(spaceId))
  })),
  useReorderSpacesMutation: vi.fn(() => ({ mutate: vi.fn() })),
  useUpdateSpaceMutation: vi.fn(() => ({ mutateAsync: vi.fn() }))
}));

function space(id: string, permission: Space["permission"] = "write"): Space {
  return makeSpace({
    id,
    name: id,
    permission,
    root_node_id: `${id}-root`
  });
}

function node(id: string, spaceId: string): RestNode {
  return makeRestNode({
    id,
    space_id: spaceId,
    parent_id: `${spaceId}-root`,
    name: `${id}.md`,
    path: `/${id}.md`,
  });
}

describe("useWorkbenchSpaceActions", () => {
  beforeEach(() => {
    window.localStorage.clear();
    useUiStore.setState(useUiStore.getInitialState(), true);
  });

  it("deletes an inactive space without clearing the active workbench", async () => {
    const activeSpace = space("space-a");
    const deletedSpace = space("space-b");
    let dialog: AppDialog | null = null;

    useUiStore.getState().setActiveSpaceId(activeSpace.id);
    useUiStore.getState().openInActiveGroup(node("active", activeSpace.id));
    persistSpaceWorkbench(deletedSpace.id, [{ id: 9, node: node("deleted", deletedSpace.id), mode: "preview", back: [], forward: [] }], 0);

    const { result } = renderHook(() =>
      useWorkbenchSpaceActions({
        activeSpace,
        canCreateSpace: true,
        setDialog: (value) => {
          dialog = typeof value === "function" ? value(dialog) : value;
        }
      })
    );

    result.current.confirmDeleteSpace(deletedSpace);
    await requireConfirmDialog(dialog).onConfirm();

    expect(useUiStore.getState().activeSpaceId).toBe(activeSpace.id);
    expect(useUiStore.getState().editorGroups[0].node?.id).toBe("active");
    expect(window.localStorage.getItem(workbenchSpaceKey(deletedSpace.id))).toBeNull();
  });

  it("clears the workbench after deleting the active space", async () => {
    const activeSpace = space("space-a");
    let dialog: AppDialog | null = null;

    useUiStore.getState().setActiveSpaceId(activeSpace.id);
    useUiStore.getState().openInActiveGroup(node("active", activeSpace.id));

    const { result } = renderHook(() =>
      useWorkbenchSpaceActions({
        activeSpace,
        canCreateSpace: true,
        setDialog: (value) => {
          dialog = typeof value === "function" ? value(dialog) : value;
        }
      })
    );

    result.current.confirmDeleteSpace(activeSpace);
    await requireConfirmDialog(dialog).onConfirm();

    expect(useUiStore.getState().activeSpaceId).toBeNull();
    expect(useUiStore.getState().editorGroups).toMatchObject([{ node: null, mode: "preview" }]);
    expect(window.localStorage.getItem(workbenchSpaceKey(activeSpace.id))).toBeNull();
  });

  it("uses the target space permission for inactive space management", async () => {
    const activeSpace = space("space-a", "read");
    const writableTarget = space("space-b", "write");
    const readonlyTarget = space("space-c", "read");
    let dialog: AppDialog | null = null;

    useUiStore.getState().setActiveSpaceId(activeSpace.id);

    const { result } = renderHook(() =>
      useWorkbenchSpaceActions({
        activeSpace,
        canCreateSpace: true,
        setDialog: (value) => {
          dialog = typeof value === "function" ? value(dialog) : value;
        }
      })
    );

    result.current.confirmDeleteSpace(writableTarget);
    expect(requireConfirmDialog(dialog).message).toContain(writableTarget.name);

    dialog = null;
    result.current.confirmDeleteSpace(readonlyTarget);
    expect(dialog).toBeNull();
  });
});

function requireConfirmDialog(dialog: AppDialog | null): Extract<AppDialog, { kind: "confirm" }> {
  if (!dialog || dialog.kind !== "confirm") throw new Error("Expected confirm dialog");
  return dialog;
}
