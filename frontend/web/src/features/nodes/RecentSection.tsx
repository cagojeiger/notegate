import { FileText, List } from "lucide-react";

import type { RestNode, Space } from "../../api/types";
import { useRecentNodesQuery } from "./useNodeQueries";
import { EmptyState } from "../../shared/ui";
import { NodeRow } from "./NodeRow";
import { SidebarSectionHeader } from "./SidebarSectionHeader";
import type { NodeContextHandler } from "./types";

export function RecentSection({ activeSpace, activeNodeId, density, open, onToggle, onToggleDensity, onOpenNode, onNodeContextMenu }: { activeSpace: Space; activeNodeId: string | null; density: "list" | "compact"; open: boolean; onToggle: () => void; onToggleDensity: () => void; onOpenNode: (node: RestNode) => void; onNodeContextMenu: NodeContextHandler }) {
  return (
    <section className="flex min-h-0 min-w-0 flex-col px-3 py-2">
      <SidebarSectionHeader icon={<FileText size={13} />} label="Recent" open={open} onToggle={onToggle} action={{ label: "Toggle recent density", icon: <List size={13} />, onClick: onToggleDensity }} />
      {open ? (
        <div className="mt-2 min-h-0 flex-1 overflow-y-auto">
          <RecentList activeSpace={activeSpace} activeNodeId={activeNodeId} density={density} onOpenNode={onOpenNode} onNodeContextMenu={onNodeContextMenu} />
        </div>
      ) : null}
    </section>
  );
}

function RecentList({ activeSpace, activeNodeId, density, onOpenNode, onNodeContextMenu }: { activeSpace: Space; activeNodeId: string | null; density: "list" | "compact"; onOpenNode: (node: RestNode) => void; onNodeContextMenu: NodeContextHandler }) {
  const recentQuery = useRecentNodesQuery(activeSpace.id);
  if (recentQuery.isLoading) return <div className="text-xs text-muted">Loading recent…</div>;
  if (recentQuery.isError) return <EmptyState>Recent is unavailable for this server build.</EmptyState>;
  const nodes = recentQuery.data?.nodes ?? [];
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
    </div>
  );
}
