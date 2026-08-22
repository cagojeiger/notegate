import { List } from "lucide-react";
import { useEffect, useMemo, useRef, type RefObject } from "react";

import type { NodeSummary, Space } from "../../api/types";
import { useRecentNodesQuery } from "./useNodeQueries";
import { EmptyState } from "../../shared/ui";
import { NodeRow } from "./NodeRow";
import { SidebarSectionHeader } from "./SidebarSectionHeader";
import type { NodeContextHandler } from "./types";

export function RecentSection({ activeSpace, openedNodeId, inspectedNodeId, density, open, onToggle, onToggleDensity, onOpenNode, onInspectNode, onNodeContextMenu }: { activeSpace: Space; openedNodeId: string | null; inspectedNodeId: string | null; density: "list" | "compact"; open: boolean; onToggle: () => void; onToggleDensity: () => void; onOpenNode: (node: NodeSummary) => void; onInspectNode: (node: NodeSummary) => void; onNodeContextMenu: NodeContextHandler }) {
  const scrollRef = useRef<HTMLDivElement>(null);
  return (
    <section className="flex min-h-0 min-w-0 flex-col px-2 py-1 font-ui">
      <SidebarSectionHeader
        label="Recent"
        open={open}
        onToggle={onToggle}
        trailing={(
          <button onClick={onToggleDensity} aria-label="Toggle recent density" title="Toggle recent density" className="grid size-workbench-control shrink-0 place-items-center rounded-workbench text-muted hover:bg-surface hover:text-text md:size-6">
            <List size={13} />
          </button>
        )}
      />
      {open ? (
        <div ref={scrollRef} data-recent-list className="min-h-0 flex-1 overflow-y-auto">
          <RecentList activeSpace={activeSpace} openedNodeId={openedNodeId} inspectedNodeId={inspectedNodeId} density={density} scrollRef={scrollRef} onOpenNode={onOpenNode} onInspectNode={onInspectNode} onNodeContextMenu={onNodeContextMenu} />
        </div>
      ) : null}
    </section>
  );
}

function RecentList({ activeSpace, openedNodeId, inspectedNodeId, density, scrollRef, onOpenNode, onInspectNode, onNodeContextMenu }: { activeSpace: Space; openedNodeId: string | null; inspectedNodeId: string | null; density: "list" | "compact"; scrollRef: RefObject<HTMLDivElement | null>; onOpenNode: (node: NodeSummary) => void; onInspectNode: (node: NodeSummary) => void; onNodeContextMenu: NodeContextHandler }) {
  const recentQuery = useRecentNodesQuery(activeSpace.id);
  const nodes = useMemo(() => {
    const seen = new Set<string>();
    return (recentQuery.data?.pages ?? []).flatMap((page) =>
      page.nodes.filter((node) => {
        if (seen.has(node.id)) return false;
        seen.add(node.id);
        return true;
      })
    );
  }, [recentQuery.data?.pages]);
  if (recentQuery.isLoading) return <div className="text-xs text-muted">Loading recent…</div>;
  if (recentQuery.isError) return <EmptyState>Recent is unavailable for this server build.</EmptyState>;
  if (nodes.length === 0) return <div className="text-xs text-muted">No recent items yet.</div>;
  return (
    <div className={density === "list" ? "space-y-0.5" : undefined}>
      {nodes.map((node) => (
        <NodeRow
          key={node.id}
          node={node}
          depth={0}
          inspected={inspectedNodeId === node.id}
          opened={openedNodeId === node.id}
          meta={density === "list" ? `${node.path} · ${node.updated_at.slice(0, 10)}` : undefined}
          reserveDisclosureSpace={false}
          onOpenNode={onOpenNode}
          onInspectNode={onInspectNode}
          onNodeContextMenu={onNodeContextMenu}
        />
      ))}
      {recentQuery.hasNextPage ? (
        <RecentLoadMore
          loaded={nodes.length}
          isFetching={recentQuery.isFetchingNextPage}
          scrollRef={scrollRef}
          fetchNextPage={() => {
            if (!recentQuery.isFetchingNextPage) void recentQuery.fetchNextPage();
          }}
        />
      ) : null}
    </div>
  );
}

function RecentLoadMore({ loaded, isFetching, scrollRef, fetchNextPage }: { loaded: number; isFetching: boolean; scrollRef: RefObject<HTMLDivElement | null>; fetchNextPage: () => void }) {
  const ref = useRef<HTMLDivElement>(null);
  useEffect(() => {
    const element = ref.current;
    if (!element) return;
    const observer = new IntersectionObserver(
      (entries) => {
        if (entries[0]?.isIntersecting && !isFetching) fetchNextPage();
      },
      { root: scrollRef.current, rootMargin: "80px" }
    );
    observer.observe(element);
    return () => observer.disconnect();
  }, [fetchNextPage, isFetching, scrollRef]);

  return (
    <div ref={ref} className="flex justify-center py-1">
      <button
        type="button"
        className="min-h-workbench-control rounded px-2 py-1 text-workbench text-faint hover:bg-[var(--ng-hover)] hover:text-text focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-primary/50 md:min-h-6"
        disabled={isFetching}
        onClick={fetchNextPage}
      >
        {isFetching ? "Loading…" : `Load more (${loaded} loaded)`}
      </button>
    </div>
  );
}
