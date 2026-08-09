import { Image as ImageIcon, Link2, RefreshCw, Unlink } from "lucide-react";

import type { LinkReference, RestNode } from "../../api/types";
import { Button, MetaRow, SectionHeader } from "../../shared/ui";
import { formatLinkIndexUpdateTime, linkIndexStatusLabel } from "./linkIndexPresentation";
import {
  useNodeLinkReferencesQuery,
  useSpaceLinkIndexQuery,
  useSyncNodeLinkIndexMutation
} from "./useLinkIndexQueries";

export function LinkInspectorPanel({
  node,
  canSync,
  onOpen
}: {
  node: RestNode;
  canSync: boolean;
  onOpen: (nodeId: string) => void;
}) {
  const linkIndex = useSpaceLinkIndexQuery(node.space_id);
  const syncLinkIndex = useSyncNodeLinkIndexMutation();
  const data = linkIndex.data;
  const snapshot = data ? `${data.status}:${data.latest_index_update_at ?? "never"}` : null;
  const outgoing = useNodeLinkReferencesQuery(node, "outgoing", snapshot);
  const incoming = useNodeLinkReferencesQuery(node, "incoming", snapshot);
  const outgoingReferences = outgoing.data?.pages.flatMap((page) => page.links) ?? [];
  const incomingReferences = incoming.data?.pages.flatMap((page) => page.links) ?? [];

  return (
    <div className="divide-y divide-seam rounded-2xl border border-border bg-surface">
      <section className="px-3 py-3">
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
          <dl className="space-y-1.5">
            <MetaRow label="Space status" value={linkIndexStatusLabel(data.status)} />
            <MetaRow
              label="Latest index update"
              value={formatLinkIndexUpdateTime(data.latest_index_update_at)}
            />
          </dl>
        ) : null}
      </section>
      <LinkReferenceSection
        title="Outgoing"
        empty="This item does not link to another item."
        references={outgoingReferences}
        isLoading={outgoing.isLoading}
        isError={outgoing.isError}
        hasMore={outgoing.hasNextPage}
        isLoadingMore={outgoing.isFetchingNextPage}
        onLoadMore={() => { void outgoing.fetchNextPage(); }}
        onOpen={onOpen}
      />
      <LinkReferenceSection
        title="Incoming"
        empty="No other item links here."
        references={incomingReferences}
        isLoading={incoming.isLoading}
        isError={incoming.isError}
        hasMore={incoming.hasNextPage}
        isLoadingMore={incoming.isFetchingNextPage}
        onLoadMore={() => { void incoming.fetchNextPage(); }}
        onOpen={onOpen}
      />
    </div>
  );
}

function LinkReferenceSection({
  title,
  empty,
  references,
  isLoading,
  isError,
  hasMore,
  isLoadingMore,
  onLoadMore,
  onOpen
}: {
  title: string;
  empty: string;
  references: LinkReference[];
  isLoading: boolean;
  isError: boolean;
  hasMore: boolean;
  isLoadingMore: boolean;
  onLoadMore: () => void;
  onOpen: (nodeId: string) => void;
}) {
  return (
    <section aria-label={title} className="px-3 py-3">
      <SectionHeader
        title={title}
        actions={<span className="text-xs tabular-nums text-muted">{references.length}{hasMore ? "+" : ""}</span>}
      />
      {isLoading ? <p className="text-xs text-muted">Loading…</p> : null}
      {isError ? <p role="alert" className="text-xs text-danger">Could not load {title.toLowerCase()} links.</p> : null}
      {!isLoading && !isError && references.length === 0 ? <p className="text-xs text-muted">{empty}</p> : null}
      {references.length > 0 ? (
        <ul className="space-y-0.5">
          {references.map((reference) => {
            const missing = reference.node_id === null;
            return (
              <li key={`${reference.kind}:${reference.path}`}>
                <button
                  type="button"
                  className="flex min-h-8 w-full min-w-0 items-center gap-1.5 rounded-md px-1.5 text-left text-sm hover:bg-[var(--ng-hover)] disabled:cursor-default disabled:hover:bg-transparent [@media(pointer:coarse)]:min-h-11"
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
      ) : null}
      {hasMore ? (
        <div className="mt-2 flex justify-center">
          <Button
            secondary
            size="sm"
            aria-label={`Load more ${title.toLowerCase()} links`}
            onClick={onLoadMore}
            disabled={isLoadingMore}
          >
            {isLoadingMore ? "Loading…" : "Load more"}
          </Button>
        </div>
      ) : null}
    </section>
  );
}
