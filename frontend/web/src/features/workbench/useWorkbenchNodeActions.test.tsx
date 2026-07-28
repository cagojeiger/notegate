import { QueryClientProvider } from "@tanstack/react-query";
import { act, renderHook } from "@testing-library/react";
import type { ReactNode } from "react";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { getNode } from "../../api/nodes";
import type { RestNode, Space } from "../../api/types";
import { useUiStore } from "../../stores/uiStore";
import { makeRestNode, makeSpace } from "../../test/fixtures";
import { createTestQueryClient } from "../../test/queryClient";
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
  const queryClient = createTestQueryClient();
  const wrapper = ({ children }: { children: ReactNode }) => (
    <QueryClientProvider client={queryClient}>{children}</QueryClientProvider>
  );
  const inspectedNode = props.inspectedNode === undefined ? props.activeNode : props.inspectedNode;
  return renderHook(
    () => useWorkbenchNodeActions({ canManageActiveSpace: true, ...props, inspectedNode }),
    { wrapper }
  );
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
