import { AlertTriangle, ArrowDownLeft, ArrowUpRight, Image, Link2 } from "lucide-react";

import type { LinkReference, NodeLinkSummary } from "../../api/linkIndex";
import type { RestNode } from "../../api/types";
import { SectionHeader } from "../../shared/ui";
import { useNodeLinksQuery } from "./useLinkIndexQueries";

export function NodeLinksSection({
  node,
  onOpenLinkedNode
}: {
  node: RestNode | null;
  onOpenLinkedNode: (spaceId: string, nodeId: string) => void;
}) {
  const links = useNodeLinksQuery(node?.space_id ?? null, node?.id ?? null);

  return (
    <section className="p-4">
      <SectionHeader title="Links" />
      {!node ? <p className="text-xs text-muted">Choose something from Files to inspect its links.</p> : null}
      {node && links.isLoading ? <p className="text-xs text-muted">Loading links…</p> : null}
      {node && links.isError ? <p className="text-xs text-danger">Could not load links.</p> : null}
      {node && links.data ? (
        <NodeLinkContent
          summary={links.data}
          onOpenLinkedNode={(nodeId) => onOpenLinkedNode(node.space_id, nodeId)}
        />
      ) : null}
    </section>
  );
}

function NodeLinkContent({
  summary,
  onOpenLinkedNode
}: {
  summary: NodeLinkSummary;
  onOpenLinkedNode: (nodeId: string) => void;
}) {
  if (summary.index.freshness === "uninitialized") {
    return <p className="text-xs text-muted">Index links from the Space Inspector to view relations.</p>;
  }
  if (summary.index.freshness === "rebuilding") {
    return <p className="text-xs text-muted">Reindexing links…</p>;
  }
  if (summary.index.freshness === "failed") {
    return <p className="text-xs text-danger">The link index failed. Retry from the Space Inspector.</p>;
  }

  const hasRelations = summary.outgoing_count > 0 || summary.incoming_count > 0;
  return (
    <div className="space-y-3">
      <div className="flex flex-wrap gap-x-3 gap-y-1 text-xs text-muted" aria-label="Link summary">
        <span className="inline-flex items-center gap-1"><ArrowUpRight size={13} />{summary.outgoing_count} outgoing</span>
        <span className="inline-flex items-center gap-1"><ArrowDownLeft size={13} />{summary.incoming_count} incoming</span>
        {summary.broken_count > 0 ? (
          <span className="inline-flex items-center gap-1 text-warning"><AlertTriangle size={13} />{summary.broken_count} broken</span>
        ) : null}
      </div>
      {!hasRelations ? <p className="text-xs text-muted">No indexed links.</p> : null}
      <RelationList title="Outgoing" items={summary.outgoing} outgoing truncated={summary.outgoing_truncated} onOpenLinkedNode={onOpenLinkedNode} />
      <RelationList title="Incoming" items={summary.incoming} outgoing={false} truncated={summary.incoming_truncated} onOpenLinkedNode={onOpenLinkedNode} />
      {summary.index.freshness === "updating" ? <p className="text-xs text-muted">Updating index…</p> : null}
    </div>
  );
}

function RelationList({
  title,
  items,
  outgoing,
  truncated,
  onOpenLinkedNode
}: {
  title: string;
  items: LinkReference[];
  outgoing: boolean;
  truncated: boolean;
  onOpenLinkedNode: (nodeId: string) => void;
}) {
  if (items.length === 0) return null;
  return (
    <div>
      <p className="mb-1.5 text-xs font-medium text-text">{title}</p>
      <ul className="space-y-1.5">
        {items.map((reference) => {
          const label = outgoing
            ? reference.target_name ?? reference.normalized_target_path ?? reference.raw_href
            : reference.source_name;
          const path = outgoing ? reference.target_path ?? reference.normalized_target_path : reference.source_path;
          const status = referenceStatusLabel(reference.status);
          const Icon = reference.kind === "image" ? Image : Link2;
          const linkedNodeId = outgoing ? reference.target_node_id : reference.source_node_id;
          const content = (
            <>
              <Icon size={14} className="mt-0.5 shrink-0 text-muted" aria-hidden="true" />
              <span className="min-w-0">
                <span className={reference.status === "resolved" ? "block truncate text-text" : "block truncate text-warning"} title={label}>{label}</span>
                {path && path !== label ? <span className="block truncate text-muted" title={path}>{path}</span> : null}
                {status ? <span className="block text-warning">{status}</span> : null}
              </span>
            </>
          );
          return (
            <li key={reference.id} className="min-w-0 text-xs">
              {reference.status === "resolved" && linkedNodeId ? (
                <button
                  type="button"
                  className="flex w-full min-w-0 items-start gap-2 rounded-md px-1 py-0.5 text-left hover:bg-[var(--ng-hover)] focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-accent"
                  onClick={() => onOpenLinkedNode(linkedNodeId)}
                  aria-label={`Open ${label}`}
                >
                  {content}
                </button>
              ) : (
                <div className="flex min-w-0 items-start gap-2 px-1 py-0.5">{content}</div>
              )}
            </li>
          );
        })}
      </ul>
      {truncated ? <p className="mt-1.5 text-xs text-muted">More links are not shown.</p> : null}
    </div>
  );
}

function referenceStatusLabel(status: LinkReference["status"]): string | null {
  switch (status) {
    case "resolved": return null;
    case "deleted": return "Deleted target";
    case "missing": return "Missing target";
    case "invalid": return "Invalid path";
  }
}
