import { ChevronRight, Copy, Database, FileText, Folder, MoreHorizontal, Pencil, Plus, Trash2, Upload } from "lucide-react";
import { useEffect, useRef, useState, type KeyboardEvent as ReactKeyboardEvent, type MouseEvent, type PointerEvent as ReactPointerEvent, type ReactNode } from "react";
import { useInfiniteQuery, useQuery } from "@tanstack/react-query";

import { useApiClient } from "../../api/ApiProvider";
import { listChildren, listNodes } from "../../api/nodes";
import { queryKeys } from "../../api/queryKeys";
import type { RestNode, Space } from "../../api/types";
import { useUiStore } from "../../stores/uiStore";
import { IconButton, MenuButton } from "../../shared/ui";

type NodeContextHandler = (node: RestNode, event: MouseEvent) => void;

type TreeProps = {
  activeSpace: Space;
  activeNodeId: string | null;
  expandedFolderIds: Set<string>;
  onToggleFolder: (nodeId: string) => void;
  onOpenNode: (node: RestNode) => void;
  onNodeContextMenu: NodeContextHandler;
};

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
  onDeleteSpace,
  onRenameNode,
  onDeleteNode
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
  onRenameNode: (node: RestNode) => void;
  onDeleteNode: (node: RestNode) => void;
}) {
  const [createOpen, setCreateOpen] = useState(false);
  const [manageOpen, setManageOpen] = useState(false);
  const [menu, setMenu] = useState<{ x: number; y: number; node: RestNode } | null>(null);
  const treeRatio = useUiStore((state) => state.treeRatio);
  const setTreeRatio = useUiStore((state) => state.setTreeRatio);
  const gridRef = useRef<HTMLDivElement>(null);
  const asideRef = useRef<HTMLElement>(null);
  function onSidebarKeyDown(event: ReactKeyboardEvent) {
    if (event.key !== "ArrowDown" && event.key !== "ArrowUp") return;
    const buttons = Array.from(asideRef.current?.querySelectorAll<HTMLButtonElement>("[data-node-open]") ?? []);
    if (buttons.length === 0) return;
    event.preventDefault();
    const current = document.activeElement as HTMLElement | null;
    const index = current ? buttons.indexOf(current as HTMLButtonElement) : -1;
    const next = event.key === "ArrowDown" ? Math.min(index + 1, buttons.length - 1) : Math.max(index <= 0 ? 0 : index - 1, 0);
    buttons[next]?.focus();
  }
  const onNodeContextMenu: NodeContextHandler = (node, event) => {
    event.preventDefault();
    setMenu({ x: event.clientX, y: event.clientY, node });
  };
  function startTreeResize(event: ReactPointerEvent) {
    event.preventDefault();
    const rect = gridRef.current?.getBoundingClientRect();
    if (!rect) return;
    const move = (e: PointerEvent) => setTreeRatio((e.clientY - rect.top) / rect.height);
    const up = () => {
      window.removeEventListener("pointermove", move);
      window.removeEventListener("pointerup", up);
      document.body.classList.remove("select-none");
    };
    document.body.classList.add("select-none");
    window.addEventListener("pointermove", move);
    window.addEventListener("pointerup", up);
  }
  return (
    <aside ref={asideRef} onKeyDown={onSidebarKeyDown} className="flex h-full w-full min-h-0 flex-col border-r border-border bg-panel">
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
        <div ref={gridRef} className="grid min-h-0 flex-1" style={{ gridTemplateRows: `${treeRatio}fr 6px ${1 - treeRatio}fr` }}>
          <section className="min-h-0 overflow-y-auto px-3 py-3">
            <SectionTitle icon={<Folder size={13} />} label="Tree" />
            <div className="mt-2 space-y-1">
              <RootTree activeSpace={activeSpace} activeNodeId={activeNodeId} expandedFolderIds={expandedFolderIds} onToggleFolder={onToggleFolder} onOpenNode={onOpenNode} onNodeContextMenu={onNodeContextMenu} />
            </div>
          </section>
          <div onPointerDown={startTreeResize} className="cursor-row-resize border-y border-border bg-surface transition-colors hover:bg-primary/30" aria-hidden="true" />
          <section className="min-h-0 overflow-y-auto px-3 py-3">
            <SectionTitle icon={<FileText size={13} />} label="Recent" />
            <RecentList activeSpace={activeSpace} activeNodeId={activeNodeId} onOpenNode={onOpenNode} onNodeContextMenu={onNodeContextMenu} />
          </section>
        </div>
      ) : (
        <div className="p-4 text-sm text-muted">Create a space to start.</div>
      )}
      {menu ? (
        <NodeContextMenu
          menu={menu}
          onClose={() => setMenu(null)}
          onOpenNode={onOpenNode}
          onRenameNode={onRenameNode}
          onDeleteNode={onDeleteNode}
        />
      ) : null}
    </aside>
  );
}

