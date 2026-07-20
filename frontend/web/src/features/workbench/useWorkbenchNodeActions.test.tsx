import { act, renderHook } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { ApiError } from "../../api/errors";
import { resolveNodePath } from "../../api/nodes";
import type { RestNode, Space } from "../../api/types";
import { useUiStore } from "../../stores/uiStore";
import { useWorkbenchNodeActions } from "./useWorkbenchNodeActions";

const mocks = vi.hoisted(() => ({
  revealNode: vi.fn(),
  startUpload: vi.fn()
}));

vi.mock("../../api/ApiProvider", () => ({
  useApiClient: () => ({})
}));

vi.mock("../../api/nodes", () => ({
  resolveNodePath: vi.fn()
}));

vi.mock("../uploads/UploadProvider", () => ({
  useUploadManager: () => ({ startUpload: mocks.startUpload })
}));

vi.mock("./useWorkbenchQueries", () => {
  const mutation = () => ({ mutate: vi.fn(), mutateAsync: vi.fn() });
  return {
    useCreateNodeMutation: mutation,
    useDeleteNodeMutation: mutation,
    useMoveNodeMutation: mutation,
    useReplaceMetadataMutation: mutation,
    useUpdateNodeMutation: mutation,
    useRevealNode: () => mocks.revealNode
  };
});

