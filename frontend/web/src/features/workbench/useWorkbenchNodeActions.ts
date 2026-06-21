import type { Dispatch, SetStateAction } from "react";

import { downloadFile } from "../../api/files";
import { useApiClient } from "../../api/ApiProvider";
import { ApiError } from "../../api/errors";
import { resolveNodePath } from "../../api/nodes";
import type { RestNode, Space } from "../../api/types";
import type { AppDialog } from "../../layout/dialogs/DialogHost";
import { createNodeDialog, deleteNodeDialog, metadataDialog, moveNodeDialog, renameNodeDialog, uploadFileDialog } from "../../layout/dialogs/appDialogs";
import { downloadBlob } from "../../shared/lib/downloadBlob";
import { useUiStore } from "../../stores/uiStore";
import { useCreateNodeMutation, useDeleteNodeMutation, useMoveNodeMutation, useReplaceMetadataMutation, useRevealNode, useUpdateNodeMutation, useUploadFileMutation } from "./useWorkbenchQueries";

type NodeActionsProps = {
  activeSpace: Space | null;
  activeNode: RestNode | null;
  canWriteActiveSpace: boolean;
  setDialog: Dispatch<SetStateAction<AppDialog | null>>;
};

export function useWorkbenchNodeActions({ activeSpace, activeNode, canWriteActiveSpace, setDialog }: NodeActionsProps) {
  const client = useApiClient();
  const openInActiveGroup = useUiStore((state) => state.openInActiveGroup);
  const openInGroup = useUiStore((state) => state.openInGroup);
  const openInNewGroup = useUiStore((state) => state.openInNewGroup);
  const updateGroupsNode = useUiStore((state) => state.updateGroupsNode);
  const clearGroupsWithNode = useUiStore((state) => state.clearGroupsWithNode);
  const addExpanded = useUiStore((state) => state.addExpanded);
  const setExpanded = useUiStore((state) => state.setExpanded);
  const closeMobile = useUiStore((state) => state.closeMobile);
  const showToast = useUiStore((state) => state.showToast);

  const createNodeMutation = useCreateNodeMutation(activeSpace, (node) => {
    addExpanded([node.parent_id ?? activeSpace!.root_node_id]);
    openInActiveGroup(node);
  });
  const updateNodeMutation = useUpdateNodeMutation(updateGroupsNode);
  const moveNodeMutation = useMoveNodeMutation(updateGroupsNode);
  const deleteNodeMutation = useDeleteNodeMutation((node) => clearGroupsWithNode(node.id));
  const uploadFileMutation = useUploadFileMutation(activeSpace, openInActiveGroup);
  const replaceMetadataMutation = useReplaceMetadataMutation(updateGroupsNode);
  const revealNodeInSpace = useRevealNode();

  async function openNode(node: RestNode) {
    openInActiveGroup(node);
    closeMobile();
    await revealNodeBestEffort(node);
  }

  async function openNodeInNewGroup(node: RestNode) {
    openInNewGroup(node);
    closeMobile();
    await revealNodeBestEffort(node);
  }

  async function openMarkdownLink(groupId: number, sourceNode: RestNode, path: string) {
    if (!activeSpace || sourceNode.space_id !== activeSpace.id || !isCurrentMarkdownLinkSource(activeSpace.id, groupId, sourceNode)) return;
    const spaceId = activeSpace.id;

    let node: RestNode;
    try {
      node = await resolveNodePath(client, spaceId, path);
    } catch (error) {
      showToast(error instanceof ApiError && error.status === 404 ? "Linked node not found" : "Could not open linked node");
      return;
    }

    if (node.space_id !== spaceId) {
      showToast("Could not open linked node");
      return;
    }
    if (!isCurrentMarkdownLinkSource(spaceId, groupId, sourceNode)) return;

    openInGroup(groupId, node);
    closeMobile();
    await revealNodeBestEffort(node);
  }

  function isCurrentMarkdownLinkSource(spaceId: string, groupId: number, sourceNode: RestNode): boolean {
    const state = useUiStore.getState();
    return state.activeSpaceId === spaceId && state.editorGroups.some((group) => group.id === groupId && group.node?.id === sourceNode.id);
  }

  async function revealNodeBestEffort(node: RestNode) {
    try {
      await revealNode(node);
    } catch {
      showToast("Opened node, but could not reveal it in the tree");
    }
  }

  async function revealNode(node: RestNode) {
    if (!activeSpace || node.parent_id === null) return;
    const reveal = await revealNodeInSpace(activeSpace.id, node.id);
    addExpanded(reveal.ancestors.map((ancestor) => ancestor.id));
  }

  function parentForCreate(): string | null {
    if (!activeSpace) return null;
    if (!activeNode) return activeSpace.root_node_id;
    return activeNode.kind === "folder" ? activeNode.id : activeNode.parent_id ?? activeSpace.root_node_id;
  }

  function promptCreateNode(kind: "folder" | "text") {
    if (!canWriteActiveSpace) return;
    const parentId = parentForCreate();
    if (!parentId) return;
    setDialog(createNodeDialog(parentId, kind, async (input) => {
      await createNodeMutation.mutateAsync(input);
    }));
  }

  function promptCreateInFolder(folder: RestNode, kind: "folder" | "text") {
    if (!canWriteActiveSpace) return;
    setDialog(createNodeDialog(folder.id, kind, async (input) => {
      await createNodeMutation.mutateAsync(input);
    }));
  }

  function uploadInFolder(folder: RestNode, file: File | null) {
    if (!canWriteActiveSpace || !file) return;
    setDialog(uploadFileDialog(folder.id, file, async (input) => {
      await uploadFileMutation.mutateAsync(input);
    }));
  }

  function collapseTree() {
    if (activeSpace) setExpanded([activeSpace.root_node_id]);
  }

  function promptRenameNode(node: RestNode) {
    if (!canWriteActiveSpace || node.parent_id === null) return;
    setDialog(renameNodeDialog(node, async (renamedNode, name) => {
      await updateNodeMutation.mutateAsync({ node: renamedNode, name });
    }));
  }

  function promptMoveNode(node: RestNode) {
    if (!canWriteActiveSpace || node.parent_id === null || !activeSpace) return;
    setDialog(moveNodeDialog(node, activeSpace, async (movedNode, parentId) => {
      await moveNodeMutation.mutateAsync({ node: movedNode, parentId }, { onSuccess: () => addExpanded([parentId]) });
    }));
  }

  function moveNodeToFolder(node: RestNode, folder: RestNode) {
    if (!canWriteActiveSpace || node.parent_id === null || folder.kind !== "folder" || node.id === folder.id) return;
    moveNodeMutation.mutate({ node, parentId: folder.id }, { onSuccess: () => addExpanded([folder.id]) });
  }

  function confirmDeleteNode(node: RestNode) {
    if (!canWriteActiveSpace || node.parent_id === null) return;
    setDialog(deleteNodeDialog(node, async (deletedNode, recursive) => {
      await deleteNodeMutation.mutateAsync({ node: deletedNode, recursive });
    }));
  }

  function handleFileSelected(file: File | null) {
    const parentId = parentForCreate();
    if (!canWriteActiveSpace || !file || !parentId) return;
    setDialog(uploadFileDialog(parentId, file, async (input) => {
      await uploadFileMutation.mutateAsync(input);
    }));
  }

  async function downloadFileNode(node: RestNode) {
    if (node.kind !== "file") return;
    const blob = await downloadFile(client, node.space_id, node.id);
    downloadBlob(blob, node.original_filename ?? node.name);
  }

  function promptReplaceMetadata() {
    if (!canWriteActiveSpace || !activeNode) return;
    const node = activeNode;
    setDialog(metadataDialog(node, async (metadataNode, metadata) => {
      await replaceMetadataMutation.mutateAsync({ node: metadataNode, metadata });
    }));
  }

  return {
    openNode,
    openNodeInNewGroup,
    openMarkdownLink,
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
    downloadFileNode
  };
}
