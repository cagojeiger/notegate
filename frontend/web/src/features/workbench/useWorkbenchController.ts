import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useEffect, useMemo, useState, type PointerEvent as ReactPointerEvent } from "react";

import { useApiClient } from "../../api/ApiProvider";
import { logout } from "../../api/auth";
import { uploadFile } from "../../api/files";
import { replaceMetadata } from "../../api/metadata";
import { createNode, deleteNode, moveNode, revealNode, updateNode } from "../../api/nodes";
import { queryKeys } from "../../api/queryKeys";
import { createSpace, deleteSpace, listSpaces, updateSpace } from "../../api/spaces";
import type { RestNode, Space } from "../../api/types";
import { clearDevApiKey } from "../../auth/session";
import { useIsMobile } from "../../shared/hooks/useMediaQuery";
import { persistLastSpace, persistTheme, useUiStore } from "../../stores/uiStore";
import type { AppDialog } from "../../layout/dialogs/DialogHost";
import { createNodeDialog, deleteNodeDialog, deleteSpaceDialog, metadataDialog, moveNodeDialog, newSpaceDialog, renameNodeDialog, renameSpaceDialog, uploadFileDialog } from "../../layout/dialogs/appDialogs";

type WorkbenchControllerProps = {
  onSignOut: () => void;
};

