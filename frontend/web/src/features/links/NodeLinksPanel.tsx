import { useId } from "react";
import { RefreshCw } from "lucide-react";

import type { NodeLinkProjectionStatus } from "../../api/links";
import type { RestNode } from "../../api/types";
import { WORKBENCH_LAYOUT } from "../../shared/model/workbenchLayout";
import { Button, ResizeSeparator, SectionHeader } from "../../shared/ui";
import {
  useNodeLinksQuery,
  useNodeLinkStatusQuery,
  useSyncNodeLinksMutation
} from "./useLinkQueries";
import {
  isLinkProjectionActive,
  useRefreshNodeLinksAfterProjection
} from "./useRefreshNodeLinksAfterProjection";
import { NodeLinkSection } from "./NodeLinkSection";
import { useNodeLinkSections } from "./useNodeLinkSections";

type NodeLinksPanelProps = {
  node: RestNode;
  canSync: boolean;
  onOpenNode: (nodeId: string, sourceNodeId: string) => void;
};

export function NodeLinksPanel({ node, canSync, onOpenNode }: NodeLinksPanelProps) {
  const indexableText = node.kind === "text" && node.text_storage_format !== "encrypted";
  const sections = useNodeLinkSections();
  const outgoingSectionId = useId();
  const incomingSectionId = useId();
  const statusQuery = useNodeLinkStatusQuery(node, true);
  const outgoingQuery = useNodeLinksQuery(node, "outgoing", indexableText);
  const incomingQuery = useNodeLinksQuery(node, "incoming", true);
  const syncMutation = useSyncNodeLinksMutation();
  const status = statusQuery.data?.status;
  const requestPending = status === "pending" || status === "syncing" || syncMutation.isPending;
  const syncUnavailable = statusQuery.isLoading
    || statusQuery.isError
    || statusQuery.data?.availability?.can_trigger !== true
    || syncMutation.isPending;
  const openLinkedNode = (targetNodeId: string) => onOpenNode(targetNodeId, node.id);

  useRefreshNodeLinksAfterProjection({
    spaceId: node.space_id,
    nodeId: node.id,
    status: statusQuery.data
  });

  const outgoing = outgoingQuery.data?.pages.flatMap((page) => page.links) ?? [];
  const incoming = incomingQuery.data?.pages.flatMap((page) => page.links) ?? [];

  return (
    <div className="flex h-full min-h-0 flex-col gap-3">
      {indexableText ? (
        <section className="shrink-0">
          <SectionHeader
            title="Index status"
            actions={canSync ? (
              <Button
                variant="ghost"
                size="xs"
                disabled={syncUnavailable}
                onClick={() => syncMutation.mutate(node)}
                aria-label={`Sync links for ${node.name}`}
              >
                <RefreshCw size={13} className={requestPending ? "animate-spin" : undefined} />
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

      <div
        ref={sections.gridRef}
        data-testid="node-link-sections"
        className="grid min-h-0 min-w-0 flex-1 content-start border-y border-seam"
        style={{
          gridTemplateRows: indexableText
            ? sections.gridRows
            : node.kind === "text"
              ? `auto 1px ${sections.incomingOpen ? "1fr" : "auto"}`
              : sections.incomingOpen ? "1fr" : "auto"
        }}
      >
        {indexableText ? (
          <NodeLinkSection
            id={outgoingSectionId}
            direction="outgoing"
            title="Links from this document"
            emptyMessage="This document does not link to another item."
            expanded={sections.outgoingOpen}
            links={outgoing}
            loading={outgoingQuery.isLoading}
            error={outgoingQuery.isError && !outgoingQuery.isFetchNextPageError}
            loadMoreError={outgoingQuery.isFetchNextPageError}
            fetchingMore={outgoingQuery.isFetchingNextPage}
            hasMore={outgoingQuery.hasNextPage}
            onToggle={sections.toggleOutgoing}
            onRetry={() => { void outgoingQuery.refetch(); }}
            onLoadMore={() => { void outgoingQuery.fetchNextPage(); }}
            onOpenNode={openLinkedNode}
          />
        ) : node.kind === "text" ? (
          <div className="shrink-0 px-1 py-2">
            <p className="text-xs font-semibold text-text">Links from this document</p>
            <p className="mt-1 text-xs text-muted">
              Links from client-encrypted text cannot be indexed.
            </p>
          </div>
        ) : null}

        {indexableText ? (
          <div className="relative">
            {sections.bothOpen ? (
              <ResizeSeparator
                orientation="horizontal"
                label="Resize link sections"
                value={Math.round(sections.linkRatio * 100)}
                min={WORKBENCH_LAYOUT.minLinkRatio * 100}
                max={WORKBENCH_LAYOUT.maxLinkRatio * 100}
                step={5}
                valueText={`${Math.round(sections.linkRatio * 100)}% links from this document`}
                controls={`${outgoingSectionId} ${incomingSectionId}`}
                onPointerDown={sections.startResize}
                onValueChange={(value) => sections.setLinkRatio(value / 100)}
              />
            ) : (
              <span className="absolute inset-x-0 top-1/2 h-px bg-seam" aria-hidden="true" />
            )}
          </div>
        ) : node.kind === "text" ? (
          <span className="bg-seam" aria-hidden="true" />
        ) : null}

        <NodeLinkSection
          id={incomingSectionId}
          direction="incoming"
          title="Links to this document"
          emptyMessage="No documents link to this item."
          expanded={sections.incomingOpen}
          links={incoming}
          loading={incomingQuery.isLoading}
          error={incomingQuery.isError && !incomingQuery.isFetchNextPageError}
          loadMoreError={incomingQuery.isFetchNextPageError}
          fetchingMore={incomingQuery.isFetchingNextPage}
          hasMore={incomingQuery.hasNextPage}
          onToggle={sections.toggleIncoming}
          onRetry={() => { void incomingQuery.refetch(); }}
          onLoadMore={() => { void incomingQuery.fetchNextPage(); }}
          onOpenNode={openLinkedNode}
        />
      </div>
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
