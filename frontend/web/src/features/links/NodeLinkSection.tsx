import { useEffect, useId, useRef, type RefObject } from "react";
import { AlertTriangle, ArrowLeft, ArrowRight, ChevronRight, FileText, Image } from "lucide-react";

import type { NodeLink, NodeLinkDirection } from "../../api/links";
import { Button } from "../../shared/ui";

type NodeLinkSectionProps = {
  id?: string;
  direction: NodeLinkDirection;
  title: string;
  emptyMessage: string;
  expanded: boolean;
  links: NodeLink[];
  loading: boolean;
  error: boolean;
  loadMoreError: boolean;
  fetchingMore: boolean;
  hasMore: boolean;
  onToggle: () => void;
  onRetry: () => void;
  onLoadMore: () => void;
  onOpenNode: (nodeId: string) => void;
};

export function NodeLinkSection({
  id,
  direction,
  title,
  emptyMessage,
  expanded,
  links,
  loading,
  error,
  loadMoreError,
  fetchingMore,
  hasMore,
  onToggle,
  onRetry,
  onLoadMore,
  onOpenNode
}: NodeLinkSectionProps) {
  const panelId = useId();
  const scrollRef = useRef<HTMLDivElement>(null);

  return (
    <section id={id} className={expanded ? "flex min-h-0 min-w-0 flex-col" : "min-w-0 shrink-0"}>
      <button
        type="button"
        className="flex min-h-workbench-control w-full shrink-0 items-center gap-2 px-1 py-1.5 text-left outline-none transition hover:bg-[var(--ng-hover)] focus-visible:ring-2 focus-visible:ring-inset focus-visible:ring-primary/45"
        aria-expanded={expanded}
        aria-controls={panelId}
        onClick={onToggle}
      >
        <span className="min-w-0 flex-1 text-xs font-semibold text-text">{title}</span>
        <span className="text-xs tabular-nums text-muted">
          <span aria-hidden="true">{links.length}{hasMore ? "+" : ""}</span>
          <span className="sr-only">
            {links.length} links loaded{hasMore ? ", more available" : ""}
          </span>
        </span>
        <ChevronRight
          size={14}
          className={`shrink-0 text-muted transition-transform ${expanded ? "rotate-90" : ""}`}
          aria-hidden="true"
        />
      </button>
      <div
        ref={scrollRef}
        id={panelId}
        role="region"
        aria-label={title}
        tabIndex={0}
        hidden={!expanded}
        className="min-h-0 flex-1 overflow-y-auto"
      >
        {loading ? <p className="px-1 py-2 text-xs text-muted">Loading links…</p> : null}
        {error ? (
          <div className="flex items-center justify-between gap-2 px-1 py-2">
            <p className="text-xs text-danger">Could not load links.</p>
            <Button secondary size="xs" onClick={onRetry}>Retry</Button>
          </div>
        ) : null}
        {!loading && !error && links.length === 0 ? (
          <p className="px-1 py-2 text-xs text-muted">{emptyMessage}</p>
        ) : null}
        {links.length > 0 ? (
          <ul className="divide-y divide-seam">
            {links.map((link) => (
              <li key={`${link.kind}:${link.path}`}>
                <LinkRow direction={direction} link={link} onOpenNode={onOpenNode} />
              </li>
            ))}
          </ul>
        ) : null}
        {hasMore || fetchingMore ? (
          <AutoPageSentinel
            rootRef={scrollRef}
            active={expanded && hasMore && !error && !loadMoreError}
            fetching={fetchingMore}
            onLoadMore={onLoadMore}
          />
        ) : null}
        {loadMoreError ? (
          <div className="flex items-center justify-between gap-2 px-1 py-2">
            <p className="text-xs text-danger">Could not load more links.</p>
            <Button secondary size="xs" onClick={onLoadMore}>Retry</Button>
          </div>
        ) : null}
      </div>
    </section>
  );
}

function AutoPageSentinel({
  rootRef,
  active,
  fetching,
  onLoadMore
}: {
  rootRef: RefObject<HTMLDivElement | null>;
  active: boolean;
  fetching: boolean;
  onLoadMore: () => void;
}) {
  const sentinelRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    const root = rootRef.current;
    const sentinel = sentinelRef.current;
    if (!active || fetching || !root || !sentinel || typeof IntersectionObserver === "undefined") return;

    let requested = false;
    const observer = new IntersectionObserver((entries) => {
      if (!requested && entries[0]?.isIntersecting) {
        requested = true;
        observer.disconnect();
        onLoadMore();
      }
    }, { root, rootMargin: "160px 0px" });
    observer.observe(sentinel);
    return () => observer.disconnect();
  }, [active, fetching, onLoadMore, rootRef]);

  return (
    <div ref={sentinelRef} className="flex min-h-8 items-center justify-center" aria-live="polite">
      {fetching ? <span className="text-xs text-muted">Loading…</span> : null}
    </div>
  );
}

function LinkRow({
  direction,
  link,
  onOpenNode
}: {
  direction: NodeLinkDirection;
  link: NodeLink;
  onOpenNode: (nodeId: string) => void;
}) {
  const content = (
    <>
      {link.node_id ? (
        <DirectionalNodeIcon direction={direction} kind={link.kind} />
      ) : (
        <span className="relative size-4 shrink-0 text-danger" aria-hidden="true">
          <FileText size={14} className="absolute left-0 top-0" />
          <AlertTriangle size={9} className="absolute -bottom-0.5 -right-0.5" />
        </span>
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

function DirectionalNodeIcon({
  direction,
  kind
}: {
  direction: NodeLinkDirection;
  kind: NodeLink["kind"];
}) {
  const BaseIcon = kind === "image" ? Image : FileText;
  const DirectionIcon = direction === "outgoing" ? ArrowRight : ArrowLeft;
  const directionPosition = direction === "outgoing" ? "-bottom-0.5 -right-1" : "-bottom-0.5 -left-1";
  const basePosition = direction === "outgoing" ? "left-0" : "right-0";

  return (
    <span className="relative size-4 shrink-0" aria-hidden="true">
      <BaseIcon size={14} className={`absolute top-0 ${basePosition}`} />
      <DirectionIcon size={9} className={`absolute text-primary ${directionPosition}`} strokeWidth={2.75} />
    </span>
  );
}
