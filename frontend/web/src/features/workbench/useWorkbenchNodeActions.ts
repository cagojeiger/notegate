import type { Dispatch, SetStateAction } from "react";

import type { RestNode, Space } from "../../api/types";
import type { AppDialog } from "../../layout/dialogs/DialogHost";
import { createNodeDialog, deleteNodeDialog, metadataDialog, moveNodeDialog, renameNodeDialog, uploadFileDialog } from "../../layout/dialogs/appDialogs";
import { useUiStore } from "../../stores/uiStore";
import { useCreateNodeMutation, useDeleteNodeMutation, useMoveNodeMutation, useReplaceMetadataMutation, useRevealNode, useUpdateNodeMutation, useUploadFileMutation } from "./useWorkbenchQueries";

type NodeActionsProps = {
  activeSpace: Space | null;
  activeNode: RestNode | null;
  setDialog: Dispatch<SetStateAction<AppDialog | null>>;
};

export function useWorkbenchNodeActions({ activeSpace, activeNode, setDialog }: NodeActionsProps) {
  const openInActiveGroup = useUiStore((state) => state.openInActiveGroup);
  const updateGroupsNode = useUiStore((state) => state.updateGroupsNode);
  const clearGroupsWithNode = useUiStore((state) => state.clearGroupsWithNode);
  const addExpanded = useUiStore((state) => state.addExpanded);
  const setExpanded = useUiStore((state) => state.setExpanded);
  const closeMobile = useUiStore((state) => state.closeMobile);

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
    const parentId = parentForCreate();
    if (!parentId) return;
    setDialog(createNodeDialog(parentId, kind, (input) => createNodeMutation.mutate(input)));
  }

  function promptCreateInFolder(folder: RestNode, kind: "folder" | "text") {
    setDialog(createNodeDialog(folder.id, kind, (input) => createNodeMutation.mutate(input)));
  }

  function uploadInFolder(folder: RestNode, file: File | null) {
    if (!file) return;
    setDialog(uploadFileDialog(folder.id, file, (input) => uploadFileMutation.mutate(input)));
  }

  function collapseTree() {
    if (activeSpace) setExpanded([activeSpace.root_node_id]);
  }

  function promptRenameNode(node: RestNode) {
    if (node.parent_id === null) return;
    setDialog(renameNodeDialog(node, (renamedNode, name) => updateNodeMutation.mutate({ node: renamedNode, name })));
  }

  function promptMoveNode(node: RestNode) {
    if (node.parent_id === null || !activeSpace) return;
    setDialog(moveNodeDialog(node, activeSpace, (movedNode, parentId) => moveNodeMutation.mutate({ node: movedNode, parentId }, { onSuccess: () => addExpanded([parentId]) })));
  }

  function moveNodeToFolder(node: RestNode, folder: RestNode) {
    if (node.parent_id === null || folder.kind !== "folder" || node.id === folder.id) return;
    moveNodeMutation.mutate({ node, parentId: folder.id }, { onSuccess: () => addExpanded([folder.id]) });
  }

  function confirmDeleteNode(node: RestNode) {
    if (node.parent_id === null) return;
    setDialog(deleteNodeDialog(node, (deletedNode, recursive) => deleteNodeMutation.mutate({ node: deletedNode, recursive })));
  }

  function handleFileSelected(file: File | null) {
    const parentId = parentForCreate();
    if (!file || !parentId) return;
    setDialog(uploadFileDialog(parentId, file, (input) => uploadFileMutation.mutate(input)));
  }

  function promptReplaceMetadata() {
    if (!activeNode) return;
    const node = activeNode;
    setDialog(metadataDialog(node, (metadataNode, metadata) => replaceMetadataMutation.mutate({ node: metadataNode, metadata })));
  }

  return {
    openNode,
    promptCreateNode,
    promptCreateInFolder,
    handleFileSelected,
    uploadInFolder,
    collapseTree,
    promptRenameNode,
    promptMoveNode,
    moveNodeToFolder,
    confirmDeleteNode,
    promptReplaceMetadata
  };
}
