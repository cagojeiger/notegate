import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { act, renderHook } from "@testing-library/react";
import type { ReactNode } from "react";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { ApiError } from "../../api/errors";
import { getNode, resolveNodePath } from "../../api/nodes";
import type { RestNode, Space } from "../../api/types";
import { useUiStore } from "../../stores/uiStore";
import { makeRestNode, makeSpace } from "../../test/fixtures";
import { useWorkbenchNodeActions } from "./useWorkbenchNodeActions";

const mocks = vi.hoisted(() => ({
  createNode: vi.fn(),
  deleteNode: vi.fn(),
  downloadFile: vi.fn(),
  moveNode: vi.fn(),
  replaceMetadata: vi.fn(),
  revealNode: vi.fn(),
  startUpload: vi.fn(),
  updateNode: vi.fn(),
  updateNodeSearchPolicy: vi.fn(),
  updateNodeWriteLock: vi.fn(),
  updateTextEncryption: vi.fn()
}));

vi.mock("../../api/ApiProvider", () => ({
  useApiClient: () => ({})
}));

vi.mock("../../api/nodes", () => ({
  getNode: vi.fn(),
  resolveNodePath: vi.fn()
}));

vi.mock("../../api/files", () => ({
  downloadFile: mocks.downloadFile
}));

vi.mock("../uploads/UploadProvider", () => ({
  useUploadActions: () => ({ startUpload: mocks.startUpload })
}));

vi.mock("./useWorkbenchQueries", () => {
  return {
    useCreateNodeMutation: () => ({ mutateAsync: mocks.createNode }),
    useDeleteNodeMutation: () => ({ mutateAsync: mocks.deleteNode }),
    useMoveNodeMutation: () => ({ mutate: mocks.moveNode, mutateAsync: mocks.moveNode }),
    useReplaceMetadataMutation: () => ({ mutateAsync: mocks.replaceMetadata }),
    useUpdateNodeMutation: () => ({ mutateAsync: mocks.updateNode }),
    useUpdateNodeSearchPolicyMutation: () => ({
      mutate: mocks.updateNodeSearchPolicy,
      isPending: false
    }),
    useUpdateNodeWriteLockMutation: () => ({
      mutate: mocks.updateNodeWriteLock,
      isPending: false
    }),
    useUpdateTextEncryptionMutation: () => ({
      mutate: mocks.updateTextEncryption,
      isPending: false
    }),
    useRevealNode: () => mocks.revealNode
  };
});

