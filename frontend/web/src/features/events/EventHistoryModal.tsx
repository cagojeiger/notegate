import type { InfiniteData, UseInfiniteQueryResult } from "@tanstack/react-query";
import { ChevronRight, History, RefreshCw } from "lucide-react";
import { useEffect, useId, useMemo, useState } from "react";

import type { AuditEventListResponse, BackgroundJob, BackgroundJobListResponse, FileChangeEvent, FileChangeEventListResponse, McpInvocation, McpInvocationListResponse, Space } from "../../api/types";
import { Button, EmptyState, Modal, SelectField, Tabs } from "../../shared/ui";
import {
  formatActor,
  formatAuditAction,
  formatAuditDetail,
  formatAuditTarget,
  formatDurationBetween,
  formatEventTime,
  formatEventTimeCompact,
  formatFileChangeAction,
  formatFileChangeDetails,
  formatFileChangeTarget,
  shortId
} from "./eventDisplay";
import { useAuditEventsQuery, useBackgroundJobQuery, useBackgroundJobsQuery, useFileChangeEventsQuery, useMcpInvocationsQuery } from "./useEventHistoryQueries";

type HistoryTab = "audit" | "files" | "mcp" | "jobs";
type EventListResponse = AuditEventListResponse | BackgroundJobListResponse | FileChangeEventListResponse | McpInvocationListResponse;
type EventHistoryQuery<T extends EventListResponse> = UseInfiniteQueryResult<InfiniteData<T, unknown>, Error>;

const TABS: { id: HistoryTab; label: string }[] = [
  { id: "files", label: "Changes" },
  { id: "audit", label: "Audit" },
  { id: "mcp", label: "MCP" },
  { id: "jobs", label: "Jobs" }
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
  const tabs = useMemo(
    () => TABS.filter((item) => item.id === "files" || canViewAuditEvents),
    [canViewAuditEvents]
  );

  useEffect(() => {
    if (!canViewAuditEvents && tab !== "files") setTab("files");
  }, [canViewAuditEvents, tab]);

  return (
    <Modal title="History" onClose={onClose} width="max-w-5xl">
      <Tabs items={tabs} value={tab} onChange={setTab} label="History sections" />
      <div className="min-h-[20rem] max-h-[min(68vh,42rem)] overflow-y-auto pr-1 sm:min-h-[24rem]">
        {canViewAuditEvents && tab === "audit" ? <AuditEventsPanel /> : null}
        {canViewAuditEvents && tab === "mcp" ? <McpInvocationsPanel /> : null}
        {canViewAuditEvents && tab === "jobs" ? <BackgroundJobsPanel /> : null}
        {tab === "files" ? <FileChangeEventsPanel spaces={spaces} initialSpaceId={initialSpaceId} /> : null}
      </div>
    </Modal>
  );
}

function BackgroundJobsPanel() {
  const query = useBackgroundJobsQuery();
  const jobs = useMemo(() => query.data?.pages.flatMap((page) => page.jobs) ?? [], [query.data]);
  const activeCount = jobs.filter((job) => job.status === "queued" || job.status === "running").length;
  return (
    <section className="space-y-3">
      <div className="flex items-center justify-between gap-3">
        <p className="text-xs text-muted">
          {activeCount > 0 ? `${activeCount} active · updates automatically` : "Background activity for your account"}
        </p>
        <RefreshButton isFetching={query.isFetching} onRefresh={() => { void query.refetch(); }} />
      </div>
      <EventQueryState query={query} itemCount={jobs.length} emptyLabel="No background jobs yet." />
      {jobs.length > 0 ? (
        <ol className="rounded-lg border border-border bg-surface px-4">
          {jobs.map((job) => <BackgroundJobRow key={job.id} job={job} />)}
        </ol>
      ) : null}
      <LoadMore query={query} />
    </section>
  );
}

