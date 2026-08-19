import { AlertTriangle, Image, Link2, RefreshCw } from "lucide-react";

import type { NodeLink, NodeLinkProjectionStatus } from "../../api/links";
import type { RestNode } from "../../api/types";
import { Button, SectionHeader } from "../../shared/ui";
import {
  useNodeLinksQuery,
  useNodeLinkStatusQuery,
  useSyncNodeLinksMutation
} from "./useLinkQueries";
import {
  isLinkProjectionActive,
  useRefreshNodeLinksAfterProjection
} from "./useRefreshNodeLinksAfterProjection";

type NodeLinksPanelProps = {
  node: RestNode;
  canSync: boolean;
  onOpenNode: (nodeId: string, sourceNodeId: string) => void;
};

export function NodeLinksPanel({ node, canSync, onOpenNode }: NodeLinksPanelProps) {
  const indexableText = node.kind === "text" && node.text_storage_format !== "encrypted";
  const statusQuery = useNodeLinkStatusQuery(node, true);
  const outgoingQuery = useNodeLinksQuery(node, "outgoing", indexableText);
  const incomingQuery = useNodeLinksQuery(node, "incoming", true);
  const syncMutation = useSyncNodeLinksMutation();
  const status = statusQuery.data?.status;
  const busy = status === "pending" || status === "syncing" || syncMutation.isPending;
  const openLinkedNode = (targetNodeId: string) => onOpenNode(targetNodeId, node.id);

  useRefreshNodeLinksAfterProjection({
    spaceId: node.space_id,
    nodeId: node.id,
    status: statusQuery.data
  });

  const outgoing = outgoingQuery.data?.pages.flatMap((page) => page.links) ?? [];
  const incoming = incomingQuery.data?.pages.flatMap((page) => page.links) ?? [];

  return (
    <div className="space-y-4">
      {indexableText ? (
        <section>
          <SectionHeader
            title="Index status"
            actions={canSync ? (
              <Button
                secondary
                size="xs"
                disabled={busy}
                onClick={() => syncMutation.mutate(node)}
                aria-label={`Sync links for ${node.name}`}
              >
                <RefreshCw size={13} className={busy ? "animate-spin" : undefined} />
                Sync
              </Button>
            ) : undefined}
          />
          <ProjectionStatus status={statusQuery.data} loading={statusQuery.isLoading} error={statusQuery.isError} />
          {syncMutation.isError ? (
            <p role="alert" className="mt-1 text-xs text-danger">Could not request a link update.</p>
          ) : null}
        </section>
      ) : null}

      {indexableText ? (
        <LinkSection
          title="Outgoing"
          emptyMessage="No outgoing links."
          links={outgoing}
          loading={outgoingQuery.isLoading}
          error={outgoingQuery.isError}
          fetchingMore={outgoingQuery.isFetchingNextPage}
          hasMore={outgoingQuery.hasNextPage}
          loadMoreLabel="Load more outgoing links"
          onRetry={() => { void outgoingQuery.refetch(); }}
          onLoadMore={() => { void outgoingQuery.fetchNextPage(); }}
          onOpenNode={openLinkedNode}
        />
      ) : node.kind === "text" ? (
        <section>
          <SectionHeader title="Outgoing" />
          <p className="text-xs text-muted">
            Outgoing links are unavailable for client-encrypted text.
          </p>
        </section>
      ) : null}

      <LinkSection
        title="Backlinks"
        emptyMessage="No backlinks."
        links={incoming}
        loading={incomingQuery.isLoading}
        error={incomingQuery.isError}
        fetchingMore={incomingQuery.isFetchingNextPage}
        hasMore={incomingQuery.hasNextPage}
        loadMoreLabel="Load more backlinks"
        onRetry={() => { void incomingQuery.refetch(); }}
        onLoadMore={() => { void incomingQuery.fetchNextPage(); }}
        onOpenNode={openLinkedNode}
      />
    </div>
  );
}

