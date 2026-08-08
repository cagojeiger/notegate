import { Image as ImageIcon, Link2, RefreshCw, Unlink } from "lucide-react";

import type { LinkReference, RestNode } from "../../api/types";
import { Button, MetaRow, SectionHeader } from "../../shared/ui";
import { formatLinkIndexSyncTime, linkIndexStatusLabel } from "./linkIndexPresentation";
import { useNodeLinkIndexQuery, useSyncNodeLinkIndexMutation } from "./useLinkIndexQueries";

export function LinkInspectorPanel({
  node,
  canSync,
  onOpen
}: {
  node: RestNode;
  canSync: boolean;
  onOpen: (nodeId: string) => void;
}) {
  const linkIndex = useNodeLinkIndexQuery(node);
  const syncLinkIndex = useSyncNodeLinkIndexMutation();
  const data = linkIndex.data;

  return (
    <div className="divide-y divide-seam rounded-2xl border border-border bg-surface">
      <section className="p-4">
        <SectionHeader
          title="Link index"
          actions={node.kind === "text" ? (
            <Button
              secondary
              size="sm"
              onClick={() => syncLinkIndex.mutate({ spaceId: node.space_id, nodeId: node.id })}
              disabled={!canSync || syncLinkIndex.isPending}
            >
              <RefreshCw size={14} className={syncLinkIndex.isPending ? "animate-spin" : undefined} />
              Sync now
            </Button>
          ) : undefined}
        />
        {linkIndex.isLoading ? <p className="text-xs text-muted">Loading links…</p> : null}
        {linkIndex.isError || syncLinkIndex.isError ? (
          <p role="alert" className="text-xs text-danger">Could not load links.</p>
        ) : null}
        {data ? (
          <dl className="space-y-2">
            <MetaRow label="Status" value={linkIndexStatusLabel(data.status)} />
            <MetaRow label="Last synced" value={formatLinkIndexSyncTime(data.last_synced_at)} />
          </dl>
        ) : null}
      </section>
      <LinkReferenceSection
        title="Outgoing"
        empty="This item does not link to another item."
        references={data?.outgoing ?? []}
        onOpen={onOpen}
      />
      <LinkReferenceSection
        title="Incoming"
        empty="No other item links here."
        references={data?.incoming ?? []}
        onOpen={onOpen}
      />
    </div>
  );
}

function LinkReferenceSection({
  title,
  empty,
  references,
  onOpen
}: {
  title: string;
  empty: string;
  references: LinkReference[];
  onOpen: (nodeId: string) => void;
}) {
  return (
    <section aria-label={title} className="p-4">
      <SectionHeader title={title} actions={<span className="text-xs tabular-nums text-muted">{references.length}</span>} />
      {references.length === 0 ? <p className="text-xs text-muted">{empty}</p> : (
        <ul className="space-y-1">
          {references.map((reference) => {
            const missing = reference.node_id === null;
            return (
              <li key={`${reference.kind}:${reference.path}`}>
                <button
                  type="button"
                  className="flex min-h-9 w-full min-w-0 items-center gap-2 rounded-lg px-2 text-left text-sm hover:bg-[var(--ng-hover)] disabled:cursor-default disabled:hover:bg-transparent"
                  disabled={missing}
                  onClick={() => {
                    if (reference.node_id) onOpen(reference.node_id);
                  }}
                >
                  {missing ? <Unlink size={15} className="shrink-0 text-danger" /> : reference.kind === "image" ? (
                    <ImageIcon size={15} className="shrink-0 text-muted" />
                  ) : (
                    <Link2 size={15} className="shrink-0 text-muted" />
                  )}
                  <span className={`min-w-0 flex-1 truncate ${missing ? "text-danger" : "text-text"}`}>{reference.path}</span>
                  {missing ? <span className="shrink-0 text-xs text-danger">Missing</span> : null}
                  {reference.occurrence_count > 1 ? (
                    <span className="shrink-0 text-xs tabular-nums text-muted">×{reference.occurrence_count}</span>
                  ) : null}
                </button>
              </li>
            );
          })}
        </ul>
      )}
    </section>
  );
}
