import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useEffect, useMemo, type PointerEvent as ReactPointerEvent } from "react";

import { useApiClient } from "../api/ApiProvider";
import { logout } from "../api/auth";
import { uploadFile } from "../api/files";
import { replaceMetadata } from "../api/metadata";
import { createNode, deleteNode, moveNode, revealNode, updateNode } from "../api/nodes";
import { queryKeys } from "../api/queryKeys";
import { createSpace, deleteSpace, listSpaces, updateSpace } from "../api/spaces";
import type { RestNode, Space } from "../api/types";
import { clearDevApiKey } from "../auth/session";
import { persistLastSpace, persistTheme, useUiStore } from "../stores/uiStore";
import { useIsMobile } from "../shared/hooks/useMediaQuery";
import { EditorArea } from "../features/editor/EditorArea";
import { PrimarySidebar } from "../features/nodes/PrimarySidebar";
import { ActivityRail } from "../features/spaces/ActivityRail";
import { MobileSpaceBar } from "../features/spaces/MobileSpaceBar";
import { AuxiliarySidebar } from "./AuxiliarySidebar";
import { FullScreenStatus } from "./FullScreenStatus";
import { StatusBar } from "./StatusBar";
import { TitleBar } from "./TitleBar";

type AppShellProps = {
  onSignOut: () => void;
};