function ProjectionStatus({
  status,
  loading,
  error
}: {
  status: NodeLinkProjectionStatus | undefined;
  loading: boolean;
  error: boolean;
}) {
  if (loading) return <p className="text-xs text-muted">Loading link status…</p>;
  if (error || !status) return <p className="text-xs text-danger">Could not load link status.</p>;

  const label = projectionStatusLabel(status);
  const tone = status.status === "failed"
    ? "text-danger"
    : isLinkProjectionActive(status)
      ? "text-warning"
      : "text-muted";
  return (
    <p
      className={`text-xs ${tone}`}
      aria-live="polite"
    >
      {label}
    </p>
  );
}

function LinkSection({
  title,
  emptyMessage,
  links,
  loading,
  error,
  fetchingMore,
  hasMore,
  loadMoreLabel,
  onRetry,
  onLoadMore,
  onOpenNode
}: {
  title: string;
  emptyMessage: string;
  links: NodeLink[];
  loading: boolean;
  error: boolean;
  fetchingMore: boolean;
  hasMore: boolean;
  loadMoreLabel: string;
  onRetry: () => void;
  onLoadMore: () => void;
  onOpenNode: (nodeId: string) => void;
}) {
  return (
    <section>
      <SectionHeader
        title={title}
        actions={(
          <span className="text-xs tabular-nums text-muted">
            <span aria-hidden="true">{links.length}{hasMore ? "+" : ""}</span>
            <span className="sr-only">
              {links.length} links loaded{hasMore ? ", more available" : ""}
            </span>
          </span>
        )}
      />
      {loading ? <p className="text-xs text-muted">Loading links…</p> : null}
      {error ? (
        <div className="flex items-center justify-between gap-2">
          <p className="text-xs text-danger">Could not load links.</p>
          <Button secondary size="xs" onClick={onRetry}>Retry</Button>
        </div>
      ) : null}
      {!loading && !error && links.length === 0 ? (
        <p className="text-xs text-muted">{emptyMessage}</p>
      ) : null}
      {links.length > 0 ? (
        <ul className="divide-y divide-seam border-y border-seam">
          {links.map((link) => (
            <li key={`${link.kind}:${link.path}`}>
              <LinkRow link={link} onOpenNode={onOpenNode} />
            </li>
          ))}
        </ul>
      ) : null}
      {hasMore ? (
        <Button
          secondary
          size="xs"
          className="mt-2 w-full"
          disabled={fetchingMore}
          onClick={onLoadMore}
          aria-label={loadMoreLabel}
        >
          {fetchingMore ? "Loading…" : "Load more"}
        </Button>
      ) : null}
    </section>
  );
}

function LinkRow({ link, onOpenNode }: { link: NodeLink; onOpenNode: (nodeId: string) => void }) {
  const content = (
    <>
      {link.node_id ? (
        link.kind === "image" ? <Image size={14} aria-hidden="true" /> : <Link2 size={14} aria-hidden="true" />
      ) : (
        <AlertTriangle size={14} className="text-danger" aria-hidden="true" />
      )}
      <span className="min-w-0 flex-1 break-words text-left">{link.path}</span>
      {link.occurrence_count > 1 ? (
        <span className="shrink-0 text-xs tabular-nums text-muted">×{link.occurrence_count}</span>
      ) : null}
      {!link.node_id ? <span className="shrink-0 text-xs text-danger">Broken</span> : null}
    </>
  );

  if (!link.node_id) {
    return <div className="flex min-h-workbench-row items-start gap-2 px-1 py-1.5 text-workbench text-muted">{content}</div>;
  }
  return (
    <button
      type="button"
      className="flex min-h-workbench-row w-full items-start gap-2 px-1 py-1.5 text-workbench text-muted outline-none transition hover:bg-[var(--ng-hover)] hover:text-text focus-visible:ring-2 focus-visible:ring-inset focus-visible:ring-primary/45"
      onClick={() => onOpenNode(link.node_id!)}
      aria-label={`Open ${link.path}`}
      title={link.path}
    >
      {content}
    </button>
  );
}

function projectionStatusLabel(status: NodeLinkProjectionStatus): string {
  if (status.status === "failed") {
    return status.failure_code === "link_reference_limit_exceeded"
      ? "Too many unique links to index"
      : "Link update failed";
  }
  if (status.status === "syncing") return "Updating links…";
  if (status.status === "pending" || status.space_pending) return "Waiting to update";
  return status.projected_at ? `Indexed ${status.projected_at.slice(0, 10)}` : "Not indexed yet";
}
