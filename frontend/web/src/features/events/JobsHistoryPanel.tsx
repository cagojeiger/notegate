import { ChevronRight } from "lucide-react";
import { useId, useMemo, useState } from "react";

import type { BackgroundJob } from "../../api/types";
import { formatDurationBetween } from "./eventDisplay";
import { EventQueryState, LifecycleTime, LoadMore, RefreshButton } from "./eventHistoryPrimitives";
import { useBackgroundJobQuery, useBackgroundJobsQuery } from "./useEventHistoryQueries";

export function BackgroundJobsPanel() {
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
