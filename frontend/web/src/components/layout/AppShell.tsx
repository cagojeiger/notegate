import { useQuery } from "@tanstack/react-query";
import { ChevronRight, Database, FileText, Folder, LayoutPanelLeft, Loader2, PanelRight, Plus, Search, Settings } from "lucide-react";
import { ReactNode, useEffect, useMemo, useState } from "react";

import { useApiClient } from "../../api/ApiProvider";
import { getMe } from "../../api/me";
import { listChildren, listNodes, revealNode } from "../../api/nodes";
import { queryKeys } from "../../api/queryKeys";
import { listSpaces } from "../../api/spaces";
import type { RestNode, Space } from "../../api/types";
import { clearDevApiKey } from "../../auth/session";

type AppShellProps = {
  onSignOut: () => void;
};

export function AppShell({ onSignOut }: AppShellProps) {
  const client = useApiClient();
  const spacesQuery = useQuery({ queryKey: queryKeys.spaces, queryFn: () => listSpaces(client) });
  const meQuery = useQuery({ queryKey: queryKeys.me, queryFn: () => getMe(client) });
  const spaces = spacesQuery.data?.spaces ?? [];
  const [activeSpaceId, setActiveSpaceId] = useState<string | null>(() => window.localStorage.getItem("notegate.lastActiveSpaceId"));
  const [activeNode, setActiveNode] = useState<RestNode | null>(null);
  const [expandedFolderIds, setExpandedFolderIds] = useState<Set<string>>(() => new Set());

  const activeSpace = useMemo(() => spaces.find((space) => space.id === activeSpaceId) ?? spaces[0] ?? null, [activeSpaceId, spaces]);

  useEffect(() => {
    if (!activeSpace) return;
    setActiveSpaceId(activeSpace.id);
    window.localStorage.setItem("notegate.lastActiveSpaceId", activeSpace.id);
    setExpandedFolderIds((prev) => new Set(prev).add(activeSpace.root_node_id));
  }, [activeSpace]);

  async function openNode(node: RestNode) {
    setActiveNode(node);
    if (!activeSpace) return;
    if (node.parent_id) {
      const reveal = await revealNode(client, activeSpace.id, node.id);
      setExpandedFolderIds((prev) => {
        const next = new Set(prev);
        for (const ancestor of reveal.ancestors) next.add(ancestor.id);
        return next;
      });
    }
  }

  function toggleFolder(nodeId: string) {
    setExpandedFolderIds((prev) => {
      const next = new Set(prev);
      if (next.has(nodeId)) next.delete(nodeId);
      else next.add(nodeId);
      return next;
    });
  }

  function handleSignOut() {
    clearDevApiKey();
    onSignOut();
  }

  if (spacesQuery.isLoading) {
    return <FullScreenStatus label="Loading spaces" />;
  }

  if (spacesQuery.isError) {
    return <FullScreenStatus label="Could not load spaces" detail={String(spacesQuery.error)} />;
  }

  return (
    <div className="flex h-full flex-col overflow-hidden bg-bg text-text">
      <TitleBar activeSpace={activeSpace} displayName={meQuery.data?.account.display_name} onSignOut={handleSignOut} />
      <main className="grid min-h-0 flex-1 grid-cols-[56px_300px_minmax(0,1fr)_320px] border-y border-border">
        <ActivityRail spaces={spaces} activeSpace={activeSpace} onSelectSpace={(space) => setActiveSpaceId(space.id)} />
        <PrimarySidebar
          activeSpace={activeSpace}
          activeNodeId={activeNode?.id ?? null}
          expandedFolderIds={expandedFolderIds}
          onToggleFolder={toggleFolder}
          onOpenNode={openNode}
        />
        <EditorArea activeNode={activeNode} />
        <AuxiliarySidebar activeNode={activeNode} />
      </main>
      <StatusBar activeSpace={activeSpace} />
    </div>
  );
}

