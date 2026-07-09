import type { InfiniteData, UseInfiniteQueryResult } from "@tanstack/react-query";
import { FileClock, History, RefreshCw, ShieldCheck } from "lucide-react";
import { useEffect, useMemo, useState, type ReactNode } from "react";

import type { AuditEvent, AuditEventListResponse, FileChangeEvent, FileChangeEventListResponse, RestNode, Space } from "../../api/types";
import { Badge, Button, EmptyState, Modal, Tabs } from "../../shared/ui";
import { formatEventTime, formatMetadata, shortId } from "./eventDisplay";
import { useAuditEventsQuery, useFileChangeEventsQuery } from "./useEventHistoryQueries";

type Tab = "audit" | "files";
type FileScope = "space" | "node";
type EventListResponse = AuditEventListResponse | FileChangeEventListResponse;
type EventHistoryQuery<T extends EventListResponse> = UseInfiniteQueryResult<InfiniteData<T, unknown>, Error>;

const TABS: { id: Tab; label: string }[] = [
  { id: "audit", label: "Audit" },
  { id: "files", label: "File changes" }
];

export function EventHistoryModal({ activeSpace, activeNode, canViewAuditEvents, onClose }: { activeSpace: Space | null; activeNode: RestNode | null; canViewAuditEvents: boolean; onClose: () => void }) {
  const [tab, setTab] = useState<Tab>(canViewAuditEvents ? "audit" : "files");
  const [fileScope, setFileScope] = useState<FileScope>("space");
  const tabs = useMemo(() => TABS.filter((item) => item.id !== "audit" || canViewAuditEvents), [canViewAuditEvents]);
  const activeNodeInSpace = activeSpace && activeNode?.space_id === activeSpace.id ? activeNode : null;
  const selectedNodeId = fileScope === "node" ? activeNodeInSpace?.id ?? null : null;
  const auditQuery = useAuditEventsQuery(canViewAuditEvents && tab === "audit");
  const fileQuery = useFileChangeEventsQuery(activeSpace?.id ?? null, selectedNodeId, tab === "files");

  useEffect(() => {
    if (!canViewAuditEvents && tab === "audit") setTab("files");
  }, [canViewAuditEvents, tab]);

  useEffect(() => {
    if (fileScope === "node" && !activeNodeInSpace) setFileScope("space");
  }, [activeNodeInSpace, fileScope]);

  return (
    <Modal title="History" onClose={onClose} width="max-w-5xl">
      <Tabs items={tabs} value={tab} onChange={setTab} label="History sections" />
      <div className="min-h-[34rem] max-h-[min(68vh,42rem)] overflow-y-auto pr-1">
        {tab === "audit" ? <AuditEventsPanel query={auditQuery} /> : null}
        {tab === "files" ? (
          <FileChangeEventsPanel
            query={fileQuery}
            activeSpace={activeSpace}
            activeNode={activeNodeInSpace}
            fileScope={fileScope}
            onFileScopeChange={setFileScope}
          />
        ) : null}
      </div>
    </Modal>
  );
}

function AuditEventsPanel({ query }: { query: EventHistoryQuery<AuditEventListResponse> }) {
  const events = useMemo(() => query.data?.pages.flatMap((page) => page.events) ?? [], [query.data]);
  return (
    <section className="space-y-3">
      <PanelToolbar icon={<ShieldCheck size={16} />} title="Account audit" isFetching={query.isFetching} onRefresh={() => { void query.refetch(); }} />
      <EventQueryState query={query} emptyLabel="No audit events." />
      {events.length > 0 ? (
        <ol className="divide-y divide-seam rounded-xl border border-border bg-surface">
          {events.map((event) => (
            <li key={event.id} className="grid gap-3 px-4 py-3 text-sm md:grid-cols-[9rem_minmax(11rem,14rem)_minmax(0,1fr)_minmax(0,1.2fr)]">
              <EventTime value={event.created_at} />
              <div className="min-w-0">
                <Badge className="normal-case">{event.op_type}</Badge>
                <div className="mt-1 text-xs text-muted">{event.source}</div>
              </div>
              <div className="min-w-0">
                <div className="truncate font-medium">{event.resource_type}</div>
                <div className="truncate font-mono text-xs text-muted" title={event.resource_id ?? undefined}>{shortId(event.resource_id)}</div>
              </div>
              <EventMeta actorAccountId={event.actor_account_id} metadata={event.metadata} />
            </li>
          ))}
        </ol>
      ) : null}
      <LoadMore query={query} />
    </section>
  );
}