describe("useWorkbenchNodeActions", () => {
  beforeEach(() => {
    window.localStorage.clear();
    useUiStore.setState(useUiStore.getInitialState(), true);
    vi.mocked(resolveNodePath).mockReset();
    vi.mocked(getNode).mockReset();
    mocks.downloadFile.mockReset().mockResolvedValue(undefined);
    mocks.createNode.mockReset();
    mocks.deleteNode.mockReset();
    mocks.moveNode.mockReset();
    mocks.replaceMetadata.mockReset();
    mocks.revealNode.mockReset();
    mocks.startUpload.mockReset();
    mocks.updateNode.mockReset();
    mocks.updateNodeSearchPolicy.mockReset();
    mocks.updateNodeWriteLock.mockReset();
    mocks.updateTextEncryption.mockReset();
  });

  it("opens a resolved markdown link through the active editor group and reveals its ancestors", async () => {
    const activeSpace = space("space-1");
    const sourceNode = node("source", activeSpace.id, "/index.md");
    const folder = node("folder", activeSpace.id, "/Policies", "folder");
    const targetNode = node("target", activeSpace.id, "/Policies/Access Control Policy.md");
    const groupId = openSourceGroup(activeSpace, sourceNode);
    vi.mocked(resolveNodePath).mockResolvedValue(targetNode);
    mocks.revealNode.mockResolvedValue({ ancestors: [folder], target: targetNode });

    const { result } = renderNodeActions({
        activeSpace,
        activeNode: sourceNode,
        canWriteActiveSpace: true,
        setDialog: vi.fn()
      });

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

    const { result } = renderNodeActions({
        activeSpace,
        activeNode: sourceNode,
        canWriteActiveSpace: true,
        setDialog: vi.fn()
      });

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

    const { result } = renderNodeActions({
        activeSpace,
        activeNode: sourceNode,
        canWriteActiveSpace: true,
        setDialog: vi.fn()
      });

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

    const { result } = renderNodeActions({
        activeSpace,
        activeNode: sourceNode,
        canWriteActiveSpace: true,
        setDialog: vi.fn()
      });

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

    const { result } = renderNodeActions({
        activeSpace,
        activeNode: sourceNode,
        canWriteActiveSpace: true,
        setDialog: vi.fn()
      });

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

    const { result } = renderNodeActions({
        activeSpace,
        activeNode: sourceNode,
        canWriteActiveSpace: true,
        setDialog: vi.fn()
      });

    await act(async () => {
      await result.current.openMarkdownLink(groupId, sourceNode, targetNode.path);
    });

    expect(useUiStore.getState().editorGroups[0].node?.id).toBe(sourceNode.id);
    expect(useUiStore.getState().toast).toBe("Could not open linked node");
  });

  it("opens regular nodes even when tree reveal fails", async () => {
    const activeSpace = space("space-1");
    const targetNode = node("target", activeSpace.id, "/target.md");
    vi.mocked(getNode).mockResolvedValue(targetNode);
    mocks.revealNode.mockRejectedValue(new Error("reveal failed"));

    const { result } = renderNodeActions({
        activeSpace,
        activeNode: targetNode,
        canWriteActiveSpace: true,
        setDialog: vi.fn()
      });

    await act(async () => {
      await result.current.openNode(targetNode);
    });

    expect(useUiStore.getState().editorGroups[0].node?.id).toBe(targetNode.id);
    expect(useUiStore.getState().toast).toBe("Opened node, but could not reveal it in the tree");
  });

  it("reuses the canonical node query when the same summary is opened again", async () => {
    const activeSpace = space("space-1");
    const targetNode = node("target", activeSpace.id, "/target.md");
    vi.mocked(getNode).mockResolvedValue(targetNode);
    mocks.revealNode.mockResolvedValue({ ancestors: [], target: targetNode });
    const { result } = renderNodeActions({
      activeSpace,
      activeNode: null,
      canWriteActiveSpace: true,
      setDialog: vi.fn()
    });

    await act(async () => {
      await result.current.openNode(targetNode);
      await result.current.openNode(targetNode);
    });

    expect(getNode).toHaveBeenCalledOnce();
  });

  it("navigates backward using a fresh canonical node", async () => {
    const activeSpace = space("space-1");
    const first = node("first", activeSpace.id, "/first.md");
    const second = node("second", activeSpace.id, "/second.md");
    const third = node("third", activeSpace.id, "/third.md");
    const groupId = openSourceGroup(activeSpace, first);
    useUiStore.getState().openInGroup(groupId, second);
    useUiStore.getState().openInGroup(groupId, third);
    vi.mocked(getNode).mockResolvedValue(second);
    mocks.revealNode.mockResolvedValue({ ancestors: [], target: second });
    const { result } = renderNodeActions({
      activeSpace,
      activeNode: third,
      canWriteActiveSpace: true,
      setDialog: vi.fn()
    });

    await act(async () => {
      await result.current.navigateEditorGroup(groupId, "back");
    });

    const group = useUiStore.getState().editorGroups[0];
    expect(getNode).toHaveBeenCalledWith(expect.anything(), activeSpace.id, second.id);
    expect(group.node?.id).toBe(second.id);
    expect(group.back.map((entry) => entry.nodeId)).toEqual([first.id]);
    expect(group.forward.map((entry) => entry.nodeId)).toEqual([third.id]);
  });

  it("skips deleted navigation entries and continues in the same direction", async () => {
    const activeSpace = space("space-1");
    const first = node("first", activeSpace.id, "/first.md");
    const deleted = node("deleted", activeSpace.id, "/deleted.md");
    const current = node("current", activeSpace.id, "/current.md");
    const groupId = openSourceGroup(activeSpace, first);
    useUiStore.getState().openInGroup(groupId, deleted);
    useUiStore.getState().openInGroup(groupId, current);
    vi.mocked(getNode)
      .mockRejectedValueOnce(new ApiError("not found", 404))
      .mockResolvedValueOnce(first);
    mocks.revealNode.mockResolvedValue({ ancestors: [], target: first });
    const { result } = renderNodeActions({
      activeSpace,
      activeNode: current,
      canWriteActiveSpace: true,
      setDialog: vi.fn()
    });

    await act(async () => {
      await result.current.navigateEditorGroup(groupId, "back");
    });

    const group = useUiStore.getState().editorGroups[0];
    expect(group.node?.id).toBe(first.id);
    expect(group.back).toEqual([]);
    expect(group.forward.map((entry) => entry.nodeId)).toEqual([current.id]);
    expect(getNode).toHaveBeenCalledTimes(2);
  });

  it("keeps navigation history when the target cannot be verified", async () => {
    const activeSpace = space("space-1");
    const first = node("first", activeSpace.id, "/first.md");
    const current = node("current", activeSpace.id, "/current.md");
    const groupId = openSourceGroup(activeSpace, first);
    useUiStore.getState().openInGroup(groupId, current);
    vi.mocked(getNode).mockRejectedValue(new ApiError("unavailable", 503));
    const { result } = renderNodeActions({
      activeSpace,
      activeNode: current,
      canWriteActiveSpace: true,
      setDialog: vi.fn()
    });

    await act(async () => {
      await result.current.navigateEditorGroup(groupId, "back");
    });

    const group = useUiStore.getState().editorGroups[0];
    expect(group.node?.id).toBe(current.id);
    expect(group.back.map((entry) => entry.nodeId)).toEqual([first.id]);
    expect(useUiStore.getState().toast).toBe("Could not navigate to node");
  });

  it("does not apply a navigation result after the group changes", async () => {
    const activeSpace = space("space-1");
    const first = node("first", activeSpace.id, "/first.md");
    const current = node("current", activeSpace.id, "/current.md");
    const replacement = node("replacement", activeSpace.id, "/replacement.md");
    const groupId = openSourceGroup(activeSpace, first);
    useUiStore.getState().openInGroup(groupId, current);
    const pending = deferred<RestNode>();
    vi.mocked(getNode).mockReturnValue(pending.promise);
    const { result } = renderNodeActions({
      activeSpace,
      activeNode: current,
      canWriteActiveSpace: true,
      setDialog: vi.fn()
    });

    const navigation = result.current.navigateEditorGroup(groupId, "back");
    act(() => {
      useUiStore.getState().openInGroup(groupId, replacement);
      pending.resolve(first);
    });
    await act(async () => {
      await navigation;
    });

    expect(useUiStore.getState().editorGroups[0].node?.id).toBe(replacement.id);
    expect(mocks.revealNode).not.toHaveBeenCalled();
  });

  it("queues a selected file with the current space snapshot", async () => {
    const activeSpace = space("space-1");
    const destinationFolder = node("reports", activeSpace.id, "/Reports", "folder");
    const setDialog = vi.fn();
    const file = new File(["data"], "source.bin", { type: "application/octet-stream" });
    const { result } = renderNodeActions({
        activeSpace,
        activeNode: destinationFolder,
        canWriteActiveSpace: true,
        setDialog
      });

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

  it("explains why header create and upload actions are blocked", () => {
    const activeSpace = space("space-1");
    const lockedFolder = {
      ...node("folder-1", activeSpace.id, "/Policies", "folder"),
      effective_write_locked: true
    };
    const file = new File(["data"], "source.bin");
    const { result } = renderNodeActions({
      activeSpace,
      activeNode: lockedFolder,
      canWriteActiveSpace: true,
      setDialog: vi.fn()
    });

    act(() => result.current.promptCreateNode("text"));
    expect(useUiStore.getState().toast).toBe(
      "Changes are blocked because the destination folder or an ancestor is write-locked"
    );

    act(() => {
      useUiStore.setState({ toast: null });
      result.current.handleFileSelected(file);
    });
    expect(useUiStore.getState().toast).toBe(
      "Changes are blocked because the destination folder or an ancestor is write-locked"
    );
    expect(mocks.startUpload).not.toHaveBeenCalled();
  });

  it("does not dispatch node mutations from any command path under an inherited lock", () => {
    const activeSpace = space("space-1");
    const lockedText = {
      ...node("locked", activeSpace.id, "/Policies/locked.md"),
      effective_write_locked: true,
      write_lock_sources: [
        { node_id: "folder-1", name: "Policies", path: "/Policies" }
      ]
    };
    const lockedFolder = {
      ...node("folder-1", activeSpace.id, "/Policies", "folder"),
      has_children: true,
      effective_write_locked: true
    };
    const unlockedFolder = {
      ...node("folder-2", activeSpace.id, "/Elsewhere", "folder"),
      has_children: true
    };
    const setDialog = vi.fn();
    const file = new File(["data"], "source.bin");
    const { result } = renderNodeActions({
      activeSpace,
      activeNode: lockedText,
      canWriteActiveSpace: true,
      setDialog
    });

    act(() => {
      result.current.promptCreateNode("text");
      result.current.promptCreateInFolder(lockedFolder, "text");
      result.current.handleFileSelected(file);
      result.current.uploadInFolder(lockedFolder, file);
      result.current.promptRenameNode(lockedText);
      result.current.promptMoveNode(lockedText);
      result.current.moveNodeToFolder(lockedText, unlockedFolder);
      result.current.moveNodeToFolder(unlockedFolder, lockedFolder);
      result.current.confirmDeleteNode(lockedText);
      result.current.promptReplaceMetadata();
      result.current.setNodeSearchEnabled(false);
      result.current.setTextEncryptionEnabled(true);
    });

    expect(setDialog).not.toHaveBeenCalled();
    expect(mocks.startUpload).not.toHaveBeenCalled();
    expect(mocks.moveNode).not.toHaveBeenCalled();
    expect(mocks.updateNodeSearchPolicy).not.toHaveBeenCalled();
    expect(mocks.updateTextEncryption).not.toHaveBeenCalled();
  });

  it("keeps the dedicated direct-lock control available under an inherited lock", () => {
    const activeSpace = space("space-1");
    const lockedText = {
      ...node("locked", activeSpace.id, "/Policies/locked.md"),
      effective_write_locked: true,
      write_lock_sources: [
        { node_id: "folder-1", name: "Policies", path: "/Policies" }
      ]
    };
    const { result } = renderNodeActions({
      activeSpace,
      activeNode: lockedText,
      canWriteActiveSpace: true,
      setDialog: vi.fn()
    });

    act(() => result.current.setNodeWriteLocked(true));

    expect(mocks.updateNodeWriteLock).toHaveBeenCalledWith({
      node: lockedText,
      enabled: true
    });
  });

  it("applies inspector commands to the inspected node instead of the open editor node", () => {
    const activeSpace = space("space-1");
    const openText = node("open", activeSpace.id, "/open.md");
    const inspectedFolder = node("folder-1", activeSpace.id, "/Policies", "folder");
    const setDialog = vi.fn();
    const { result } = renderNodeActions({
      activeSpace,
      activeNode: openText,
      inspectedNode: inspectedFolder,
      canWriteActiveSpace: true,
      setDialog
    });

    act(() => {
      result.current.promptReplaceMetadata();
      result.current.setNodeSearchEnabled(false);
      result.current.setNodeWriteLocked(true);
    });

    expect(setDialog).toHaveBeenCalledWith(expect.objectContaining({
      kind: "metadata",
      node: inspectedFolder
    }));
    expect(mocks.updateNodeSearchPolicy).toHaveBeenCalledWith({
      node: inspectedFolder,
      enabled: false
    });
    expect(mocks.updateNodeWriteLock).toHaveBeenCalledWith({
      node: inspectedFolder,
      enabled: true
    });
  });

  it("allows sibling creation when only the active text is directly locked", async () => {
    const activeSpace = space("space-1");
    const directlyLockedText = {
      ...node("locked", activeSpace.id, "/Policies/locked.md"),
      parent_id: "folder-1",
      write_locked: true,
      effective_write_locked: true,
      write_lock_sources: [
        { node_id: "locked", name: "locked.md", path: "/Policies/locked.md" }
      ]
    };
    const setDialog = vi.fn();
    const { result } = renderNodeActions({
      activeSpace,
      activeNode: directlyLockedText,
      canWriteActiveSpace: true,
      setDialog
    });

    act(() => result.current.promptCreateNode("text"));

    expect(setDialog).toHaveBeenCalledOnce();
    const dialog = setDialog.mock.calls[0]?.[0];
    expect(dialog).toMatchObject({ kind: "prompt" });
    if (!dialog || dialog.kind !== "prompt") throw new Error("create prompt was not opened");

    await act(async () => { await dialog.onSubmit("sibling.md"); });
    expect(mocks.createNode).toHaveBeenCalledWith({
      parentId: "folder-1",
      kind: "text",
      name: "sibling.md",
      content: ""
    });
  });

  it("downloads a file through the browser download path", async () => {
    const activeSpace = space("space-1");
    const fileSummary = node(
      "file-1",
      activeSpace.id,
      "/Reports/report.pdf",
      "file"
    );
    const fileNode = {
      ...fileSummary,
      original_filename: "source-report.pdf"
    };
    vi.mocked(getNode).mockResolvedValue(fileNode);
    const { result } = renderNodeActions({
        activeSpace,
        activeNode: fileNode,
        canWriteActiveSpace: true,
        setDialog: vi.fn()
      });

    await act(async () => { await result.current.downloadFileNode(fileSummary); });

    expect(getNode).toHaveBeenCalledWith(
      expect.anything(),
      activeSpace.id,
      fileNode.id
    );
    expect(mocks.downloadFile).toHaveBeenCalledWith(
      expect.anything(),
      activeSpace.id,
      fileNode.id,
      "source-report.pdf"
    );
  });
});

function renderNodeActions(
  props: Omit<Parameters<typeof useWorkbenchNodeActions>[0], "canManageActiveSpace" | "inspectedNode">
    & Partial<Pick<Parameters<typeof useWorkbenchNodeActions>[0], "canManageActiveSpace" | "inspectedNode">>
) {
  const queryClient = new QueryClient({
    defaultOptions: { queries: { retry: false } }
  });
  const wrapper = ({ children }: { children: ReactNode }) => (
    <QueryClientProvider client={queryClient}>{children}</QueryClientProvider>
  );
  const inspectedNode = props.inspectedNode === undefined ? props.activeNode : props.inspectedNode;
  return renderHook(
    () => useWorkbenchNodeActions({ canManageActiveSpace: true, ...props, inspectedNode }),
    { wrapper }
  );
}

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
  return makeSpace({
    id,
    name: id,
    root_node_id: `${id}-root`
  });
}

function node(id: string, spaceId: string, path: string, kind: RestNode["kind"] = "text"): RestNode {
  return makeRestNode({
    id,
    space_id: spaceId,
    parent_id: `${spaceId}-root`,
    name: path.split("/").pop() ?? id,
    kind,
    path,
    has_children: kind === "folder"
  });
}