function NodeContextMenu({ menu, onClose, onOpenNode, onRenameNode, onDeleteNode }: { menu: { x: number; y: number; node: RestNode }; onClose: () => void; onOpenNode: (node: RestNode) => void; onRenameNode: (node: RestNode) => void; onDeleteNode: (node: RestNode) => void }) {
  const showToast = useUiStore((state) => state.showToast);
  const { node } = menu;
  const isRoot = node.parent_id === null;
  useEffect(() => {
    function onKey(event: KeyboardEvent) {
      if (event.key === "Escape") onClose();
    }
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [onClose]);
  function run(action: () => void) {
    action();
    onClose();
  }
  function copyPath() {
    void navigator.clipboard?.writeText(node.path);
    showToast("Path copied");
  }
  const left = Math.min(menu.x, window.innerWidth - 196);
  const top = Math.min(menu.y, window.innerHeight - 176);
  return (
    <>
      <div className="fixed inset-0 z-40" onClick={onClose} onContextMenu={(event) => { event.preventDefault(); onClose(); }} />
      <div className="fixed z-50 w-44 rounded-xl border border-border bg-surface p-1 text-sm shadow-[var(--ng-focus-shadow)]" style={{ left, top }} role="menu">
        <div className="truncate px-3 py-1 text-xs text-muted">{node.path}</div>
        <MenuButton onClick={() => run(() => onOpenNode(node))}>Open</MenuButton>
        <MenuButton onClick={() => run(() => onRenameNode(node))} disabled={isRoot}><Pencil size={14} /> Rename</MenuButton>
        <MenuButton onClick={() => run(copyPath)}><Copy size={14} /> Copy path</MenuButton>
        <MenuButton danger onClick={() => run(() => onDeleteNode(node))} disabled={isRoot}><Trash2 size={14} /> Delete</MenuButton>
      </div>
    </>
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

function RootTree(props: TreeProps) {
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

function TreeNode({ node, depth, activeSpace, activeNodeId, expandedFolderIds, onToggleFolder, onOpenNode, onNodeContextMenu }: TreeProps & { node: RestNode; depth: number }) {
  const isExpanded = expandedFolderIds.has(node.id);
  const childrenQuery = useNodeChildren(activeSpace.id, node.id, node.kind === "folder" && isExpanded);
  const children = childrenQuery.data?.pages.flatMap((page) => page.children) ?? [];
  return (
    <div>
      <NodeRow node={node} depth={depth} selected={activeNodeId === node.id} expanded={isExpanded} onToggleFolder={onToggleFolder} onOpenNode={onOpenNode} onNodeContextMenu={onNodeContextMenu} />
      {node.kind === "folder" && isExpanded ? (
        <div>
          {childrenQuery.isLoading ? <div className="px-8 py-1 text-xs text-muted">Loading…</div> : null}
          {children.map((child) => (
            <TreeNode key={child.id} node={child} depth={depth + 1} activeSpace={activeSpace} activeNodeId={activeNodeId} expandedFolderIds={expandedFolderIds} onToggleFolder={onToggleFolder} onOpenNode={onOpenNode} onNodeContextMenu={onNodeContextMenu} />
          ))}
          {childrenQuery.hasNextPage ? (
            <button
              className="block w-full rounded-lg px-2 py-1 text-left text-xs text-muted hover:bg-surface hover:text-text disabled:opacity-50"
              style={{ paddingLeft: `${8 + (depth + 1) * 14}px` }}
              disabled={childrenQuery.isFetchingNextPage}
              onClick={() => childrenQuery.fetchNextPage()}
            >
              {childrenQuery.isFetchingNextPage ? "Loading…" : "Load more"}
            </button>
          ) : null}
        </div>
      ) : null}
    </div>
  );
}

function useNodeChildren(spaceId: string, nodeId: string, enabled: boolean) {
  const client = useApiClient();
  return useInfiniteQuery({
    queryKey: queryKeys.children(spaceId, nodeId),
    queryFn: ({ pageParam }) => listChildren(client, spaceId, nodeId, pageParam),
    initialPageParam: null as string | null,
    getNextPageParam: (lastPage) => (lastPage.page.has_more ? lastPage.page.next_cursor : undefined),
    enabled
  });
}

function RecentList({ activeSpace, activeNodeId, onOpenNode, onNodeContextMenu }: { activeSpace: Space; activeNodeId: string | null; onOpenNode: (node: RestNode) => void; onNodeContextMenu: NodeContextHandler }) {
  const client = useApiClient();
  const recentQuery = useQuery({ queryKey: queryKeys.recent(activeSpace.id), queryFn: () => listNodes(client, activeSpace.id, { sort: "updated_at_desc" }) });
  if (recentQuery.isLoading) return <div className="mt-2 text-xs text-muted">Loading recent…</div>;
  if (recentQuery.isError) return <div className="mt-2 rounded-lg border border-border bg-surface p-3 text-xs text-muted">Recent is unavailable for this server build.</div>;
  const nodes = recentQuery.data?.nodes ?? [];
  if (nodes.length === 0) return <div className="mt-2 text-xs text-muted">No recent nodes yet.</div>;
  return <div className="mt-2 space-y-1">{nodes.map((node) => <NodeRow key={node.id} node={node} depth={0} selected={activeNodeId === node.id} onOpenNode={onOpenNode} onNodeContextMenu={onNodeContextMenu} />)}</div>;
}

function SectionTitle({ icon, label }: { icon: ReactNode; label: string }) {
  return <div className="flex items-center gap-2 text-[11px] font-semibold uppercase tracking-wide text-muted">{icon}{label}</div>;
}

function NodeRow({ node, depth, selected, expanded, onToggleFolder, onOpenNode, onNodeContextMenu }: { node: RestNode; depth: number; selected: boolean; expanded?: boolean; onToggleFolder?: (nodeId: string) => void; onOpenNode: (node: RestNode) => void; onNodeContextMenu: NodeContextHandler }) {
  const Icon = node.kind === "folder" ? Folder : node.kind === "file" ? Database : FileText;
  function handleOpen() {
    if (node.kind === "folder") onToggleFolder?.(node.id);
    onOpenNode(node);
  }
  return (
    <div
      className={`group flex w-full items-center gap-1 rounded-lg py-1.5 pr-2 text-sm transition ${selected ? "bg-panel-strong text-text" : "text-muted hover:bg-surface hover:text-text"}`}
      style={{ paddingLeft: `${8 + depth * 14}px` }}
      onContextMenu={(event) => onNodeContextMenu(node, event)}
    >
      {node.kind === "folder" ? <button className="grid size-4 place-items-center" onClick={() => onToggleFolder?.(node.id)}><ChevronRight size={13} className={expanded ? "rotate-90 transition" : "transition"} /></button> : <span className="size-4" />}
      <button data-node-open className="flex min-w-0 flex-1 items-center gap-2 text-left outline-none focus-visible:rounded focus-visible:ring-2 focus-visible:ring-primary/50" onClick={handleOpen}>
        <Icon size={15} />
        <span className="truncate">{node.name}</span>
      </button>
    </div>
  );
}
