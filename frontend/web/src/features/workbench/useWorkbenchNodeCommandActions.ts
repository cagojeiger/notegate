import type { Dispatch, SetStateAction } from "react";

import { downloadFile } from "../../api/files";
import { useApiClient } from "../../api/ApiProvider";
import type { NodeSummary, RestNode, Space } from "../../api/types";
import { useUiStore } from "../../stores/uiStore";
import { useUploadActions } from "../uploads/UploadProvider";
import { createNodeDialog, deleteNodeDialog, metadataDialog, moveNodeDialog, renameNodeDialog, uploadFileDialog } from "./dialogs/appDialogs";
import type { AppDialog } from "./dialogs/dialogTypes";
import type { CanonicalNodeLoader } from "./useCanonicalNodeLoader";
import {
  useCreateNodeMutation,
  useDeleteNodeMutation,
  useMoveNodeMutation,
  useReplaceMetadataMutation,
  useUpdateNodeMutation,
  useUpdateNodeSearchPolicyMutation,
  useUpdateNodeWriteLockMutation,
  useUpdateTextEncryptionMutation
} from "./useWorkbenchQueries";

type CommandActionsProps = {
  activeSpace: Space | null;
  activeNode: RestNode | null;
  canWriteActiveSpace: boolean;
  canManageActiveSpace: boolean;
  setDialog: Dispatch<SetStateAction<AppDialog | null>>;
  loadCanonicalNode: CanonicalNodeLoader;
};

