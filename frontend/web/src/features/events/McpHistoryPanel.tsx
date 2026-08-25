import { useMemo } from "react";

import type { McpInvocation } from "../../api/types";
import { formatActor, shortId } from "./eventDisplay";
import { EventQueryState, EventTime, LoadMore, RefreshButton } from "./eventHistoryPrimitives";
import { useMcpInvocationsQuery } from "./useEventHistoryQueries";

export function McpInvocationsPanel() {
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
