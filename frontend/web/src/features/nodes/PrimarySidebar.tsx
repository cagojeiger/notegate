import { ChevronRight, Database, FileText, Folder, MoreHorizontal, Plus, Trash2, Upload } from "lucide-react";
import { useState, type ReactNode } from "react";
import { useQuery } from "@tanstack/react-query";

import { useApiClient } from "../../api/ApiProvider";
import { listChildren, listNodes } from "../../api/nodes";
import { queryKeys } from "../../api/queryKeys";
import type { RestNode, Space } from "../../api/types";
import { IconButton, MenuButton } from "../../shared/ui";

export function PrimarySidebar({
  activeSpace,
  activeNodeId,
  expandedFolderIds,
  onToggleFolder,
  onOpenNode,
  onCreateFolder,
  onCreateText,
  onFileSelected,
  onRenameSpace,
  onDeleteSpace
}: {
  activeSpace: Space | null;
  activeNodeId: string | null;
  expandedFolderIds: Set<string>;
  onToggleFolder: (nodeId: string) => void;
  onOpenNode: (node: RestNode) => void;
  onCreateFolder: () => void;
  onCreateText: () => void;
  onFileSelected: (file: File | null) => void;
  onRenameSpace: () => void;
  onDeleteSpace: () => void;
}) {
  const [createOpen, setCreateOpen] = useState(false);
  const [manageOpen, setManageOpen] = useState(false);
  return (
    <aside className="flex min-h-0 flex-col border-r border-border bg-panel">
      <div className="relative flex h-12 items-center justify-between border-b border-border px-3">
        <div className="min-w-0">
          <div className="truncate text-sm font-semibold">{activeSpace?.name ?? "No space"}</div>
        </div>
        <div className="flex items-center gap-1">
          <IconButton label="Create node" onClick={() => setCreateOpen((open) => !open)}><Plus size={15} /></IconButton>
          <IconButton label="Manage space" onClick={() => setManageOpen((open) => !open)}><MoreHorizontal size={15} /></IconButton>
        </div>
        {createOpen ? <CreateMenu onCreateFolder={onCreateFolder} onCreateText={onCreateText} onFileSelected={onFileSelected} onClose={() => setCreateOpen(false)} /> : null}
        {manageOpen ? <SpaceMenu onRenameSpace={onRenameSpace} onDeleteSpace={onDeleteSpace} onClose={() => setManageOpen(false)} /> : null}
      </div>
      {activeSpace ? (
        <div className="grid min-h-0 flex-1 grid-rows-[2fr_6px_1fr]">
          <section className="min-h-0 overflow-y-auto px-3 py-3">
            <SectionTitle icon={<Folder size={13} />} label="Tree" />
            <div className="mt-2 space-y-1">
              <RootTree activeSpace={activeSpace} activeNodeId={activeNodeId} expandedFolderIds={expandedFolderIds} onToggleFolder={onToggleFolder} onOpenNode={onOpenNode} />
            </div>
          </section>
          <div className="cursor-row-resize border-y border-border bg-surface" />
          <section className="min-h-0 overflow-y-auto px-3 py-3">
            <SectionTitle icon={<FileText size={13} />} label="Recent" />
            <RecentList activeSpace={activeSpace} activeNodeId={activeNodeId} onOpenNode={onOpenNode} />
          </section>
        </div>
      ) : (
        <div className="p-4 text-sm text-muted">Create a space to start.</div>
      )}
    </aside>
  );
}

function CreateMenu({ onCreateFolder, onCreateText, onFileSelected, onClose }: { onCreateFolder: () => void; onCreateText: () => void; onFileSelected: (file: File | null) => void; onClose: () => void }) {
  function run(action: () => void) {
    action();
    onClose();
  }

  return (
    <div className="absolute right-3 top-11 z-20 w-44 rounded-xl border border-border bg-surface p-1 text-sm shadow-[var(--ng-focus-shadow)]">
      <MenuButton onClick={() => run(onCreateFolder)}><Folder size={14} /> New folder</MenuButton>
      <MenuButton onClick={() => run(onCreateText)}><FileText size={14} /> New text</MenuButton>
      <label className="flex cursor-pointer items-center gap-2 rounded-lg px-3 py-2 text-muted hover:bg-panel hover:text-text">
        <Upload size={14} /> Upload file
        <input
          className="hidden"
          type="file"
          onChange={(event) => {
            onFileSelected(event.target.files?.[0] ?? null);
            onClose();
          }}
        />
      </label>
    </div>
  );
}

function SpaceMenu({ onRenameSpace, onDeleteSpace, onClose }: { onRenameSpace: () => void; onDeleteSpace: () => void; onClose: () => void }) {
  function run(action: () => void) {
    action();
    onClose();
  }

  return (
    <div className="absolute right-3 top-11 z-20 w-44 rounded-xl border border-border bg-surface p-1 text-sm shadow-[var(--ng-focus-shadow)]">
      <MenuButton onClick={() => run(onRenameSpace)}>Rename space</MenuButton>
      <MenuButton danger onClick={() => run(onDeleteSpace)}><Trash2 size={14} /> Delete space</MenuButton>
    </div>
  );
}

