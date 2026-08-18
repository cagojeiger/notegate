import { AlertTriangle, Image, Link2, RefreshCw } from "lucide-react";
import { useEffect, useRef } from "react";
import { useQueryClient } from "@tanstack/react-query";

import type { NodeLink, NodeLinkProjectionStatus } from "../../api/links";
import { queryKeys } from "../../api/queryKeys";
import type { RestNode } from "../../api/types";
import { Button, SectionHeader } from "../../shared/ui";
import {
  useNodeLinksQuery,
  useNodeLinkStatusQuery,
  useSyncNodeLinksMutation
} from "./useLinkQueries";

type NodeLinksPanelProps = {
  node: RestNode;
  canSync: boolean;
  onOpenNode: (nodeId: string) => void;
};

export function NodeLinksPanel({ node, canSync, onOpenNode }: NodeLinksPanelProps) {
  const queryClient = useQueryClient();
  const statusQuery = useNodeLinkStatusQuery(node, node.kind === "text");
  const outgoingQuery = useNodeLinksQuery(node, "outgoing", node.kind === "text");
  const incomingQuery = useNodeLinksQuery(node, "incoming", true);
  const syncMutation = useSyncNodeLinksMutation();
  const previousStatus = useRef<{ nodeId: string; status?: NodeLinkProjectionStatus["status"] }>({
    nodeId: node.id
  });
  const status = statusQuery.data?.status;
  const busy = status === "pending" || status === "syncing" || syncMutation.isPending;

  useEffect(() => {
    const previous = previousStatus.current;
    if (
      previous.nodeId === node.id
      && isActiveStatus(previous.status)
      && status !== undefined
      && !isActiveStatus(status)
    ) {
      void queryClient.resetQueries({
        queryKey: queryKeys.nodeLinkList(node.space_id, node.id, "outgoing"),
        exact: true
      });
      void queryClient.resetQueries({
        queryKey: queryKeys.nodeLinkList(node.space_id, node.id, "incoming"),
        exact: true
      });
    }
    previousStatus.current = { nodeId: node.id, status };
  }, [node.id, node.space_id, queryClient, status]);

  const outgoing = outgoingQuery.data?.pages.flatMap((page) => page.links) ?? [];
  const incoming = incomingQuery.data?.pages.flatMap((page) => page.links) ?? [];

  return (
    <div className="space-y-4">
      {node.kind === "text" ? (
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

      {node.kind === "text" ? (
        <LinkSection
          title="Outgoing"
          emptyMessage="No outgoing links."
          links={outgoing}
          loading={outgoingQuery.isLoading}
          error={outgoingQuery.isError}
          fetchingMore={outgoingQuery.isFetchingNextPage}
          hasMore={outgoingQuery.hasNextPage}
          onRetry={() => { void outgoingQuery.refetch(); }}
          onLoadMore={() => { void outgoingQuery.fetchNextPage(); }}
          onOpenNode={onOpenNode}
        />
      ) : null}

      <LinkSection
        title="Backlinks"
        emptyMessage="No backlinks."
        links={incoming}
        loading={incomingQuery.isLoading}
        error={incomingQuery.isError}
        fetchingMore={incomingQuery.isFetchingNextPage}
        hasMore={incomingQuery.hasNextPage}
        onRetry={() => { void incomingQuery.refetch(); }}
        onLoadMore={() => { void incomingQuery.fetchNextPage(); }}
        onOpenNode={onOpenNode}
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

  const label = {
    idle: status.projected_at ? `Indexed ${status.projected_at.slice(0, 10)}` : "Not indexed yet",
    pending: "Waiting to update",
    syncing: "Updating links…",
    failed: status.failure_code === "link_reference_limit_exceeded"
      ? "Too many unique links to index"
      : "Link update failed"
  }[status.status];
  return (
    <p
      className={`text-xs ${status.status === "failed" ? "text-danger" : status.status === "idle" ? "text-muted" : "text-warning"}`}
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
  onRetry: () => void;
  onLoadMore: () => void;
  onOpenNode: (nodeId: string) => void;
}) {
  return (
    <section>
      <SectionHeader
        title={title}
        actions={<span className="text-xs tabular-nums text-muted">{links.length}</span>}
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

function isActiveStatus(status: NodeLinkProjectionStatus["status"] | undefined): boolean {
  return status === "pending" || status === "syncing";
}
