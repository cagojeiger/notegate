import type { InfiniteData, UseInfiniteQueryResult } from "@tanstack/react-query";
import { ChevronRight, History, RefreshCw } from "lucide-react";
import { useEffect, useId, useMemo, useState } from "react";

import type { AuditEventListResponse, FileChangeEvent, FileChangeEventListResponse, Space } from "../../api/types";
import { Button, EmptyState, Modal, SelectField, Tabs } from "../../shared/ui";
import {
  formatActor,
  formatAuditAction,
  formatAuditDetail,
  formatAuditTarget,
  formatEventTime,
  formatEventTimeCompact,
  formatFileChangeAction,
  formatFileChangeDetails,
  formatFileChangeTarget
} from "./eventDisplay";
import { useAuditEventsQuery, useFileChangeEventsQuery } from "./useEventHistoryQueries";

type HistoryTab = "audit" | "files";
type EventListResponse = AuditEventListResponse | FileChangeEventListResponse;
type EventHistoryQuery<T extends EventListResponse> = UseInfiniteQueryResult<InfiniteData<T, unknown>, Error>;

const TABS: { id: HistoryTab; label: string }[] = [
  { id: "files", label: "Changes" },
  { id: "audit", label: "Audit log" }
];

export function EventHistoryModal({
  spaces,
  initialSpaceId,
  canViewAuditEvents,
  onClose
}: {
  spaces: Space[];
  initialSpaceId: string | null;
  canViewAuditEvents: boolean;
  onClose: () => void;
}) {
  const [tab, setTab] = useState<HistoryTab>("files");
  const tabs = useMemo(() => TABS.filter((item) => item.id !== "audit" || canViewAuditEvents), [canViewAuditEvents]);

  useEffect(() => {
    if (!canViewAuditEvents && tab === "audit") setTab("files");
  }, [canViewAuditEvents, tab]);

  return (
    <Modal title="History" onClose={onClose} width="max-w-5xl">
      <Tabs items={tabs} value={tab} onChange={setTab} label="History sections" />
      <div className="min-h-[34rem] max-h-[min(68vh,42rem)] overflow-y-auto pr-1">
        {canViewAuditEvents && tab === "audit" ? <AuditEventsPanel /> : null}
        {tab === "files" ? <FileChangeEventsPanel spaces={spaces} initialSpaceId={initialSpaceId} /> : null}
      </div>
    </Modal>
  );
}

function AuditEventsPanel() {
  const query = useAuditEventsQuery();
  const events = useMemo(() => query.data?.pages.flatMap((page) => page.events) ?? [], [query.data]);
  return (
    <section className="space-y-3">
      <div className="flex justify-end">
        <RefreshButton isFetching={query.isFetching} onRefresh={() => { void query.refetch(); }} />
      </div>
      <EventQueryState query={query} emptyLabel="No audit events." />
      {events.length > 0 ? (
        <ol className="rounded-lg border border-border bg-surface px-4">
          {events.map((event) => {
            const detail = formatAuditDetail(event);
            const action = formatAuditAction(event);
            const target = formatAuditTarget(event);
            const actor = formatActor(event.actor, event.actor_account_id);
            return (
              <li key={event.id} className="group relative flex gap-3 border-b border-seam py-2 last:border-b-0">
                <div className="relative flex w-4 shrink-0 justify-center" aria-hidden="true">
                  <span className="absolute bottom-[-0.75rem] top-3 w-px bg-seam group-last:hidden" />
                  <span className="relative mt-1.5 size-2 rounded-full bg-primary ring-4 ring-surface" />
                </div>
                <div className="min-w-0 flex-1">
                  <div className="flex items-baseline justify-between gap-3">
                    <div className="truncate text-sm font-medium text-text">{action}</div>
                    <EventTime value={event.created_at} />
                  </div>
                  <div className="mt-0.5 flex min-w-0 items-center gap-1.5 text-xs text-muted">
                    <span className="truncate" title={event.resource_id ?? undefined}>{target}</span>
                    {detail ? <><span className="shrink-0" aria-hidden="true">·</span><span className="shrink-0">{detail}</span></> : null}
                    <span className="shrink-0" aria-hidden="true">·</span>
                    <span className="truncate" title={event.actor_account_id ?? undefined}>{actor}</span>
                  </div>
                </div>
              </li>
            );
          })}
        </ol>
      ) : null}
      <LoadMore query={query} />
    </section>
  );
}

