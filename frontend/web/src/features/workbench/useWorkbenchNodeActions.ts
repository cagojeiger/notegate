import type { Dispatch, SetStateAction } from "react";
import { useQueryClient } from "@tanstack/react-query";

import { downloadFile } from "../../api/files";
import { useApiClient } from "../../api/ApiProvider";
import { ApiError } from "../../api/errors";
import { resolveNodePath } from "../../api/nodes";
import { queryKeys } from "../../api/queryKeys";
import type { RestNode } from "../../entities/node/model";
import type { Space } from "../../entities/space/model";
import type { AppDialog } from "./dialogs/DialogHost";
import { createNodeDialog, deleteNodeDialog, metadataDialog, moveNodeDialog, renameNodeDialog, uploadFileDialog } from "./dialogs/appDialogs";
import { useUiStore } from "../../stores/uiStore";
import { useUploadActions } from "../uploads/UploadProvider";
import { useCreateNodeMutation, useDeleteNodeMutation, useMoveNodeMutation, useReplaceMetadataMutation, useRevealNode, useUpdateNodeMutation } from "./useWorkbenchQueries";

type NodeActionsProps = {
  activeSpace: Space | null;
  activeNode: RestNode | null;
  canWriteActiveSpace: boolean;
  setDialog: Dispatch<SetStateAction<AppDialog | null>>;
};

export function useWorkbenchNodeActions({ activeSpace, activeNode, canWriteActiveSpace, setDialog }: NodeActionsProps) {
  const client = useApiClient();
  const queryClient = useQueryClient();
  const openInActiveGroup = useUiStore((state) => state.openInActiveGroup);
  const openInGroup = useUiStore((state) => state.openInGroup);
  const openInNewGroup = useUiStore((state) => state.openInNewGroup);
  const clearGroupsWithNode = useUiStore((state) => state.clearGroupsWithNode);
  const addExpanded = useUiStore((state) => state.addExpanded);
  const setExpanded = useUiStore((state) => state.setExpanded);
  const closeMobile = useUiStore((state) => state.closeMobile);
  const showToast = useUiStore((state) => state.showToast);
  const { startUpload } = useUploadActions();

  const createNodeMutation = useCreateNodeMutation(activeSpace, (node) => {
    addExpanded([node.parent_id ?? activeSpace!.root_node_id]);
    openInActiveGroup(node);
  });
  const updateNodeMutation = useUpdateNodeMutation();
  const moveNodeMutation = useMoveNodeMutation();
  const deleteNodeMutation = useDeleteNodeMutation((node) => clearGroupsWithNode(node.id));
  const replaceMetadataMutation = useReplaceMetadataMutation();
  const revealNodeInSpace = useRevealNode();

  async function openNode(node: RestNode) {
    cacheOpenedNode(node);
    openInActiveGroup(node);
    closeMobile();
    await revealNodeBestEffort(node);
  }

  async function openNodeInNewGroup(node: RestNode) {
    cacheOpenedNode(node);
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

    cacheOpenedNode(node);
    openInGroup(groupId, node);
    closeMobile();
    await revealNodeBestEffort(node);
  }

  function isCurrentMarkdownLinkSource(spaceId: string, groupId: number, sourceNode: RestNode): boolean {
    const state = useUiStore.getState();
    return state.activeSpaceId === spaceId && state.editorGroups.some((group) => group.id === groupId && group.nodeRef?.nodeId === sourceNode.id);
  }

  function cacheOpenedNode(node: RestNode) {
    queryClient.setQueryData(queryKeys.node(node.space_id, node.id), node);
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
    if (!canWriteActiveSpace || !file || !activeSpace || folder.space_id !== activeSpace.id) return;
    promptUpload(activeSpace, folder.id, folder.path, file);
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
    if (!canWriteActiveSpace || !file || !parentId || !activeSpace) return;
    const destinationPath = !activeNode
      ? "/"
      : activeNode.kind === "folder" ? activeNode.path : parentPath(activeNode.path);
    promptUpload(activeSpace, parentId, destinationPath, file);
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

  async function downloadFileNode(node: RestNode) {
    if (node.kind !== "file") return;
    await downloadFile(client, node.space_id, node.id, node.original_filename ?? node.name);
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

function parentPath(path: string): string {
  const lastSlash = path.lastIndexOf("/");
  return lastSlash <= 0 ? "/" : path.slice(0, lastSlash);
}