describe("useWorkbenchNodeActions", () => {
  beforeEach(() => {
    window.localStorage.clear();
    useUiStore.setState(useUiStore.getInitialState(), true);
    vi.mocked(resolveNodePath).mockReset();
    mocks.revealNode.mockReset();
    mocks.startUpload.mockReset();
  });

  it("opens a resolved markdown link through the active editor group and reveals its ancestors", async () => {
    const activeSpace = space("space-1");
    const sourceNode = node("source", activeSpace.id, "/index.md");
    const folder = node("folder", activeSpace.id, "/Policies", "folder");
    const targetNode = node("target", activeSpace.id, "/Policies/Access Control Policy.md");
    const groupId = openSourceGroup(activeSpace, sourceNode);
    vi.mocked(resolveNodePath).mockResolvedValue(targetNode);
    mocks.revealNode.mockResolvedValue({ ancestors: [folder], target: targetNode });

    const { result } = renderHook(() =>
      useWorkbenchNodeActions({
        activeSpace,
        activeNode: sourceNode,
        canWriteActiveSpace: true,
        setDialog: vi.fn()
      })
    );

    await act(async () => {
      await result.current.openMarkdownLink(groupId, sourceNode, targetNode.path);
    });

    expect(resolveNodePath).toHaveBeenCalledWith(expect.anything(), activeSpace.id, targetNode.path);
    expect(useUiStore.getState().editorGroups[0].node?.id).toBe(targetNode.id);
    expect(useUiStore.getState().expandedFolderIds.has(folder.id)).toBe(true);
  });

  it("keeps the current editor state when markdown link resolution fails", async () => {
    const activeSpace = space("space-1");
    const sourceNode = node("source", activeSpace.id, "/index.md");
    const groupId = openSourceGroup(activeSpace, sourceNode);
    vi.mocked(resolveNodePath).mockRejectedValue(new ApiError("not found", 404));

    const { result } = renderHook(() =>
      useWorkbenchNodeActions({
        activeSpace,
        activeNode: sourceNode,
        canWriteActiveSpace: true,
        setDialog: vi.fn()
      })
    );

    await act(async () => {
      await result.current.openMarkdownLink(groupId, sourceNode, "/missing.md");
    });

    expect(useUiStore.getState().editorGroups[0].node?.id).toBe(sourceNode.id);
    expect(useUiStore.getState().toast).toBe("Linked node not found");
  });

  it("opens markdown links even when tree reveal fails", async () => {
    const activeSpace = space("space-1");
    const sourceNode = node("source", activeSpace.id, "/index.md");
    const targetNode = node("target", activeSpace.id, "/Policies/Access Control Policy.md");
    const groupId = openSourceGroup(activeSpace, sourceNode);
    vi.mocked(resolveNodePath).mockResolvedValue(targetNode);
    mocks.revealNode.mockRejectedValue(new Error("reveal failed"));

    const { result } = renderHook(() =>
      useWorkbenchNodeActions({
        activeSpace,
        activeNode: sourceNode,
        canWriteActiveSpace: true,
        setDialog: vi.fn()
      })
    );

    await act(async () => {
      await result.current.openMarkdownLink(groupId, sourceNode, targetNode.path);
    });

    expect(useUiStore.getState().editorGroups[0].node?.id).toBe(targetNode.id);
    expect(useUiStore.getState().toast).toBe("Opened node, but could not reveal it in the tree");
  });

  it("opens a resolved markdown link in the original source group when focus changes before resolution", async () => {
    const activeSpace = space("space-1");
    const sourceNode = node("source", activeSpace.id, "/index.md");
    const otherNode = node("other", activeSpace.id, "/other.md");
    const targetNode = node("target", activeSpace.id, "/target.md");
    const groupId = openSourceGroup(activeSpace, sourceNode);
    useUiStore.getState().openInNewGroup(otherNode);
    useUiStore.getState().focusGroup(0);
    const pending = deferred<RestNode>();
    vi.mocked(resolveNodePath).mockReturnValue(pending.promise);
    mocks.revealNode.mockResolvedValue({ ancestors: [], target: targetNode });

    const { result } = renderHook(() =>
      useWorkbenchNodeActions({
        activeSpace,
        activeNode: sourceNode,
        canWriteActiveSpace: true,
        setDialog: vi.fn()
      })
    );

    const openPromise = result.current.openMarkdownLink(groupId, sourceNode, targetNode.path);
    act(() => {
      useUiStore.getState().focusGroup(1);
      pending.resolve(targetNode);
    });
    await act(async () => {
      await openPromise;
    });

    const state = useUiStore.getState();
    expect(state.editorGroups[0].node?.id).toBe(targetNode.id);
    expect(state.editorGroups[1].node?.id).toBe(otherNode.id);
    expect(state.activeGroupIndex).toBe(1);
  });

  it("does not open a stale markdown link when the source group changed before resolution", async () => {
    const activeSpace = space("space-1");
    const sourceNode = node("source", activeSpace.id, "/index.md");
    const replacementNode = node("replacement", activeSpace.id, "/replacement.md");
    const targetNode = node("target", activeSpace.id, "/target.md");
    const groupId = openSourceGroup(activeSpace, sourceNode);
    const pending = deferred<RestNode>();
    vi.mocked(resolveNodePath).mockReturnValue(pending.promise);

    const { result } = renderHook(() =>
      useWorkbenchNodeActions({
        activeSpace,
        activeNode: sourceNode,
        canWriteActiveSpace: true,
        setDialog: vi.fn()
      })
    );

    const openPromise = result.current.openMarkdownLink(groupId, sourceNode, targetNode.path);
    act(() => {
      useUiStore.getState().openInGroup(groupId, replacementNode);
      pending.resolve(targetNode);
    });
    await act(async () => {
      await openPromise;
    });

    expect(useUiStore.getState().editorGroups[0].node?.id).toBe(replacementNode.id);
    expect(mocks.revealNode).not.toHaveBeenCalled();
  });

  it("does not open resolved markdown links from a different space", async () => {
    const activeSpace = space("space-1");
    const sourceNode = node("source", activeSpace.id, "/index.md");
    const targetNode = node("target", "space-2", "/target.md");
    const groupId = openSourceGroup(activeSpace, sourceNode);
    vi.mocked(resolveNodePath).mockResolvedValue(targetNode);

    const { result } = renderHook(() =>
      useWorkbenchNodeActions({
        activeSpace,
        activeNode: sourceNode,
        canWriteActiveSpace: true,
        setDialog: vi.fn()
      })
    );

    await act(async () => {
      await result.current.openMarkdownLink(groupId, sourceNode, targetNode.path);
    });

    expect(useUiStore.getState().editorGroups[0].node?.id).toBe(sourceNode.id);
    expect(useUiStore.getState().toast).toBe("Could not open linked node");
  });

  it("opens regular nodes even when tree reveal fails", async () => {
    const activeSpace = space("space-1");
    const targetNode = node("target", activeSpace.id, "/target.md");
    mocks.revealNode.mockRejectedValue(new Error("reveal failed"));

    const { result } = renderHook(() =>
      useWorkbenchNodeActions({
        activeSpace,
        activeNode: targetNode,
        canWriteActiveSpace: true,
        setDialog: vi.fn()
      })
    );

    await act(async () => {
      await result.current.openNode(targetNode);
    });

    expect(useUiStore.getState().editorGroups[0].node?.id).toBe(targetNode.id);
    expect(useUiStore.getState().toast).toBe("Opened node, but could not reveal it in the tree");
  });

  it("queues a selected file with the current space snapshot", async () => {
    const activeSpace = space("space-1");
    const destinationFolder = node("reports", activeSpace.id, "/Reports", "folder");
    const setDialog = vi.fn();
    const file = new File(["data"], "source.bin", { type: "application/octet-stream" });
    const { result } = renderHook(() =>
      useWorkbenchNodeActions({
        activeSpace,
        activeNode: destinationFolder,
        canWriteActiveSpace: true,
        setDialog
      })
    );

    act(() => { result.current.handleFileSelected(file); });
    const dialog = setDialog.mock.calls[0]?.[0];
    expect(dialog?.kind).toBe("prompt");
    if (!dialog || dialog.kind !== "prompt") throw new Error("upload prompt was not opened");

    await act(async () => { await dialog.onSubmit("archive.bin"); });

    expect(mocks.startUpload).toHaveBeenCalledWith({
      spaceId: activeSpace.id,
      spaceName: activeSpace.name,
      destinationPath: destinationFolder.path,
      parentNodeId: destinationFolder.id,
      name: "archive.bin",
      file
    });
  });
});

function openSourceGroup(activeSpace: Space, sourceNode: RestNode): number {
  useUiStore.getState().setActiveSpaceId(activeSpace.id);
  useUiStore.getState().openInActiveGroup(sourceNode);
  return useUiStore.getState().editorGroups[0].id;
}

function deferred<T>() {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((done) => {
    resolve = done;
  });
  return { promise, resolve };
}

function space(id: string): Space {
  return {
    id,
    name: id,
    sort_order: 0,
    permission: "write",
    root_node_id: `${id}-root`,
    created_at: "2026-06-13T00:00:00Z",
    updated_at: "2026-06-13T00:00:00Z"
  };
}

function node(id: string, spaceId: string, path: string, kind: RestNode["kind"] = "text"): RestNode {
  return {
    id,
    space_id: spaceId,
    parent_id: `${spaceId}-root`,
    name: path.split("/").pop() ?? id,
    kind,
    path,
    sort_order: 0,
    metadata: {},
    has_children: false,
    created_by: { id: "user-1", kind: "user", display_name: "User" },
    updated_by: { id: "user-1", kind: "user", display_name: "User" },
    created_at: "2026-06-13T00:00:00Z",
    updated_at: "2026-06-13T00:00:00Z"
  };
}