function BackgroundJobRow({ job }: { job: BackgroundJob }) {
  const [open, setOpen] = useState(false);
  const detail = useBackgroundJobQuery(job.id, open);
  const currentJob = detail.data?.job ?? job;
  const presentation = jobPresentation(currentJob);
  const attempts = detail.data?.attempts ?? [];
  const totalDuration = currentJob.completed_at
    ? formatDurationBetween(currentJob.created_at, currentJob.completed_at)
    : null;
  const attemptsId = useId();
  const toggleLabel = `${open ? "Hide" : "Show"} attempts for ${jobLabel(currentJob.kind)}`;

  return (
    <li className="group relative flex gap-3 border-b border-seam py-2 last:border-b-0">
      <div className="relative flex w-4 shrink-0 justify-center" aria-hidden="true">
        <span className="absolute bottom-[-0.75rem] top-3 w-px bg-seam group-last:hidden" />
        <span className={`relative mt-1.5 size-2 rounded-full ring-4 ring-surface ${presentation.dot}`} />
      </div>
      <div className="min-w-0 flex-1">
        <div className="flex min-w-0 flex-col gap-1 sm:flex-row sm:items-start sm:justify-between sm:gap-3">
          <div className="min-w-0">
            <div className="text-workbench font-medium text-text sm:truncate">{jobLabel(currentJob.kind)}</div>
            <div className="mt-0.5 flex min-w-0 flex-wrap items-center gap-1.5 text-xs text-muted">
              {currentJob.context_label ? <><span className="truncate" title={currentJob.context_label}>{formatJobContext(currentJob.context_kind, currentJob.context_label)}</span><span aria-hidden="true">·</span></> : null}
              <span className={presentation.text}>{presentation.label}</span>
              <span aria-hidden="true">·</span>
              <span>{currentJob.attempt_count} {currentJob.attempt_count === 1 ? "attempt" : "attempts"}</span>
              {totalDuration ? <><span aria-hidden="true">·</span><span>{totalDuration} total</span></> : null}
              {currentJob.last_error_code ? <><span aria-hidden="true">·</span><span className="font-mono text-danger">{currentJob.last_error_code}</span></> : null}
            </div>
          </div>
          <div className="flex items-center justify-between gap-1 sm:shrink-0 sm:justify-start">
            <JobTimes job={currentJob} />
            <button
              type="button"
              aria-label={toggleLabel}
              aria-expanded={open}
              aria-controls={attemptsId}
              title={toggleLabel}
              onClick={() => setOpen((value) => !value)}
              className="grid size-7 place-items-center rounded-lg text-muted outline-none transition hover:bg-[var(--ng-hover)] hover:text-text focus-visible:ring-2 focus-visible:ring-primary/45"
            >
              <ChevronRight size={14} className={`transition ${open ? "rotate-90" : ""}`} />
            </button>
          </div>
        </div>
        {open ? (
          <div id={attemptsId} className="mt-3 border-t border-seam pt-3 text-xs">
            {detail.isLoading ? <p className="text-muted">Loading attempts…</p> : null}
            {detail.isError ? <p className="text-danger">Could not load attempts.</p> : null}
            {detail.data && attempts.length === 0 ? <p className="text-muted">Waiting for the first attempt.</p> : null}
            {attempts.length > 0 ? (
              <ol className="space-y-2">
                {attempts.map((attempt) => {
                  const previousAttempt = attempts.find(
                    (candidate) => candidate.attempt_number === attempt.attempt_number - 1
                  );
                  const queuedAt = previousAttempt?.finished_at ?? (attempt.attempt_number === 1 ? currentJob.created_at : null);
                  const queueDuration = queuedAt ? formatDurationBetween(queuedAt, attempt.started_at) : null;
                  const runDuration = attempt.finished_at
                    ? formatDurationBetween(attempt.started_at, attempt.finished_at)
                    : null;
                  return (
                    <li key={attempt.attempt_number} className="flex flex-wrap items-center justify-between gap-x-4 gap-y-1 rounded-md bg-bg px-3 py-2">
                      <span className="font-medium text-text">Attempt {attempt.attempt_number}</span>
                      <div className="flex flex-wrap items-center gap-1.5 text-muted">
                        <span>{attempt.outcome ? formatAttemptOutcome(attempt.outcome) : "Running"}</span>
                        {queueDuration ? <><span aria-hidden="true">·</span><span>Queue {queueDuration}</span></> : null}
                        {runDuration ? <><span aria-hidden="true">·</span><span>Run {runDuration}</span></> : null}
                      </div>
                      <div className="flex flex-col items-end text-muted">
                        <LifecycleTime label="Started" value={attempt.started_at} />
                        {attempt.finished_at ? <LifecycleTime label="Finished" value={attempt.finished_at} /> : null}
                      </div>
                      {attempt.error_code ? <span className="font-mono text-danger">{attempt.error_code}</span> : null}
                    </li>
                  );
                })}
              </ol>
            ) : null}
          </div>
        ) : null}
      </div>
    </li>
  );
}

