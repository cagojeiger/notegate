import { FileText, List } from "lucide-react";
import { useEffect, useMemo, useRef, type RefObject } from "react";

import type { NodeSummary, Space } from "../../api/types";
import { useRecentNodesQuery } from "./useNodeQueries";
import { EmptyState } from "../../shared/ui";
import { NodeRow } from "./NodeRow";
import { SidebarSectionHeader } from "./SidebarSectionHeader";
import type { NodeContextHandler } from "./types";

export function RecentSection({ activeSpace, activeNodeId, density, open, onToggle, onToggleDensity, onOpenNode, onNodeContextMenu }: { activeSpace: Space; activeNodeId: string | null; density: "list" | "compact"; open: boolean; onToggle: () => void; onToggleDensity: () => void; onOpenNode: (node: NodeSummary) => void; onNodeContextMenu: NodeContextHandler }) {
  const scrollRef = useRef<HTMLDivElement>(null);
  return (
    <section className="flex min-h-0 min-w-0 flex-col px-3 py-2">
      <SidebarSectionHeader icon={<FileText size={13} />} label="Recent" open={open} onToggle={onToggle} action={{ label: "Toggle recent density", icon: <List size={13} />, onClick: onToggleDensity }} />
      {open ? (
        <div ref={scrollRef} data-recent-list className="mt-2 min-h-0 flex-1 overflow-y-auto">
          <RecentList activeSpace={activeSpace} activeNodeId={activeNodeId} density={density} scrollRef={scrollRef} onOpenNode={onOpenNode} onNodeContextMenu={onNodeContextMenu} />
        </div>
      ) : null}
    </section>
  );
}

function RecentList({ activeSpace, activeNodeId, density, scrollRef, onOpenNode, onNodeContextMenu }: { activeSpace: Space; activeNodeId: string | null; density: "list" | "compact"; scrollRef: RefObject<HTMLDivElement | null>; onOpenNode: (node: NodeSummary) => void; onNodeContextMenu: NodeContextHandler }) {
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
  if (nodes.length === 0) return <div className="text-xs text-muted">No recent nodes yet.</div>;
  return (
    <div className="space-y-1">
      {nodes.map((node) => (
        <NodeRow
          key={node.id}
          node={node}
          depth={0}
          selected={activeNodeId === node.id}
          meta={density === "list" ? `${node.path} · ${node.updated_at.slice(0, 10)}` : undefined}
          onOpenNode={onOpenNode}
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
        className="rounded px-2 py-1 text-xs text-faint hover:bg-[var(--ng-hover)] hover:text-text focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-primary/50"
        disabled={isFetching}
        onClick={fetchNextPage}
      >
        {isFetching ? "Loading…" : `Load more (${loaded} loaded)`}
      </button>
    </div>
  );
}