function FullScreenStatus({ label, detail }: { label: string; detail?: string }) {
  return (
    <main className="grid h-full place-items-center bg-bg text-text">
      <div className="rounded-xl border border-border bg-panel p-6 text-center">
        <Loader2 className="mx-auto mb-3 animate-spin text-primary" size={24} />
        <div className="font-semibold">{label}</div>
        {detail ? <div className="mt-2 max-w-md text-sm text-muted">{detail}</div> : null}
      </div>
    </main>
  );
}

function TitleBar({ activeSpace, displayName, onSignOut }: { activeSpace: Space | null; displayName?: string; onSignOut: () => void }) {
  return (
    <header className="flex h-12 items-center justify-between border-b border-border bg-surface px-3">
      <div className="flex items-center gap-2 font-semibold">
        <div className="grid size-7 place-items-center rounded-lg bg-primary text-sm font-bold text-bg">N</div>
        <span>Notegate</span>
        {activeSpace ? <span className="text-sm text-muted">/ {activeSpace.name}</span> : null}
      </div>
      <div className="flex items-center gap-2 text-muted">
        <span className="hidden text-xs md:inline">{displayName}</span>
        <button className="rounded-md border border-border bg-panel p-1.5" aria-label="Toggle primary sidebar">
          <LayoutPanelLeft size={16} />
        </button>
        <button className="rounded-md border border-border bg-panel p-1.5" aria-label="Add editor group">
          <Plus size={16} />
        </button>
        <button className="rounded-md border border-border bg-panel p-1.5" aria-label="Toggle auxiliary sidebar">
          <PanelRight size={16} />
        </button>
        <button className="rounded-md border border-border bg-panel px-2 py-1 text-xs" onClick={onSignOut}>
          Reset key
        </button>
      </div>
    </header>
  );
}

function ActivityRail({ spaces, activeSpace, onSelectSpace }: { spaces: Space[]; activeSpace: Space | null; onSelectSpace: (space: Space) => void }) {
  return (
    <aside className="flex min-h-0 flex-col border-r border-border bg-surface">
      <div className="flex min-h-0 flex-1 flex-col items-center gap-2 overflow-y-auto py-3">
        {spaces.map((space) => (
          <button
            key={space.id}
            onClick={() => onSelectSpace(space)}
            title={space.name}
            className={`grid size-9 place-items-center rounded-xl border text-sm font-semibold ${
              activeSpace?.id === space.id ? "border-primary bg-panel-strong text-text" : "border-border bg-panel text-muted"
            }`}
          >
            {space.name.slice(0, 1).toUpperCase()}
          </button>
        ))}
        <button className="grid size-9 place-items-center rounded-xl border border-dashed border-border text-muted" aria-label="Add space">
          <Plus size={16} />
        </button>
      </div>
      <div className="border-t border-border p-2">
        <button className="grid size-10 place-items-center rounded-xl border border-border bg-panel text-muted" aria-label="Settings">
          <Settings size={16} />
        </button>
      </div>
    </aside>
  );
}

function PrimarySidebar({
  activeSpace,
  activeNodeId,
  expandedFolderIds,
  onToggleFolder,
  onOpenNode
}: {
  activeSpace: Space | null;
  activeNodeId: string | null;
  expandedFolderIds: Set<string>;
  onToggleFolder: (nodeId: string) => void;
  onOpenNode: (node: RestNode) => void;
}) {
  return (
    <aside className="flex min-h-0 flex-col border-r border-border bg-panel">
      <div className="flex h-12 items-center justify-between border-b border-border px-4">
        <div>
          <div className="text-sm font-semibold">{activeSpace?.name ?? "No space"}</div>
          <div className="text-xs text-muted">active space</div>
        </div>
        <button className="rounded-md border border-border bg-surface p-1.5 text-muted" aria-label="Create node">
          <Plus size={15} />
        </button>
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
            <SectionTitle icon={<Search size={13} />} label="Recent" />
            <RecentList activeSpace={activeSpace} activeNodeId={activeNodeId} onOpenNode={onOpenNode} />
          </section>
        </div>
      ) : (
        <div className="p-4 text-sm text-muted">Create a space to start.</div>
      )}
    </aside>
  );
}

