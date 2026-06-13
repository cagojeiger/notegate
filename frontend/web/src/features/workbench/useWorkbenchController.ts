import { useEffect, useMemo, useState, type PointerEvent as ReactPointerEvent } from "react";

import type { RestNode, Space } from "../../api/types";
import { clearDevApiKey } from "../../auth/session";
import { useIsMobile } from "../../shared/hooks/useMediaQuery";
import { persistLastSpace, persistTheme, useUiStore } from "../../stores/uiStore";
import type { AppDialog } from "../../layout/dialogs/DialogHost";
import { createNodeDialog, deleteNodeDialog, deleteSpaceDialog, metadataDialog, moveNodeDialog, newSpaceDialog, renameNodeDialog, renameSpaceDialog, uploadFileDialog } from "../../layout/dialogs/appDialogs";
import { useCreateNodeMutation, useCreateSpaceMutation, useDeleteNodeMutation, useDeleteSpaceMutation, useLogout, useMoveNodeMutation, useReplaceMetadataMutation, useRevealNode, useSpacesQuery, useUpdateNodeMutation, useUpdateSpaceMutation, useUploadFileMutation } from "./useWorkbenchQueries";

type WorkbenchControllerProps = {
  onSignOut: () => void;
};

export function useWorkbenchController({ onSignOut }: WorkbenchControllerProps) {
  const spacesQuery = useSpacesQuery();
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

  const createSpaceMutation = useCreateSpaceMutation((space) => {
    setActiveSpaceId(space.id);
    resetGroups();
  });
  const updateSpaceMutation = useUpdateSpaceMutation();
  const deleteSpaceMutation = useDeleteSpaceMutation(() => {
    resetGroups();
    setActiveSpaceId(null);
  });
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
  const logoutSession = useLogout();

  async function openNode(node: RestNode) {
    openInActiveGroup(node);
    closeMobile();
    if (!activeSpace || node.parent_id === null) return;
    const reveal = await revealNodeInSpace(activeSpace.id, node.id);
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
      await logoutSession();
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