function jobLabel(kind: string) {
  if (kind === "space_usage_reconcile") return "Usage recalculation";
  if (kind === "link_graph_project_nodes") return "Link indexing";
  return kind;
}

function formatJobContext(kind: string | null, label: string) {
  if (!kind) return label;
  const displayKind = kind === "space"
    ? "Space"
    : kind.split("_").map((part) => part[0]?.toUpperCase() + part.slice(1)).join(" ");
  return `${displayKind} ${label}`;
}

function jobPresentation(job: BackgroundJob) {
  if (job.status === "running") return { label: "Running…", dot: "bg-primary animate-pulse", text: "text-primary" };
  if (job.status === "succeeded") return { label: "Completed", dot: "bg-success", text: "text-success" };
  if (job.status === "dead") return { label: "Failed", dot: "bg-danger", text: "text-danger" };
  if (job.attempt_count > 0) return { label: "Retrying", dot: "bg-warning", text: "text-warning" };
  return { label: "Waiting", dot: "bg-warning", text: "text-warning" };
}

function formatAttemptOutcome(outcome: string) {
  return outcome.split("_").map((part) => part[0]?.toUpperCase() + part.slice(1)).join(" ");
}

function JobTimes({ job }: { job: BackgroundJob }) {
  return (
    <div className="flex flex-wrap items-center gap-x-2 text-xs text-muted sm:flex-col sm:items-end sm:gap-x-0">
      <LifecycleTime label="Queued" value={job.created_at} />
      {job.completed_at ? <LifecycleTime label="Finished" value={job.completed_at} /> : null}
    </div>
  );
}

