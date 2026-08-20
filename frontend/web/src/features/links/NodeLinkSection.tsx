import { useEffect, useId, useRef, type RefObject } from "react";
import { AlertTriangle, ArrowLeftToLine, ArrowRightFromLine, ChevronRight, FileText, Image } from "lucide-react";

import type { NodeLink, NodeLinkDirection } from "../../api/links";
import { Button } from "../../shared/ui";

type NodeLinkSectionProps = {
  id?: string;
  direction: NodeLinkDirection;
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
  const DirectionIcon = direction === "outgoing" ? ArrowRightFromLine : ArrowLeftToLine;
  const title = direction === "outgoing" ? "Outgoing" : "Incoming";
  const accessibleTitle = direction === "outgoing"
    ? "Links from this document"
    : "Links to this document";

  return (
    <section id={id} className={expanded ? "flex min-h-0 min-w-0 flex-col" : "min-w-0 shrink-0"}>
      <button
        type="button"
        className="flex min-h-workbench-control w-full shrink-0 items-center gap-2 px-1 py-1.5 text-left outline-none transition hover:bg-[var(--ng-hover)] focus-visible:ring-2 focus-visible:ring-inset focus-visible:ring-primary/45"
        aria-label={`${accessibleTitle}, ${links.length} links loaded${hasMore ? ", more available" : ""}`}
        aria-expanded={expanded}
        aria-controls={panelId}
        onClick={onToggle}
      >
        <span className="flex min-w-0 flex-1 items-center gap-1.5 text-xs font-semibold text-text">
          <DirectionIcon size={13} className="shrink-0 text-primary" aria-hidden="true" />
          <span>{title}</span>
        </span>
        <span className="text-xs tabular-nums text-muted">
          <span aria-hidden="true">{links.length}{hasMore ? "+" : ""}</span>
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
        aria-label={accessibleTitle}
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
                <LinkRow link={link} onOpenNode={onOpenNode} />
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
  link,
  onOpenNode
}: {
  link: NodeLink;
  onOpenNode: (nodeId: string) => void;
}) {
  const Icon = link.kind === "image" ? Image : FileText;
  const name = linkName(link.path);
  const content = (
    <>
      {link.node_id ? (
        <Icon size={14} className="mt-0.5 shrink-0" aria-hidden="true" />
      ) : (
        <span className="relative mt-0.5 size-4 shrink-0 text-danger" aria-hidden="true">
          <FileText size={14} className="absolute left-0 top-0" />
          <AlertTriangle size={9} className="absolute -bottom-0.5 -right-0.5" />
        </span>
      )}
      <span className="min-w-0 flex-1 space-y-0.5 text-left">
        <span className="block break-words text-text">{name}</span>
        <span className="block [overflow-wrap:anywhere] text-xs leading-4 text-faint">{link.path}</span>
      </span>
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

function linkName(path: string): string {
  const separator = path.lastIndexOf("/");
  return path.slice(separator + 1) || path;
}
