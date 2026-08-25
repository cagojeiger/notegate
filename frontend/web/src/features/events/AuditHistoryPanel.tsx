import { useMemo } from "react";

import {
  formatActor,
  formatAuditAction,
  formatAuditDetail,
  formatAuditTarget
} from "./eventDisplay";
import { EventQueryState, EventTime, LoadMore, RefreshButton } from "./eventHistoryPrimitives";
import { useAuditEventsQuery } from "./useEventHistoryQueries";

export function AuditEventsPanel() {
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