function LifecycleTime({ label, value }: { label: string; value: string }) {
  const full = formatEventTime(value);
  return (
    <time dateTime={value} aria-label={`${label} ${full}`} title={`${label} ${full}`}>
      <span>{label} </span>
      <span className="sm:hidden">{formatEventTimeCompact(value)}</span>
      <span className="hidden sm:inline">{full}</span>
    </time>
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
      <EventQueryState query={query} itemCount={events.length} emptyLabel="No audit events." />
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
                    <div className="truncate text-workbench font-medium text-text">{action}</div>
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

function McpInvocationsPanel() {
  const query = useMcpInvocationsQuery();
  const invocations = useMemo(
    () => query.data?.pages.flatMap((page) => page.invocations) ?? [],
    [query.data]
  );
  return (
    <section className="space-y-3">
      <div className="flex justify-end">
        <RefreshButton isFetching={query.isFetching} onRefresh={() => { void query.refetch(); }} />
      </div>
      <EventQueryState query={query} itemCount={invocations.length} emptyLabel="No MCP calls." />
      {invocations.length > 0 ? (
        <ol className="rounded-lg border border-border bg-surface px-4">
          {invocations.map((invocation) => (
            <McpInvocationRow key={invocation.id} invocation={invocation} />
          ))}
        </ol>
      ) : null}
      <LoadMore query={query} />
    </section>
  );
}

function McpInvocationRow({ invocation }: { invocation: McpInvocation }) {
  const actor = invocation.actor
    ? formatActor(invocation.actor, invocation.actor_account_id)
    : `${invocation.caller_kind === "agent" ? "Agent" : "User"} ${shortId(invocation.actor_account_id)}`;
  const operation = invocation.op ? `${invocation.tool} · ${invocation.op}` : invocation.tool;
  const status = invocation.outcome === "success"
    ? "Success"
    : `Error${invocation.error_code ? ` · ${invocation.error_code}` : ""}`;
  const duration = invocation.duration_ms < 1_000
    ? `${invocation.duration_ms} ms`
    : `${(invocation.duration_ms / 1_000).toFixed(2)} s`;

  return (
    <li className="group relative flex gap-3 border-b border-seam py-2 last:border-b-0">
      <div className="relative flex w-4 shrink-0 justify-center" aria-hidden="true">
        <span className="absolute bottom-[-0.75rem] top-3 w-px bg-seam group-last:hidden" />
        <span className={`relative mt-1.5 size-2 rounded-full ring-4 ring-surface ${invocation.outcome === "success" ? "bg-success" : "bg-danger"}`} />
      </div>
      <div className="min-w-0 flex-1">
        <div className="flex items-baseline justify-between gap-3">
          <div className="truncate text-workbench font-medium text-text" title={invocation.purpose ?? undefined}>
            {invocation.purpose ?? "Checked caller identity"}
          </div>
          <EventTime value={invocation.created_at} />
        </div>
        <div className="mt-0.5 flex min-w-0 flex-wrap items-center gap-1.5 text-xs text-muted">
          <span className="font-mono text-text">{operation}</span>
          {invocation.space_name ? (
            <>
              <span aria-hidden="true">·</span>
              <span className="truncate" title={invocation.space_name}>Space {invocation.space_name}</span>
            </>
          ) : null}
          <span aria-hidden="true">·</span>
          <span className="truncate" title={invocation.actor_account_id}>{actor}</span>
          <span aria-hidden="true">·</span>
          <span>{duration}</span>
          <span aria-hidden="true">·</span>
          <span className={invocation.outcome === "success" ? "text-success" : "text-danger"}>{status}</span>
        </div>
        <details className="mt-2 text-xs">
          <summary className="cursor-pointer select-none text-muted hover:text-text">Input</summary>
          <pre className="mt-2 max-h-64 overflow-auto whitespace-pre-wrap break-all rounded-md bg-bg p-3 font-mono text-text">
            {JSON.stringify(invocation.input, null, 2)}
          </pre>
        </details>
        <details className="mt-2 text-xs">
          <summary className="cursor-pointer select-none text-muted hover:text-text">Response</summary>
          {invocation.response ? (
            <pre className="mt-2 max-h-64 overflow-auto whitespace-pre-wrap break-all rounded-md bg-bg p-3 font-mono text-text">
              {JSON.stringify(invocation.response, null, 2)}
            </pre>
          ) : (
            <p className="mt-2 rounded-md bg-bg p-3 text-muted">
              Not recorded. This call predates response logging.
            </p>
          )}
        </details>
      </div>
    </li>
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
      {!selectedSpace ? <EmptyState>No space selected.</EmptyState> : <EventQueryState query={query} itemCount={events.length} emptyLabel="No changes yet." />}
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
          <div className="truncate text-workbench font-medium text-text">{formatFileChangeAction(event)}</div>
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

function EventQueryState<T extends EventListResponse>({ query, itemCount, emptyLabel }: { query: EventHistoryQuery<T>; itemCount: number; emptyLabel: string }) {
  if (query.isLoading) return <div className="text-workbench text-muted">Loading…</div>;
  if (query.isError) return <EmptyState>Could not load history.</EmptyState>;
  if (itemCount === 0) return <EmptyState>{emptyLabel}</EmptyState>;
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