function RootTree(props: {
  activeSpace: Space;
  activeNodeId: string | null;
  expandedFolderIds: Set<string>;
  onToggleFolder: (nodeId: string) => void;
  onOpenNode: (node: RestNode) => void;
}) {
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

function TreeNode({
  node,
  depth,
  activeSpace,
  activeNodeId,
  expandedFolderIds,
  onToggleFolder,
  onOpenNode
}: {
  node: RestNode;
  depth: number;
  activeSpace: Space;
  activeNodeId: string | null;
  expandedFolderIds: Set<string>;
  onToggleFolder: (nodeId: string) => void;
  onOpenNode: (node: RestNode) => void;
}) {
  const isExpanded = expandedFolderIds.has(node.id);
  const childrenQuery = useNodeChildren(activeSpace.id, node.id, node.kind === "folder" && isExpanded);
  const isSelected = activeNodeId === node.id;

  return (
    <div>
      <NodeRow node={node} depth={depth} selected={isSelected} expanded={isExpanded} onToggleFolder={onToggleFolder} onOpenNode={onOpenNode} />
      {node.kind === "folder" && isExpanded ? (
        <div>
          {childrenQuery.isLoading ? <div className="px-8 py-1 text-xs text-muted">Loading…</div> : null}
          {childrenQuery.data?.children.map((child) => (
            <TreeNode
              key={child.id}
              node={child}
              depth={depth + 1}
              activeSpace={activeSpace}
              activeNodeId={activeNodeId}
              expandedFolderIds={expandedFolderIds}
              onToggleFolder={onToggleFolder}
              onOpenNode={onOpenNode}
            />
          ))}
        </div>
      ) : null}
    </div>
  );
}

function useNodeChildren(spaceId: string, nodeId: string, enabled: boolean) {
  const client = useApiClient();
  return useQuery({
    queryKey: queryKeys.children(spaceId, nodeId),
    queryFn: () => listChildren(client, spaceId, nodeId),
    enabled
  });
}

function RecentList({ activeSpace, activeNodeId, onOpenNode }: { activeSpace: Space; activeNodeId: string | null; onOpenNode: (node: RestNode) => void }) {
  const client = useApiClient();
  const recentQuery = useQuery({ queryKey: queryKeys.recent(activeSpace.id), queryFn: () => listNodes(client, activeSpace.id, { sort: "updated_at_desc" }) });

  if (recentQuery.isLoading) return <div className="mt-2 text-xs text-muted">Loading recent…</div>;
  if (recentQuery.isError) return <div className="mt-2 text-xs text-danger">Could not load recent.</div>;

  return (
    <div className="mt-2 space-y-1">
      {recentQuery.data?.nodes.map((node) => (
        <NodeRow key={node.id} node={node} depth={0} selected={activeNodeId === node.id} onOpenNode={onOpenNode} />
      ))}
    </div>
  );
}

function SectionTitle({ icon, label }: { icon: ReactNode; label: string }) {
  return (
    <div className="flex items-center gap-2 text-xs font-semibold uppercase tracking-wide text-muted">
      {icon}
      {label}
    </div>
  );
}

function NodeRow({
  node,
  depth,
  selected,
  expanded,
  onToggleFolder,
  onOpenNode
}: {
  node: RestNode;
  depth: number;
  selected: boolean;
  expanded?: boolean;
  onToggleFolder?: (nodeId: string) => void;
  onOpenNode: (node: RestNode) => void;
}) {
  const Icon = node.kind === "folder" ? Folder : node.kind === "file" ? Database : FileText;
  return (
    <div
      className={`group flex w-full items-center gap-1 rounded-md py-1.5 pr-2 text-sm ${selected ? "bg-panel-strong text-text" : "text-muted hover:bg-surface hover:text-text"}`}
      style={{ paddingLeft: `${8 + depth * 14}px` }}
    >
      {node.kind === "folder" ? (
        <button className="grid size-4 place-items-center" onClick={() => onToggleFolder?.(node.id)} aria-label={expanded ? "Collapse folder" : "Expand folder"}>
          <ChevronRight size={13} className={expanded ? "rotate-90 transition" : "transition"} />
        </button>
      ) : (
        <span className="size-4" />
      )}
      <button className="flex min-w-0 flex-1 items-center gap-2 text-left" onClick={() => onOpenNode(node)}>
        <Icon size={15} />
        <span className="truncate">{node.name}</span>
      </button>
    </div>
  );
}

function EditorArea({ activeNode }: { activeNode: RestNode | null }) {
  if (!activeNode) {
    return (
      <section className="grid min-w-0 place-items-center bg-bg text-muted">
        <div className="text-center">
          <FileText className="mx-auto mb-3" size={32} />
          <div className="font-semibold text-text">Open a node</div>
          <p className="mt-2 text-sm">Select an item from Tree or Recent.</p>
        </div>
      </section>
    );
  }

  return (
    <section className="min-w-0 bg-bg">
      <div className="flex h-12 items-center justify-between border-b border-border px-4">
        <div className="flex min-w-0 items-center gap-2 font-semibold">
          {activeNode.kind === "folder" ? <Folder size={17} /> : activeNode.kind === "file" ? <Database size={17} /> : <FileText size={17} />}
          <span className="truncate">{activeNode.name}</span>
        </div>
        {activeNode.kind === "text" ? <button className="rounded-md border border-border bg-panel px-3 py-1 text-sm text-muted">Edit</button> : null}
      </div>
      <article className="mx-auto max-w-3xl px-10 py-14">
        <p className="text-sm text-muted">{activeNode.path}</p>
        <h1 className="mt-4 text-4xl font-semibold tracking-tight">{activeNode.name}</h1>
        <p className="mt-8 leading-7 text-muted">{activeNode.kind === "folder" ? "Folder selected. Use the tree to browse children." : "Content loading is connected in the next phase."}</p>
      </article>
    </section>
  );
}

function AuxiliarySidebar({ activeNode }: { activeNode: RestNode | null }) {
  return (
    <aside className="min-h-0 border-l border-border bg-panel p-3">
      <div className="grid grid-cols-2 rounded-lg bg-surface p-1 text-sm">
        <button className="rounded-md bg-panel-strong px-3 py-1.5 font-medium">Inspector</button>
        <button className="rounded-md px-3 py-1.5 text-muted">Agent</button>
      </div>
      {activeNode ? (
        <div className="mt-4 space-y-3">
          <InspectorCard title="Node">
            <dl className="grid grid-cols-[80px_1fr] gap-y-2 text-sm">
              <dt className="font-semibold text-text">Kind</dt>
              <dd className="text-muted">{activeNode.kind}</dd>
              <dt className="font-semibold text-text">Path</dt>
              <dd className="break-all text-muted">{activeNode.path}</dd>
              <dt className="font-semibold text-text">Updated</dt>
              <dd className="text-muted">{activeNode.updated_at.slice(0, 10)}</dd>
              {activeNode.byte_len !== undefined ? <dt className="font-semibold text-text">Bytes</dt> : null}
              {activeNode.byte_len !== undefined ? <dd className="text-muted">{activeNode.byte_len}</dd> : null}
            </dl>
          </InspectorCard>
          <InspectorCard title="Metadata">
            <pre className="whitespace-pre-wrap font-mono text-xs text-muted">{JSON.stringify(activeNode.metadata, null, 2)}</pre>
          </InspectorCard>
        </div>
      ) : (
        <div className="mt-4 text-sm text-muted">No node selected.</div>
      )}
    </aside>
  );
}

function InspectorCard({ title, children }: { title: string; children: ReactNode }) {
  return (
    <section className="rounded-lg border border-border bg-surface p-4">
      <h3 className="mb-3 text-xs font-bold uppercase tracking-wide text-muted">{title}</h3>
      {children}
    </section>
  );
}

function StatusBar({ activeSpace }: { activeSpace: Space | null }) {
  return (
    <footer className="flex h-7 items-center justify-between border-t border-border bg-surface px-3 text-xs text-muted">
      <span className="flex items-center gap-2"><span className="size-2 rounded-full bg-success" /> ready</span>
      <span>{activeSpace?.name ?? "No space"}</span>
    </footer>
  );
}
