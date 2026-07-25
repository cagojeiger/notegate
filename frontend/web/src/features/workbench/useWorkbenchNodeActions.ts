import type { Dispatch, SetStateAction } from "react";
import { useQueryClient } from "@tanstack/react-query";

import { downloadFile } from "../../api/files";
import { useApiClient } from "../../api/ApiProvider";
import { ApiError } from "../../api/errors";
import { getNode, resolveNodePath } from "../../api/nodes";
import { queryKeys } from "../../api/queryKeys";
import type { NodeSummary, RestNode, Space } from "../../api/types";
import type { AppDialog } from "./dialogs/dialogTypes";
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
  const updateGroupsNode = useUiStore((state) => state.updateGroupsNode);
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
  const updateNodeMutation = useUpdateNodeMutation(updateGroupsNode);
  const moveNodeMutation = useMoveNodeMutation(updateGroupsNode);
  const deleteNodeMutation = useDeleteNodeMutation((node) => clearGroupsWithNode(node.id));
  const replaceMetadataMutation = useReplaceMetadataMutation(updateGroupsNode);
  const revealNodeInSpace = useRevealNode();

  async function openNode(summary: NodeSummary) {
    const node = await loadCanonicalNode(summary, "Could not open node");
    if (!node) return;
    openInActiveGroup(node);
    closeMobile();
    await revealNodeBestEffort(node);
  }

  async function openNodeInNewGroup(summary: NodeSummary) {
    const node = await loadCanonicalNode(summary, "Could not open node");
    if (!node) return;
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

  async function revealNodeBestEffort(node: NodeSummary) {
    try {
      await revealNode(node);
    } catch {
      showToast("Opened node, but could not reveal it in the tree");
    }
  }

  async function revealNode(node: NodeSummary) {
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

  function promptCreateInFolder(folder: NodeSummary, kind: "folder" | "text") {
    if (!canWriteActiveSpace) return;
    setDialog(createNodeDialog(folder.id, kind, async (input) => {
      await createNodeMutation.mutateAsync(input);
    }));
  }

  function uploadInFolder(folder: NodeSummary, file: File | null) {
    if (!canWriteActiveSpace || !file || !activeSpace || folder.space_id !== activeSpace.id) return;
    promptUpload(activeSpace, folder.id, folder.path, file);
  }

  function collapseTree() {
    if (activeSpace) setExpanded([activeSpace.root_node_id]);
  }

  function promptRenameNode(node: NodeSummary) {
    if (!canWriteActiveSpace || node.parent_id === null) return;
    setDialog(renameNodeDialog(node, async (renamedNode, name) => {
      await updateNodeMutation.mutateAsync({ node: renamedNode, name });
    }));
  }

  function promptMoveNode(node: NodeSummary) {
    if (!canWriteActiveSpace || node.parent_id === null || !activeSpace) return;
    setDialog(moveNodeDialog(node, activeSpace, async (movedNode, parentId) => {
      await moveNodeMutation.mutateAsync({ node: movedNode, parentId }, { onSuccess: () => addExpanded([parentId]) });
    }));
  }

  function moveNodeToFolder(node: NodeSummary, folder: NodeSummary) {
    if (!canWriteActiveSpace || node.parent_id === null || folder.kind !== "folder" || node.id === folder.id) return;
    moveNodeMutation.mutate({ node, parentId: folder.id }, { onSuccess: () => addExpanded([folder.id]) });
  }

  function confirmDeleteNode(node: NodeSummary) {
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
    if (!canWriteActiveSpace || !activeNode) return;
    const node = activeNode;
    setDialog(metadataDialog(node, async (metadataNode, metadata) => {
      await replaceMetadataMutation.mutateAsync({ node: metadataNode, metadata });
    }));
  }

  async function loadCanonicalNode(
    summary: NodeSummary,
    failureMessage: string
  ): Promise<RestNode | null> {
    try {
      return await queryClient.fetchQuery({
        queryKey: queryKeys.node(summary.space_id, summary.id),
        queryFn: () => getNode(client, summary.space_id, summary.id),
        staleTime: Number.POSITIVE_INFINITY
      });
    } catch {
      showToast(failureMessage);
      return null;
    }
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