export function useWorkbenchNodeCommandActions({
  activeSpace,
  activeNode,
  canWriteActiveSpace,
  canManageActiveSpace,
  setDialog,
  loadCanonicalNode
}: CommandActionsProps) {
  const client = useApiClient();
  const openInActiveGroup = useUiStore((state) => state.openInActiveGroup);
  const updateGroupsNode = useUiStore((state) => state.updateGroupsNode);
  const clearGroupsWithNode = useUiStore((state) => state.clearGroupsWithNode);
  const addExpanded = useUiStore((state) => state.addExpanded);
  const setExpanded = useUiStore((state) => state.setExpanded);
  const { startUpload } = useUploadActions();

  const createNodeMutation = useCreateNodeMutation(activeSpace, (node) => {
    addExpanded([node.parent_id ?? activeSpace!.root_node_id]);
    openInActiveGroup(node);
  });
  const updateNodeMutation = useUpdateNodeMutation(updateGroupsNode);
  const updateNodeSearchPolicyMutation = useUpdateNodeSearchPolicyMutation(updateGroupsNode);
  const updateNodeWriteLockMutation = useUpdateNodeWriteLockMutation(updateGroupsNode);
  const updateTextEncryptionMutation = useUpdateTextEncryptionMutation(updateGroupsNode);
  const moveNodeMutation = useMoveNodeMutation(updateGroupsNode);
  const moveNodeDialogMutation = useMoveNodeMutation(updateGroupsNode, { silentError: true });
  const deleteNodeMutation = useDeleteNodeMutation((node) => clearGroupsWithNode(node.id));
  const replaceMetadataMutation = useReplaceMetadataMutation(updateGroupsNode);

  function parentForCreate(): { id: string; path: string; writeLocked: boolean } | null {
    if (!activeSpace) return null;
    if (!activeNode) return { id: activeSpace.root_node_id, path: "/", writeLocked: false };
    if (activeNode.kind === "folder") {
      return {
        id: activeNode.id,
        path: activeNode.path,
        writeLocked: activeNode.effective_write_locked
      };
    }
    return {
      id: activeNode.parent_id ?? activeSpace.root_node_id,
      path: parentPath(activeNode.path),
      writeLocked: activeNode.write_lock_sources.some((source) => source.node_id !== activeNode.id)
    };
  }

  function promptCreateNode(kind: "folder" | "text") {
    if (!canWriteActiveSpace) return;
    const parent = parentForCreate();
    if (!parent || parent.writeLocked) return;
    setDialog(createNodeDialog(parent.id, kind, async (input) => {
      await createNodeMutation.mutateAsync(input);
    }));
  }

  function promptCreateInFolder(folder: NodeSummary, kind: "folder" | "text") {
    if (!canWriteActiveSpace || folder.effective_write_locked) return;
    setDialog(createNodeDialog(folder.id, kind, async (input) => {
      await createNodeMutation.mutateAsync(input);
    }));
  }

  function uploadInFolder(folder: NodeSummary, file: File | null) {
    if (!canWriteActiveSpace || folder.effective_write_locked || !file || !activeSpace || folder.space_id !== activeSpace.id) return;
    promptUpload(activeSpace, folder.id, folder.path, file);
  }

  function collapseTree() {
    if (activeSpace) setExpanded([activeSpace.root_node_id]);
  }

  function promptRenameNode(node: NodeSummary) {
    if (!canWriteActiveSpace || node.effective_write_locked || node.parent_id === null) return;
    setDialog(renameNodeDialog(node, async (renamedNode, name) => {
      await updateNodeMutation.mutateAsync({ node: renamedNode, name });
    }));
  }

  function promptMoveNode(node: NodeSummary) {
    if (!canWriteActiveSpace || node.effective_write_locked || node.parent_id === null || !activeSpace) return;
    setDialog(moveNodeDialog(node, activeSpace, async (movedNode, parentId) => {
      await moveNodeDialogMutation.mutateAsync(
        { node: movedNode, parentId },
        { onSuccess: () => addExpanded([parentId]) }
      );
    }));
  }

  function moveNodeToFolder(node: NodeSummary, folder: NodeSummary) {
    if (
      !canWriteActiveSpace
      || node.effective_write_locked
      || folder.effective_write_locked
      || node.parent_id === null
      || folder.kind !== "folder"
      || node.id === folder.id
    ) return;
    moveNodeMutation.mutate(
      { node, parentId: folder.id },
      { onSuccess: () => addExpanded([folder.id]) }
    );
  }

  function confirmDeleteNode(node: NodeSummary) {
    if (!canWriteActiveSpace || node.effective_write_locked || node.parent_id === null) return;
    setDialog(deleteNodeDialog(node, async (deletedNode, recursive) => {
      await deleteNodeMutation.mutateAsync({ node: deletedNode, recursive });
    }));
  }

  function handleFileSelected(file: File | null) {
    const parent = parentForCreate();
    if (!canWriteActiveSpace || !file || !parent || parent.writeLocked || !activeSpace) return;
    promptUpload(activeSpace, parent.id, parent.path, file);
  }

  function promptUpload(space: Space, parentId: string, destinationPath: string, file: File) {
    setDialog(uploadFileDialog(parentId, file, (input) => {
      startUpload({
        parentNodeId: input.parentId,
        name: input.name,
        file: input.file,
        spaceId: space.id,
        spaceName: space.name,
        destinationPath
      });
    }));
  }

  async function downloadFileNode(node: NodeSummary) {
    if (node.kind !== "file") return;
    const canonicalNode = await loadCanonicalNode(node, "Could not download file");
    if (!canonicalNode) return;
    await downloadFile(
      client,
      canonicalNode.space_id,
      canonicalNode.id,
      canonicalNode.original_filename ?? canonicalNode.name
    );
  }

  function promptReplaceMetadata() {
    if (!canWriteActiveSpace || !activeNode || activeNode.effective_write_locked) return;
    const node = activeNode;
    setDialog(metadataDialog(node, async (metadataNode, metadata) => {
      await replaceMetadataMutation.mutateAsync({ node: metadataNode, metadata });
    }));
  }

  function setNodeSearchEnabled(searchEnabled: boolean) {
    if (
      !canManageActiveSpace
      || !activeNode
      || activeNode.parent_id === null
      || activeNode.effective_write_locked
      || updateNodeSearchPolicyMutation.isPending
    ) return;
    updateNodeSearchPolicyMutation.mutate({
      node: activeNode,
      enabled: searchEnabled
    });
  }

  function setTextEncryptionEnabled(textEncryptionEnabled: boolean) {
    if (
      !canManageActiveSpace
      || !activeNode
      || activeNode.kind !== "text"
      || activeNode.effective_write_locked
      || updateTextEncryptionMutation.isPending
    ) return;
    updateTextEncryptionMutation.mutate({
      node: activeNode,
      enabled: textEncryptionEnabled
    });
  }

  function setNodeWriteLocked(writeLocked: boolean) {
    if (
      !canManageActiveSpace
      || !activeNode
      || activeNode.parent_id === null
      || updateNodeWriteLockMutation.isPending
    ) return;
    updateNodeWriteLockMutation.mutate({
      node: activeNode,
      enabled: writeLocked
    });
  }

  return {
    promptCreateNode,
    promptCreateInFolder,
    handleFileSelected,
    uploadInFolder,
    collapseTree,
    promptRenameNode,
    promptMoveNode,
    moveNodeToFolder,
    confirmDeleteNode,
    promptReplaceMetadata,
    setNodeSearchEnabled,
    setNodeWriteLocked,
    setTextEncryptionEnabled,
    nodeSearchPolicyPending: updateNodeSearchPolicyMutation.isPending,
    nodeWriteLockPending: updateNodeWriteLockMutation.isPending,
    textEncryptionPending: updateTextEncryptionMutation.isPending,
    downloadFileNode
  };
}

function parentPath(path: string): string {
  const lastSlash = path.lastIndexOf("/");
  return lastSlash <= 0 ? "/" : path.slice(0, lastSlash);
}
