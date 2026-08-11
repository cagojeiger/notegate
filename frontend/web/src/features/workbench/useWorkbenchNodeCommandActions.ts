import type { Dispatch, SetStateAction } from "react";

import { downloadFile } from "../../api/files";
import { useApiClient } from "../../api/ApiProvider";
import type { NodeSummary, RestNode, Space } from "../../api/types";
import { useUiStore } from "../../stores/uiStore";
import {
  canCreateInFolder,
  canMoveNodeToFolder,
  canMutateNode,
  canWriteNode,
  resolveNodeCreateTarget
} from "../nodes/nodeWriteAccess";
import { useAudioRecordingActions } from "../recording/AudioRecordingContext";
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
  inspectedNode: RestNode | null;
  canWriteActiveSpace: boolean;
  canManageActiveSpace: boolean;
  setDialog: Dispatch<SetStateAction<AppDialog | null>>;
  loadCanonicalNode: CanonicalNodeLoader;
};

const LOCKED_DESTINATION_MESSAGE =
  "Changes are blocked because the destination folder or an ancestor is write-locked";

export function useWorkbenchNodeCommandActions({
  activeSpace,
  activeNode,
  inspectedNode,
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
  const showToast = useUiStore((state) => state.showToast);
  const { startUpload } = useUploadActions();
  const { startRecording } = useAudioRecordingActions();

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

  function promptCreateNode(kind: "folder" | "text") {
    if (!canWriteActiveSpace || !activeSpace) return;
    const parent = resolveNodeCreateTarget(activeSpace.root_node_id, activeNode);
    if (parent.writeLocked) {
      showToast(LOCKED_DESTINATION_MESSAGE);
      return;
    }
    setDialog(createNodeDialog(parent.id, kind, async (input) => {
      await createNodeMutation.mutateAsync(input);
    }));
  }

  function promptCreateInFolder(folder: NodeSummary, kind: "folder" | "text") {
    if (!canCreateInFolder(folder, canWriteActiveSpace)) return;
    setDialog(createNodeDialog(folder.id, kind, async (input) => {
      await createNodeMutation.mutateAsync(input);
    }));
  }

  function uploadInFolder(folder: NodeSummary, file: File | null) {
    if (!canCreateInFolder(folder, canWriteActiveSpace) || !file || !activeSpace || folder.space_id !== activeSpace.id) return;
    promptUpload(activeSpace, folder.id, folder.path, file);
  }

  function collapseTree() {
    if (activeSpace) setExpanded([activeSpace.root_node_id]);
  }

  function promptRenameNode(node: NodeSummary) {
    if (!canMutateNode(node, canWriteActiveSpace)) return;
    setDialog(renameNodeDialog(node, async (renamedNode, name) => {
      await updateNodeMutation.mutateAsync({ node: renamedNode, name });
    }));
  }

  function promptMoveNode(node: NodeSummary) {
    if (!canMutateNode(node, canWriteActiveSpace) || !activeSpace) return;
    setDialog(moveNodeDialog(node, activeSpace, async (movedNode, parentId) => {
      await moveNodeDialogMutation.mutateAsync(
        { node: movedNode, parentId },
        { onSuccess: () => addExpanded([parentId]) }
      );
    }));
  }

  function moveNodeToFolder(node: NodeSummary, folder: NodeSummary) {
    if (!canMoveNodeToFolder(node, folder, canWriteActiveSpace)) return;
    moveNodeMutation.mutate(
      { node, parentId: folder.id },
      { onSuccess: () => addExpanded([folder.id]) }
    );
  }

  function confirmDeleteNode(node: NodeSummary) {
    if (!canMutateNode(node, canWriteActiveSpace)) return;
    setDialog(deleteNodeDialog(node, async (deletedNode, recursive) => {
      await deleteNodeMutation.mutateAsync({ node: deletedNode, recursive });
    }));
  }

  function handleFileSelected(file: File | null) {
    if (!canWriteActiveSpace || !file || !activeSpace) return;
    const parent = resolveNodeCreateTarget(activeSpace.root_node_id, activeNode);
    if (parent.writeLocked) {
      showToast(LOCKED_DESTINATION_MESSAGE);
      return;
    }
    promptUpload(activeSpace, parent.id, parent.path, file);
  }

  function recordAudio() {
    if (!canWriteActiveSpace || !activeSpace) return;
    void startRecording({
      parentNodeId: activeSpace.root_node_id,
      spaceId: activeSpace.id,
      spaceName: activeSpace.name,
      destinationPath: "/"
    });
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
    if (!inspectedNode || !canWriteNode(inspectedNode, canWriteActiveSpace)) return;
    const node = inspectedNode;
    setDialog(metadataDialog(node, async (metadataNode, metadata) => {
      await replaceMetadataMutation.mutateAsync({ node: metadataNode, metadata });
    }));
  }

  function setNodeSearchEnabled(searchEnabled: boolean) {
    if (
      !canManageActiveSpace
      || !inspectedNode
      || inspectedNode.parent_id === null
      || inspectedNode.effective_write_locked
      || updateNodeSearchPolicyMutation.isPending
    ) return;
    updateNodeSearchPolicyMutation.mutate({
      node: inspectedNode,
      enabled: searchEnabled
    });
  }

  function setTextEncryptionEnabled(textEncryptionEnabled: boolean) {
    if (
      !canManageActiveSpace
      || !inspectedNode
      || inspectedNode.kind !== "text"
      || inspectedNode.effective_write_locked
      || updateTextEncryptionMutation.isPending
    ) return;
    updateTextEncryptionMutation.mutate({
      node: inspectedNode,
      enabled: textEncryptionEnabled
    });
  }

  function setNodeWriteLocked(writeLocked: boolean) {
    if (
      !canManageActiveSpace
      || !inspectedNode
      || inspectedNode.parent_id === null
      || updateNodeWriteLockMutation.isPending
    ) return;
    updateNodeWriteLockMutation.mutate({
      node: inspectedNode,
      enabled: writeLocked
    });
  }

  return {
    promptCreateNode,
    promptCreateInFolder,
    handleFileSelected,
    recordAudio,
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