function FileChangeEventsPanel({
  query,
  activeSpace,
  activeNode,
  fileScope,
  onFileScopeChange
}: {
  query: EventHistoryQuery<FileChangeEventListResponse>;
  activeSpace: Space | null;
  activeNode: RestNode | null;
  fileScope: FileScope;
  onFileScopeChange: (scope: FileScope) => void;
}) {
  const events = useMemo(() => query.data?.pages.flatMap((page) => page.events) ?? [], [query.data]);
  return (
    <section className="space-y-3">
      <div className="flex flex-wrap items-center justify-between gap-3">
        <div className="min-w-0">
          <div className="flex items-center gap-2 text-sm font-semibold"><FileClock size={16} /> File changes</div>
          <div className="mt-1 truncate text-xs text-muted">{activeSpace?.name ?? "No space selected"}</div>
        </div>
        <div className="flex items-center gap-2">
          <div className="flex rounded-[10px] border border-border bg-bg p-0.5">
            <button
              type="button"
              className={`rounded-lg px-2.5 py-1 text-xs font-medium transition ${fileScope === "space" ? "bg-[var(--ng-selection)] text-text" : "text-muted hover:bg-[var(--ng-hover)] hover:text-text"}`}
              onClick={() => onFileScopeChange("space")}
            >
              Space
            </button>
            <button
              type="button"
              className={`rounded-lg px-2.5 py-1 text-xs font-medium transition ${fileScope === "node" ? "bg-[var(--ng-selection)] text-text" : "text-muted hover:bg-[var(--ng-hover)] hover:text-text"} disabled:cursor-not-allowed disabled:opacity-50`}
              onClick={() => onFileScopeChange("node")}
              disabled={!activeNode}
            >
              Node
            </button>
          </div>
          <Button size="sm" secondary onClick={() => { void query.refetch(); }} disabled={!activeSpace || query.isFetching}>
            <RefreshCw size={14} className={query.isFetching ? "animate-spin" : ""} /> Refresh
          </Button>
        </div>
      </div>
      {activeNode && fileScope === "node" ? <div className="truncate rounded-lg border border-border bg-surface px-3 py-2 text-xs text-muted">{activeNode.path}</div> : null}
      {!activeSpace ? <EmptyState>No space selected.</EmptyState> : <EventQueryState query={query} emptyLabel="No file change events." />}
      {events.length > 0 ? (
        <ol className="divide-y divide-seam rounded-xl border border-border bg-surface">
          {events.map((event) => (
            <li key={event.id} className="grid gap-3 px-4 py-3 text-sm md:grid-cols-[9rem_minmax(10rem,13rem)_minmax(0,1fr)_minmax(0,1.2fr)]">
              <EventTime value={event.created_at} />
              <div className="min-w-0">
                <Badge className="normal-case">{event.op_type}</Badge>
                <div className="mt-1 truncate font-mono text-xs text-muted" title={event.node_id ?? undefined}>{shortId(event.node_id)}</div>
              </div>
              <div className="min-w-0">
                <div className="text-xs text-muted">actor</div>
                <div className="truncate font-mono text-xs" title={event.actor_account_id ?? undefined}>{shortId(event.actor_account_id)}</div>
              </div>
              <code className="block min-w-0 break-words rounded-lg bg-bg px-2 py-1 font-mono text-xs text-muted">{formatMetadata(event.metadata)}</code>
            </li>
          ))}
        </ol>
      ) : null}
      <LoadMore query={query} />
    </section>
  );
}

function PanelToolbar({ icon, title, isFetching, onRefresh }: { icon: ReactNode; title: string; isFetching: boolean; onRefresh: () => void }) {
  return (
    <div className="flex flex-wrap items-center justify-between gap-3">
      <div className="flex items-center gap-2 text-sm font-semibold">{icon}{title}</div>
      <Button size="sm" secondary onClick={onRefresh} disabled={isFetching}>
        <RefreshCw size={14} className={isFetching ? "animate-spin" : ""} /> Refresh
      </Button>
    </div>
  );
}

function EventQueryState<T extends EventListResponse>({ query, emptyLabel }: { query: EventHistoryQuery<T>; emptyLabel: string }) {
  const eventCount = query.data?.pages.reduce((count, page) => count + page.events.length, 0) ?? 0;
  if (query.isLoading) return <div className="text-sm text-muted">Loading…</div>;
  if (query.isError) return <EmptyState>Could not load history.</EmptyState>;
  if (eventCount === 0) return <EmptyState>{emptyLabel}</EmptyState>;
  return null;
}

function EventTime({ value }: { value: string }) {
  return (
    <time className="text-xs text-muted" dateTime={value}>
      <History size={14} className="mr-1 inline-block align-[-2px]" />
      {formatEventTime(value)}
    </time>
  );
}

function EventMeta({ actorAccountId, metadata }: { actorAccountId: string | null; metadata: Record<string, unknown> }) {
  return (
    <div className="min-w-0 space-y-1">
      <div className="truncate font-mono text-xs text-muted" title={actorAccountId ?? undefined}>actor {shortId(actorAccountId)}</div>
      <code className="block min-w-0 break-words rounded-lg bg-bg px-2 py-1 font-mono text-xs text-muted">{formatMetadata(metadata)}</code>
    </div>
  );
}

function LoadMore<T extends EventListResponse>({ query }: { query: EventHistoryQuery<T> }) {
  if (!query.hasNextPage) return null;
  return (
    <div className="flex justify-center">
      <Button size="sm" secondary onClick={() => { void query.fetchNextPage(); }} disabled={query.isFetchingNextPage}>
        {query.isFetchingNextPage ? "Loading…" : "Load more"}
      </Button>
    </div>
  );
}
