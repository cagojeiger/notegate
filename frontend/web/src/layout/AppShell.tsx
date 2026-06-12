import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useEffect, useMemo, useState } from "react";

import { useApiClient } from "../api/ApiProvider";
import { logout } from "../api/auth";
import { uploadFile } from "../api/files";
import { replaceMetadata } from "../api/metadata";
import { createNode, deleteNode, moveNode, revealNode, updateNode } from "../api/nodes";
import { queryKeys } from "../api/queryKeys";
import { createSpace, deleteSpace, listSpaces, updateSpace } from "../api/spaces";
import type { RestNode, Space } from "../api/types";
import { clearDevApiKey } from "../auth/session";
import type { ThemeMode } from "../design/tokens";
import { EditorArea } from "../features/editor/EditorArea";
import { PrimarySidebar } from "../features/nodes/PrimarySidebar";
import { ActivityRail } from "../features/spaces/ActivityRail";
import { AuxiliarySidebar } from "./AuxiliarySidebar";
import { FullScreenStatus } from "./FullScreenStatus";
import { StatusBar } from "./StatusBar";
import { TitleBar } from "./TitleBar";

type AppShellProps = {
  onSignOut: () => void;
};

function initialTheme(): ThemeMode {
  const stored = window.localStorage.getItem("notegate.theme");
  if (stored === "light" || stored === "dark") return stored;
  return window.matchMedia("(prefers-color-scheme: dark)").matches ? "dark" : "light";
}

