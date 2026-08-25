import { ChevronRight } from "lucide-react";
import { useEffect, useId, useMemo, useState } from "react";

import type { FileChangeEvent, Space } from "../../api/types";
import { EmptyState, SelectField } from "../../shared/ui";
import {
  formatActor,
  formatFileChangeAction,
  formatFileChangeDetails,
  formatFileChangeTarget
} from "./eventDisplay";
import { EventQueryState, EventTime, LoadMore, RefreshButton } from "./eventHistoryPrimitives";
import { useFileChangeEventsQuery } from "./useEventHistoryQueries";

export function FileChangeEventsPanel({
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