export function useWorkbenchController({ onSignOut }: WorkbenchControllerProps) {
  const client = useApiClient();
  const queryClient = useQueryClient();
  const spacesQuery = useQuery({ queryKey: queryKeys.spaces, queryFn: () => listSpaces(client) });
  const spaces = spacesQuery.data?.spaces ?? [];

  const theme = useUiStore((state) => state.theme);
  const activeSpaceId = useUiStore((state) => state.activeSpaceId);
  const editorGroups = useUiStore((state) => state.editorGroups);
  const activeGroupIndex = useUiStore((state) => state.activeGroupIndex);
  const expandedFolderIds = useUiStore((state) => state.expandedFolderIds);
  const primarySidebarOpen = useUiStore((state) => state.primarySidebarOpen);
  const auxiliaryOpen = useUiStore((state) => state.auxiliaryOpen);
  const toggleTheme = useUiStore((state) => state.toggleTheme);
  const setActiveSpaceId = useUiStore((state) => state.setActiveSpaceId);
  const openInActiveGroup = useUiStore((state) => state.openInActiveGroup);
  const addGroup = useUiStore((state) => state.addGroup);
  const closeGroup = useUiStore((state) => state.closeGroup);
  const focusGroup = useUiStore((state) => state.focusGroup);
  const setGroupMode = useUiStore((state) => state.setGroupMode);
  const updateGroupsNode = useUiStore((state) => state.updateGroupsNode);
  const clearGroupsWithNode = useUiStore((state) => state.clearGroupsWithNode);
  const resetGroups = useUiStore((state) => state.resetGroups);
  const toggleFolder = useUiStore((state) => state.toggleFolder);
  const addExpanded = useUiStore((state) => state.addExpanded);
  const setExpanded = useUiStore((state) => state.setExpanded);
  const togglePrimarySidebar = useUiStore((state) => state.togglePrimarySidebar);
  const primaryWidth = useUiStore((state) => state.primaryWidth);
  const setPrimaryWidth = useUiStore((state) => state.setPrimaryWidth);
  const toggleAuxiliary = useUiStore((state) => state.toggleAuxiliary);
  const mobileTreeOpen = useUiStore((state) => state.mobileTreeOpen);
  const mobileAuxOpen = useUiStore((state) => state.mobileAuxOpen);
  const toggleMobileTree = useUiStore((state) => state.toggleMobileTree);
  const toggleMobileAux = useUiStore((state) => state.toggleMobileAux);
  const closeMobile = useUiStore((state) => state.closeMobile);

  const isMobile = useIsMobile();
  const [settingsOpen, setSettingsOpen] = useState(false);
  const [dialog, setDialog] = useState<AppDialog | null>(null);
  const activeNode = editorGroups[activeGroupIndex]?.node ?? null;
  const activeSpace = useMemo(() => spaces.find((space) => space.id === activeSpaceId) ?? spaces[0] ?? null, [activeSpaceId, spaces]);
  const showAuxiliary = auxiliaryOpen && activeNode !== null;

  useEffect(() => {
    persistTheme(theme);
  }, [theme]);

  useEffect(() => {
    if (!activeSpace) return;
    setActiveSpaceId(activeSpace.id);
    persistLastSpace(activeSpace.id);
    addExpanded([activeSpace.root_node_id]);
  }, [activeSpace, setActiveSpaceId, addExpanded]);

  function invalidateSpace(spaceId: string) {
    void queryClient.invalidateQueries({ queryKey: queryKeys.spaces });
    void queryClient.invalidateQueries({ queryKey: ["spaces", spaceId] });
  }

  const createSpaceMutation = useMutation({
    mutationFn: (name: string) => createSpace(client, name),
    onSuccess: (space) => {
      void queryClient.invalidateQueries({ queryKey: queryKeys.spaces });
      setActiveSpaceId(space.id);
      resetGroups();
    }
  });
  const updateSpaceMutation = useMutation({
    mutationFn: ({ spaceId, name }: { spaceId: string; name: string }) => updateSpace(client, spaceId, { name }),
    onSuccess: () => void queryClient.invalidateQueries({ queryKey: queryKeys.spaces })
  });
  const deleteSpaceMutation = useMutation({
    mutationFn: (spaceId: string) => deleteSpace(client, spaceId),
    onSuccess: () => {
      resetGroups();
      setActiveSpaceId(null);
      void queryClient.invalidateQueries({ queryKey: queryKeys.spaces });
    }
  });
  const createNodeMutation = useMutation({
    mutationFn: ({ parentId, kind, name, content }: { parentId: string; kind: "folder" | "text"; name: string; content?: string }) =>
      createNode(client, activeSpace!.id, { parent_id: parentId, kind, name, content }),
    onSuccess: (node) => {
      invalidateSpace(node.space_id);
      addExpanded([node.parent_id ?? activeSpace!.root_node_id]);
      openInActiveGroup(node);
    }
  });
  const updateNodeMutation = useMutation({
    mutationFn: ({ node, name }: { node: RestNode; name: string }) => updateNode(client, node.space_id, node.id, { name }),
    onSuccess: (node) => {
      invalidateSpace(node.space_id);
      updateGroupsNode(node);
    }
  });
  const moveNodeMutation = useMutation({
    mutationFn: ({ node, parentId }: { node: RestNode; parentId: string }) =>
      moveNode(client, node.space_id, node.id, { new_parent_id: parentId, expected_parent_id: node.parent_id }),
    onSuccess: (node) => {
      invalidateSpace(node.space_id);
      updateGroupsNode(node);
    }
  });
  const deleteNodeMutation = useMutation({
    mutationFn: ({ node, recursive }: { node: RestNode; recursive: boolean }) => deleteNode(client, node.space_id, node.id, recursive),
    onSuccess: (_, variables) => {
      clearGroupsWithNode(variables.node.id);
      invalidateSpace(variables.node.space_id);
    }
  });
  const uploadFileMutation = useMutation({
    mutationFn: ({ parentId, name, file }: { parentId: string; name: string; file: File }) => uploadFile(client, activeSpace!.id, { parentNodeId: parentId, name, file }),
    onSuccess: (response) => {
      invalidateSpace(response.node.space_id);
      openInActiveGroup(response.node);
    }
  });
  const replaceMetadataMutation = useMutation({
    mutationFn: ({ node, metadata }: { node: RestNode; metadata: Record<string, unknown> }) => replaceMetadata(client, node.space_id, node.id, metadata),
    onSuccess: (node) => {
      invalidateSpace(node.space_id);
      updateGroupsNode(node);
    }
  });

  async function openNode(node: RestNode) {
    openInActiveGroup(node);
    closeMobile();
    if (!activeSpace || node.parent_id === null) return;
    const reveal = await revealNode(client, activeSpace.id, node.id);
    addExpanded(reveal.ancestors.map((ancestor) => ancestor.id));
  }

  function selectSpace(space: Space) {
    setActiveSpaceId(space.id);
    resetGroups();
    closeMobile();
  }

  function parentForCreate(): string | null {
    if (!activeSpace) return null;
    if (!activeNode) return activeSpace.root_node_id;
    return activeNode.kind === "folder" ? activeNode.id : activeNode.parent_id ?? activeSpace.root_node_id;
  }

  function promptCreateSpace() {
    setDialog(newSpaceDialog((name) => createSpaceMutation.mutate(name)));
  }

  function promptRenameSpace() {
    if (!activeSpace) return;
    const space = activeSpace;
    setDialog(renameSpaceDialog(space, (spaceId, name) => updateSpaceMutation.mutate({ spaceId, name })));
  }

  function confirmDeleteSpace() {
    if (!activeSpace) return;
    const space = activeSpace;
    setDialog(deleteSpaceDialog(space, (spaceId) => deleteSpaceMutation.mutate(spaceId)));
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
    setDialog(moveNodeDialog(node, activeSpace, (movedNode, parentId) => moveNodeMutation.mutate({ node: movedNode, parentId })));
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

  async function handleSignOut() {
    try {
      await logout(client);
    } finally {
      clearDevApiKey();
      onSignOut();
    }
  }

  function startPrimaryResize(event: ReactPointerEvent) {
    event.preventDefault();
    const startX = event.clientX;
    const startWidth = primaryWidth;
    const move = (e: PointerEvent) => setPrimaryWidth(startWidth + (e.clientX - startX));
    const up = () => {
      window.removeEventListener("pointermove", move);
      window.removeEventListener("pointerup", up);
      document.body.classList.remove("select-none");
    };
    document.body.classList.add("select-none");
    window.addEventListener("pointermove", move);
    window.addEventListener("pointerup", up);
  }

  return {
    loading: spacesQuery.isLoading,
    error: spacesQuery.isError ? String(spacesQuery.error) : null,
    spaces,
    theme,
    activeSpace,
    activeNode,
    editorGroups,
    activeGroupIndex,
    expandedFolderIds,
    primarySidebarOpen,
    auxiliaryOpen,
    primaryWidth,
    mobileTreeOpen,
    mobileAuxOpen,
    showAuxiliary,
    isMobile,
    settingsOpen,
    dialog,
    actions: {
      addGroup,
      closeGroup,
      focusGroup,
      setGroupMode,
      toggleTheme,
      togglePrimarySidebar,
      toggleAuxiliary,
      toggleMobileTree,
      toggleMobileAux,
      closeMobile,
      setSettingsOpen,
      setDialog,
      openNode,
      selectSpace,
      promptCreateSpace,
      promptRenameSpace,
      confirmDeleteSpace,
      promptCreateNode,
      promptCreateInFolder,
      handleFileSelected,
      uploadInFolder,
      collapseTree,
      promptRenameNode,
      promptMoveNode,
      confirmDeleteNode,
      promptReplaceMetadata,
      handleSignOut,
      toggleFolder,
      startPrimaryResize
    }
  };
}
