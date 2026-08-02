import { AlertTriangle, ArrowDownLeft, ArrowUpRight, Image, Link2 } from "lucide-react";

import type { LinkReference, NodeLinkSummary } from "../../api/linkIndex";
import type { RestNode } from "../../api/types";
import { SectionHeader } from "../../shared/ui";
import { useNodeLinksQuery } from "./useLinkIndexQueries";

export function NodeLinksSection({ node }: { node: RestNode | null }) {
  const links = useNodeLinksQuery(node?.space_id ?? null, node?.id ?? null);

  return (
    <section className="p-4">
      <SectionHeader title="Links" />
      {!node ? <p className="text-xs text-muted">Choose something from Files to inspect its links.</p> : null}
      {node && links.isLoading ? <p className="text-xs text-muted">Loading links…</p> : null}
      {node && links.isError ? <p className="text-xs text-danger">Could not load links.</p> : null}
      {links.data ? <NodeLinkContent summary={links.data} /> : null}
    </section>
  );
}

function NodeLinkContent({ summary }: { summary: NodeLinkSummary }) {
  if (summary.index.freshness === "rebuilding") {
    return <p className="text-xs text-muted">Reindexing links…</p>;
  }
  if (summary.index.freshness === "failed") {
    return <p className="text-xs text-danger">The last link-index update failed.</p>;
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
      <RelationList title="Outgoing" items={summary.outgoing} outgoing truncated={summary.outgoing_truncated} />
      <RelationList title="Incoming" items={summary.incoming} outgoing={false} truncated={summary.incoming_truncated} />
      {summary.index.freshness === "updating" ? <p className="text-xs text-muted">Updating index…</p> : null}
    </div>
  );
}

function RelationList({
  title,
  items,
  outgoing,
  truncated
}: {
  title: string;
  items: LinkReference[];
  outgoing: boolean;
  truncated: boolean;
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
          return (
            <li key={reference.id} className="flex min-w-0 items-start gap-2 text-xs">
              <Icon size={14} className="mt-0.5 shrink-0 text-muted" aria-hidden="true" />
              <span className="min-w-0">
                <span className={reference.status === "resolved" ? "block truncate text-text" : "block truncate text-warning"} title={label}>{label}</span>
                {path && path !== label ? <span className="block truncate text-muted" title={path}>{path}</span> : null}
                {status ? <span className="block text-warning">{status}</span> : null}
              </span>
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