function FileChangeEventsPanel({
  spaces,
  initialSpaceId
}: {
  spaces: Space[];
  initialSpaceId: string | null;
}) {
  const [selectedSpaceId, setSelectedSpaceId] = useState(() => selectInitialSpaceId(spaces, initialSpaceId));
  const selectedSpace = spaces.find((space) => space.id === selectedSpaceId) ?? null;
  const query = useFileChangeEventsQuery(selectedSpace?.id ?? null, null);
  const events = useMemo(() => query.data?.pages.flatMap((page) => page.events) ?? [], [query.data]);

  useEffect(() => {
    if (!selectedSpace) setSelectedSpaceId(spaces[0]?.id ?? null);
  }, [selectedSpace, spaces]);

  return (
    <section className="space-y-3">
      <div className="flex flex-wrap items-center justify-between gap-3">
        <SelectField label="Space" className="w-full sm:w-72" value={selectedSpaceId ?? ""} onChange={(event) => setSelectedSpaceId(event.target.value || null)} disabled={spaces.length === 0}>
          {spaces.length === 0 ? <option value="">No spaces available</option> : null}
          {spaces.map((space) => <option key={space.id} value={space.id}>{space.name}</option>)}
        </SelectField>
        <RefreshButton isFetching={query.isFetching} onRefresh={() => { void query.refetch(); }} disabled={!selectedSpace} />
      </div>
      {!selectedSpace ? <EmptyState>No space selected.</EmptyState> : <EventQueryState query={query} emptyLabel="No changes yet." />}
      {events.length > 0 ? (
        <ol className="rounded-lg border border-border bg-surface px-4">
          {events.map((event) => <FileChangeEventRow key={event.id} event={event} />)}
        </ol>
      ) : null}
      <LoadMore query={query} />
    </section>
  );
}

function FileChangeEventRow({ event }: { event: FileChangeEvent }) {
  const [open, setOpen] = useState(false);
  const detailsId = useId();
  const target = formatFileChangeTarget(event);
  const actor = formatActor(event.actor, event.actor_account_id);
  const details = formatFileChangeDetails(event);
  const toggleLabel = `${open ? "Hide" : "Show"} change details for ${target}`;

  return (
    <li className="group relative flex gap-3 border-b border-seam py-2 last:border-b-0">
      <div className="relative flex w-4 shrink-0 justify-center" aria-hidden="true">
        <span className="absolute bottom-[-0.75rem] top-3 w-px bg-seam group-last:hidden" />
        <span className="relative mt-1.5 size-2 rounded-full bg-primary ring-4 ring-surface" />
      </div>
      <div className="min-w-0 flex-1">
        <div className="flex items-baseline justify-between gap-3">
          <div className="truncate text-sm font-medium text-text">{formatFileChangeAction(event)}</div>
          <div className="flex shrink-0 items-center gap-1">
            <EventTime value={event.created_at} />
            {details.length > 0 ? (
              <button
                type="button"
                aria-label={toggleLabel}
                aria-expanded={open}
                aria-controls={detailsId}
                title={toggleLabel}
                onClick={() => setOpen((value) => !value)}
                className="grid size-7 place-items-center rounded-lg text-muted outline-none transition hover:bg-[var(--ng-hover)] hover:text-text focus-visible:ring-2 focus-visible:ring-primary/45"
              >
                <ChevronRight size={14} className={`transition ${open ? "rotate-90" : ""}`} />
              </button>
            ) : null}
          </div>
        </div>
        <div className="mt-0.5 flex min-w-0 items-center gap-1.5 text-xs text-muted">
          <span className="truncate font-mono" title={event.node_id ?? undefined}>{target}</span>
          <span className="shrink-0" aria-hidden="true">·</span>
          <span className="truncate" title={event.actor_account_id ?? undefined}>{actor}</span>
        </div>
        {open ? (
          <dl id={detailsId} className="mt-3 grid gap-x-6 gap-y-2 border-t border-seam pt-3 text-xs sm:grid-cols-2">
            <div className="flex min-w-0 items-baseline justify-between gap-3 sm:hidden">
              <dt className="text-muted">Actor</dt>
              <dd className="truncate text-text" title={actor}>{actor}</dd>
            </div>
            {details.map((detail) => (
              <div key={detail.label} className="flex min-w-0 items-baseline justify-between gap-3">
                <dt className="text-muted">{detail.label}</dt>
                <dd className="truncate font-mono text-text" title={detail.value}>{detail.value}</dd>
              </div>
            ))}
          </dl>
        ) : null}
      </div>
    </li>
  );
}

function selectInitialSpaceId(spaces: Space[], initialSpaceId: string | null): string | null {
  return spaces.some((space) => space.id === initialSpaceId) ? initialSpaceId : spaces[0]?.id ?? null;
}

function RefreshButton({
  isFetching,
  onRefresh,
  disabled = false
}: {
  isFetching: boolean;
  onRefresh: () => void;
  disabled?: boolean;
}) {
  return (
    <Button size="sm" secondary onClick={onRefresh} disabled={disabled || isFetching}>
      <RefreshCw size={14} className={isFetching ? "animate-spin" : ""} /> Refresh
    </Button>
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
      <span className="sm:hidden">{formatEventTimeCompact(value)}</span>
      <span className="hidden sm:inline">{formatEventTime(value)}</span>
    </time>
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