export function AppShell({ onSignOut }: AppShellProps) {
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
  const updateGroupsNode = useUiStore((state) => state.updateGroupsNode);
  const clearGroupsWithNode = useUiStore((state) => state.clearGroupsWithNode);
  const resetGroups = useUiStore((state) => state.resetGroups);
  const toggleFolder = useUiStore((state) => state.toggleFolder);
  const addExpanded = useUiStore((state) => state.addExpanded);
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
    const name = window.prompt("Space name");
    if (name?.trim()) createSpaceMutation.mutate(name.trim());
  }

  function promptRenameSpace() {
    if (!activeSpace) return;
    const name = window.prompt("New space name", activeSpace.name);
    if (name?.trim() && name.trim() !== activeSpace.name) updateSpaceMutation.mutate({ spaceId: activeSpace.id, name: name.trim() });
  }

  function confirmDeleteSpace() {
    if (!activeSpace) return;
    if (window.confirm(`Delete space '${activeSpace.name}'?`)) deleteSpaceMutation.mutate(activeSpace.id);
  }

  function promptCreateNode(kind: "folder" | "text") {
    const parentId = parentForCreate();
    if (!parentId) return;
    const name = window.prompt(`${kind} name`);
    if (!name?.trim()) return;
    createNodeMutation.mutate({ parentId, kind, name: name.trim(), content: kind === "text" ? "" : undefined });
  }

  function promptRenameNode(node: RestNode) {
    if (node.parent_id === null) return;
    const name = window.prompt("New node name", node.name);
    if (name?.trim() && name.trim() !== node.name) updateNodeMutation.mutate({ node, name: name.trim() });
  }

  function promptMoveNode(node: RestNode) {
    if (node.parent_id === null) return;
    const parentId = window.prompt("Destination parent node id", node.parent_id);
    if (parentId?.trim()) moveNodeMutation.mutate({ node, parentId: parentId.trim() });
  }

  function confirmDeleteNode(node: RestNode) {
    if (node.parent_id === null) return;
    const recursive = node.kind === "folder";
    if (window.confirm(`Delete '${node.name}'${recursive ? " recursively" : ""}?`)) deleteNodeMutation.mutate({ node, recursive });
  }

  function handleFileSelected(file: File | null) {
    const parentId = parentForCreate();
    if (!file || !parentId) return;
    const name = window.prompt("File node name", file.name);
    if (name?.trim()) uploadFileMutation.mutate({ parentId, name: name.trim(), file });
  }

  function promptReplaceMetadata() {
    if (!activeNode) return;
    const raw = window.prompt("Metadata JSON", JSON.stringify(activeNode.metadata, null, 2));
    if (!raw) return;
    try {
      const metadata = JSON.parse(raw) as Record<string, unknown>;
      replaceMetadataMutation.mutate({ node: activeNode, metadata });
    } catch {
      window.alert("Metadata must be valid JSON");
    }
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

  if (spacesQuery.isLoading) return <FullScreenStatus label="Loading spaces" />;
  if (spacesQuery.isError) return <FullScreenStatus label="Could not load spaces" detail={String(spacesQuery.error)} />;

  return (
    <div className="flex h-full flex-col overflow-hidden bg-bg text-text">
      <TitleBar
        activeSpace={activeSpace}
        theme={theme}
        primarySidebarOpen={isMobile ? mobileTreeOpen : primarySidebarOpen}
        auxiliaryOpen={isMobile ? mobileAuxOpen : showAuxiliary}
        editorGroupCount={editorGroups.length}
        onAddGroup={addGroup}
        onToggleTheme={toggleTheme}
        onTogglePrimarySidebar={isMobile ? toggleMobileTree : togglePrimarySidebar}
        onToggleAuxiliary={isMobile ? toggleMobileAux : toggleAuxiliary}
      />
      <main className="relative flex min-h-0 flex-1 border-y border-border">
        <ActivityRail spaces={spaces} activeSpace={activeSpace} onSelectSpace={selectSpace} onCreateSpace={promptCreateSpace} onSignOut={handleSignOut} />
        <div
          style={isMobile ? undefined : { width: primaryWidth }}
          className={`min-h-0 max-md:fixed max-md:left-0 max-md:bottom-0 max-md:top-12 max-md:z-40 max-md:flex max-md:w-[85%] max-md:max-w-[320px] max-md:shadow-2xl max-md:transition-transform ${mobileTreeOpen ? "max-md:translate-x-0" : "max-md:-translate-x-full"} ${primarySidebarOpen ? "md:flex md:shrink-0" : "md:hidden"}`}
        >
          <PrimarySidebar
            activeSpace={activeSpace}
            activeNodeId={activeNode?.id ?? null}
            expandedFolderIds={expandedFolderIds}
            onToggleFolder={toggleFolder}
            onOpenNode={openNode}
            onCreateFolder={() => promptCreateNode("folder")}
            onCreateText={() => promptCreateNode("text")}
            onFileSelected={handleFileSelected}
            onRenameSpace={promptRenameSpace}
            onDeleteSpace={confirmDeleteSpace}
            onRenameNode={promptRenameNode}
            onDeleteNode={confirmDeleteNode}
          />
        </div>
        {primarySidebarOpen ? (
          <div onPointerDown={startPrimaryResize} className="hidden w-1 shrink-0 cursor-col-resize bg-border/40 transition-colors hover:bg-primary/40 md:block" aria-hidden="true" />
        ) : null}
        <EditorArea
          groups={editorGroups}
          activeGroupIndex={activeGroupIndex}
          activeSpace={activeSpace}
          onFocusGroup={focusGroup}
          onCloseGroup={closeGroup}
          onCreateFolder={() => promptCreateNode("folder")}
          onCreateText={() => promptCreateNode("text")}
          onFileSelected={handleFileSelected}
          onRenameNode={promptRenameNode}
          onMoveNode={promptMoveNode}
          onDeleteNode={confirmDeleteNode}
        />
        <div
          className={`min-h-0 max-md:fixed max-md:right-0 max-md:bottom-0 max-md:top-12 max-md:z-40 max-md:flex max-md:w-[85%] max-md:max-w-[340px] max-md:shadow-2xl max-md:transition-transform ${mobileAuxOpen ? "max-md:translate-x-0" : "max-md:translate-x-full"} ${showAuxiliary ? "md:flex md:w-[320px] md:shrink-0" : "md:hidden"}`}
        >
          <AuxiliarySidebar activeNode={activeNode} onReplaceMetadata={promptReplaceMetadata} />
        </div>
        {mobileTreeOpen || mobileAuxOpen ? (
          <button type="button" aria-label="Close panel" onClick={closeMobile} className="fixed inset-x-0 bottom-0 top-12 z-30 bg-black/40 md:hidden" />
        ) : null}
      </main>
      <MobileSpaceBar spaces={spaces} activeSpace={activeSpace} onSelectSpace={selectSpace} onCreateSpace={promptCreateSpace} onSignOut={handleSignOut} />
      <StatusBar activeSpace={activeSpace} />
      <Toast />
    </div>
  );
}

function Toast() {
  const toast = useUiStore((state) => state.toast);
  const clearToast = useUiStore((state) => state.clearToast);
  useEffect(() => {
    if (!toast) return;
    const timer = window.setTimeout(clearToast, 2000);
    return () => window.clearTimeout(timer);
  }, [toast, clearToast]);
  if (!toast) return null;
  return (
    <div className="pointer-events-none fixed inset-x-0 bottom-20 z-50 flex justify-center md:bottom-10">
      <div className="rounded-full border border-border bg-panel-strong px-4 py-2 text-sm text-text shadow-lg">{toast}</div>
    </div>
  );
}