function RootTree(props: { activeSpace: Space; activeNodeId: string | null; expandedFolderIds: Set<string>; onToggleFolder: (nodeId: string) => void; onOpenNode: (node: RestNode) => void }) {
  const rootNode: RestNode = {
    id: props.activeSpace.root_node_id,
    space_id: props.activeSpace.id,
    parent_id: null,
    name: "/",
    kind: "folder",
    path: "/",
    sort_order: 0,
    metadata: {},
    has_children: true,
    created_by: { id: "", kind: "user", display_name: "" },
    updated_by: { id: "", kind: "user", display_name: "" },
    created_at: props.activeSpace.created_at,
    updated_at: props.activeSpace.updated_at
  };
  return <TreeNode node={rootNode} depth={0} {...props} />;
}

function TreeNode({ node, depth, activeSpace, activeNodeId, expandedFolderIds, onToggleFolder, onOpenNode }: { node: RestNode; depth: number; activeSpace: Space; activeNodeId: string | null; expandedFolderIds: Set<string>; onToggleFolder: (nodeId: string) => void; onOpenNode: (node: RestNode) => void }) {
  const isExpanded = expandedFolderIds.has(node.id);
  const childrenQuery = useNodeChildren(activeSpace.id, node.id, node.kind === "folder" && isExpanded);
  return (
    <div>
      <NodeRow node={node} depth={depth} selected={activeNodeId === node.id} expanded={isExpanded} onToggleFolder={onToggleFolder} onOpenNode={onOpenNode} />
      {node.kind === "folder" && isExpanded ? (
        <div>
          {childrenQuery.isLoading ? <div className="px-8 py-1 text-xs text-muted">Loading…</div> : null}
          {childrenQuery.data?.children.map((child) => (
            <TreeNode key={child.id} node={child} depth={depth + 1} activeSpace={activeSpace} activeNodeId={activeNodeId} expandedFolderIds={expandedFolderIds} onToggleFolder={onToggleFolder} onOpenNode={onOpenNode} />
          ))}
        </div>
      ) : null}
    </div>
  );
}

function useNodeChildren(spaceId: string, nodeId: string, enabled: boolean) {
  const client = useApiClient();
  return useQuery({ queryKey: queryKeys.children(spaceId, nodeId), queryFn: () => listChildren(client, spaceId, nodeId), enabled });
}

function RecentList({ activeSpace, activeNodeId, onOpenNode }: { activeSpace: Space; activeNodeId: string | null; onOpenNode: (node: RestNode) => void }) {
  const client = useApiClient();
  const recentQuery = useQuery({ queryKey: queryKeys.recent(activeSpace.id), queryFn: () => listNodes(client, activeSpace.id, { sort: "updated_at_desc" }) });
  if (recentQuery.isLoading) return <div className="mt-2 text-xs text-muted">Loading recent…</div>;
  if (recentQuery.isError) return <div className="mt-2 rounded-lg border border-border bg-surface p-3 text-xs text-muted">Recent is unavailable for this server build.</div>;
  const nodes = recentQuery.data?.nodes ?? [];
  if (nodes.length === 0) return <div className="mt-2 text-xs text-muted">No recent nodes yet.</div>;
  return <div className="mt-2 space-y-1">{nodes.map((node) => <NodeRow key={node.id} node={node} depth={0} selected={activeNodeId === node.id} onOpenNode={onOpenNode} />)}</div>;
}

function SectionTitle({ icon, label }: { icon: ReactNode; label: string }) {
  return <div className="flex items-center gap-2 text-[11px] font-semibold uppercase tracking-wide text-muted">{icon}{label}</div>;
}

function NodeRow({ node, depth, selected, expanded, onToggleFolder, onOpenNode }: { node: RestNode; depth: number; selected: boolean; expanded?: boolean; onToggleFolder?: (nodeId: string) => void; onOpenNode: (node: RestNode) => void }) {
  const Icon = node.kind === "folder" ? Folder : node.kind === "file" ? Database : FileText;
  function handleOpen() {
    if (node.kind === "folder") onToggleFolder?.(node.id);
    onOpenNode(node);
  }
  return (
    <div className={`group flex w-full items-center gap-1 rounded-lg py-1.5 pr-2 text-sm transition ${selected ? "bg-panel-strong text-text" : "text-muted hover:bg-surface hover:text-text"}`} style={{ paddingLeft: `${8 + depth * 14}px` }}>
      {node.kind === "folder" ? <button className="grid size-4 place-items-center" onClick={() => onToggleFolder?.(node.id)}><ChevronRight size={13} className={expanded ? "rotate-90 transition" : "transition"} /></button> : <span className="size-4" />}
      <button className="flex min-w-0 flex-1 items-center gap-2 text-left" onClick={handleOpen}>
        <Icon size={15} />
        <span className="truncate">{node.name}</span>
      </button>
    </div>
  );
}