export function AppShell({ onSignOut }: AppShellProps) {
  const client = useApiClient();
  const queryClient = useQueryClient();
  const spacesQuery = useQuery({ queryKey: queryKeys.spaces, queryFn: () => listSpaces(client) });
  const spaces = spacesQuery.data?.spaces ?? [];
  const [theme, setTheme] = useState<ThemeMode>(initialTheme);
  const [activeSpaceId, setActiveSpaceId] = useState<string | null>(() => window.localStorage.getItem("notegate.lastActiveSpaceId"));
  const [activeNode, setActiveNode] = useState<RestNode | null>(null);
  const [expandedFolderIds, setExpandedFolderIds] = useState<Set<string>>(() => new Set());
  const [primarySidebarOpen, setPrimarySidebarOpen] = useState(true);
  const [auxiliaryOpen, setAuxiliaryOpen] = useState(true);


  const activeSpace = useMemo(() => spaces.find((space) => space.id === activeSpaceId) ?? spaces[0] ?? null, [activeSpaceId, spaces]);
  const showAuxiliary = auxiliaryOpen && activeNode !== null;

  useEffect(() => {
    document.documentElement.dataset.theme = theme;
    window.localStorage.setItem("notegate.theme", theme);
  }, [theme]);

  useEffect(() => {
    if (!activeSpace) return;
    setActiveSpaceId(activeSpace.id);
    window.localStorage.setItem("notegate.lastActiveSpaceId", activeSpace.id);
    setExpandedFolderIds((prev) => new Set(prev).add(activeSpace.root_node_id));
  }, [activeSpace]);

  function invalidateSpace(spaceId: string) {
    void queryClient.invalidateQueries({ queryKey: queryKeys.spaces });
    void queryClient.invalidateQueries({ queryKey: ["spaces", spaceId] });
  }

  const createSpaceMutation = useMutation({
    mutationFn: (name: string) => createSpace(client, name),
    onSuccess: (space) => {
      void queryClient.invalidateQueries({ queryKey: queryKeys.spaces });
      setActiveSpaceId(space.id);
      setActiveNode(null);
    }
  });
  const updateSpaceMutation = useMutation({
    mutationFn: ({ spaceId, name }: { spaceId: string; name: string }) => updateSpace(client, spaceId, { name }),
    onSuccess: () => void queryClient.invalidateQueries({ queryKey: queryKeys.spaces })
  });
  const deleteSpaceMutation = useMutation({
    mutationFn: (spaceId: string) => deleteSpace(client, spaceId),
    onSuccess: () => {
      setActiveNode(null);
      setActiveSpaceId(null);
      void queryClient.invalidateQueries({ queryKey: queryKeys.spaces });
    }
  });
  const createNodeMutation = useMutation({
    mutationFn: ({ parentId, kind, name, content }: { parentId: string; kind: "folder" | "text"; name: string; content?: string }) =>
      createNode(client, activeSpace!.id, { parent_id: parentId, kind, name, content }),
    onSuccess: (node) => {
      invalidateSpace(node.space_id);
      setExpandedFolderIds((prev) => new Set(prev).add(node.parent_id ?? activeSpace!.root_node_id));
      setActiveNode(node);
    }
  });
  const updateNodeMutation = useMutation({
    mutationFn: ({ node, name }: { node: RestNode; name: string }) => updateNode(client, node.space_id, node.id, { name }),
    onSuccess: (node) => {
      invalidateSpace(node.space_id);
      setActiveNode(node);
    }
  });
  const moveNodeMutation = useMutation({
    mutationFn: ({ node, parentId }: { node: RestNode; parentId: string }) =>
      moveNode(client, node.space_id, node.id, { new_parent_id: parentId, expected_parent_id: node.parent_id }),
    onSuccess: (node) => {
      invalidateSpace(node.space_id);
      setActiveNode(node);
    }
  });
  const deleteNodeMutation = useMutation({
    mutationFn: ({ node, recursive }: { node: RestNode; recursive: boolean }) => deleteNode(client, node.space_id, node.id, recursive),
    onSuccess: (_, variables) => {
      setActiveNode(null);
      invalidateSpace(variables.node.space_id);
    }
  });
  const uploadFileMutation = useMutation({
    mutationFn: ({ parentId, name, file }: { parentId: string; name: string; file: File }) => uploadFile(client, activeSpace!.id, { parentNodeId: parentId, name, file }),
    onSuccess: (response) => {
      invalidateSpace(response.node.space_id);
      setActiveNode(response.node);
    }
  });
  const replaceMetadataMutation = useMutation({
    mutationFn: ({ node, metadata }: { node: RestNode; metadata: Record<string, unknown> }) => replaceMetadata(client, node.space_id, node.id, metadata),
    onSuccess: (node) => {
      invalidateSpace(node.space_id);
      setActiveNode(node);
    }
  });

  async function openNode(node: RestNode) {
    setActiveNode(node);
    if (!activeSpace || node.parent_id === null) return;
    const reveal = await revealNode(client, activeSpace.id, node.id);
    setExpandedFolderIds((prev) => {
      const next = new Set(prev);
      for (const ancestor of reveal.ancestors) next.add(ancestor.id);
      return next;
    });
  }

  function selectSpace(space: Space) {
    setActiveSpaceId(space.id);
    setActiveNode(null);
  }

  function toggleFolder(nodeId: string) {
    setExpandedFolderIds((prev) => {
      const next = new Set(prev);
      if (next.has(nodeId)) next.delete(nodeId);
      else next.add(nodeId);
      return next;
    });
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

  function promptRenameNode() {
    if (!activeNode || activeNode.parent_id === null) return;
    const name = window.prompt("New node name", activeNode.name);
    if (name?.trim() && name.trim() !== activeNode.name) updateNodeMutation.mutate({ node: activeNode, name: name.trim() });
  }

  function promptMoveNode() {
    if (!activeNode || activeNode.parent_id === null) return;
    const parentId = window.prompt("Destination parent node id", activeNode.parent_id);
    if (parentId?.trim()) moveNodeMutation.mutate({ node: activeNode, parentId: parentId.trim() });
  }

  function confirmDeleteNode() {
    if (!activeNode || activeNode.parent_id === null) return;
    const recursive = activeNode.kind === "folder";
    if (window.confirm(`Delete '${activeNode.name}'${recursive ? " recursively" : ""}?`)) deleteNodeMutation.mutate({ node: activeNode, recursive });
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

  if (spacesQuery.isLoading) return <FullScreenStatus label="Loading spaces" />;
  if (spacesQuery.isError) return <FullScreenStatus label="Could not load spaces" detail={String(spacesQuery.error)} />;

  return (
    <div className="flex h-full flex-col overflow-hidden bg-bg text-text">
      <TitleBar activeSpace={activeSpace} theme={theme} primarySidebarOpen={primarySidebarOpen} auxiliaryOpen={showAuxiliary} onToggleTheme={() => setTheme(theme === "light" ? "dark" : "light")} onTogglePrimarySidebar={() => setPrimarySidebarOpen((open) => !open)} onToggleAuxiliary={() => setAuxiliaryOpen((open) => !open)} />
      <main className={`grid min-h-0 flex-1 border-y border-border ${primarySidebarOpen ? (showAuxiliary ? "grid-cols-[52px_300px_minmax(0,1fr)_320px]" : "grid-cols-[52px_300px_minmax(0,1fr)]") : (showAuxiliary ? "grid-cols-[52px_minmax(0,1fr)_320px]" : "grid-cols-[52px_minmax(0,1fr)]")}`}>
        <ActivityRail spaces={spaces} activeSpace={activeSpace} onSelectSpace={selectSpace} onCreateSpace={promptCreateSpace} onSignOut={handleSignOut} />
        {primarySidebarOpen ? <PrimarySidebar
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
        /> : null}
        <EditorArea
          activeNode={activeNode}
          activeSpace={activeSpace}
          onCreateFolder={() => promptCreateNode("folder")}
          onCreateText={() => promptCreateNode("text")}
          onFileSelected={handleFileSelected}
          onRenameNode={promptRenameNode}
          onMoveNode={promptMoveNode}
          onDeleteNode={confirmDeleteNode}
        />
        {showAuxiliary ? <AuxiliarySidebar activeNode={activeNode} onReplaceMetadata={promptReplaceMetadata} /> : null}
      </main>
      <StatusBar activeSpace={activeSpace} />
    </div>
  );
}
