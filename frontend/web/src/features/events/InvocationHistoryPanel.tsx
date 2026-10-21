import { useMemo } from "react";
import { ChevronRight } from "lucide-react";

import { ListItemButton } from "../../shared/ui";

import { useSettingsViewState } from "../settings/settingsViewState";
import { SettingsControls, SettingsInspectorSection, useSettingsInspector } from "../settings/SettingsInspector";
import type { CommandInvocation, CommandInvocationSurface } from "../../api/types";
import { formatActor, shortId } from "./eventDisplay";
import { EventQueryState, EventTime, LoadMore, RefreshButton } from "./eventHistoryPrimitives";
import { useCommandInvocationsQuery } from "./useEventHistoryQueries";

export function CommandInvocationsPanel({ surface }: { surface: CommandInvocationSurface }) {
  const query = useCommandInvocationsQuery(surface);
  const inspector = useSettingsInspector();
  const [selectedId, setSelectedId] = useSettingsViewState<number | null>(`history-${surface}-selected`, null);
  const invocations = useMemo(
    () => query.data?.pages.flatMap((page) => page.command_invocations) ?? [],
    [query.data]
  );
  const selected = invocations.find((item) => item.id === selectedId);
  return (
    <>
      <RefreshButton isFetching={query.isFetching} onRefresh={() => { void query.refetch(); }} />
      <section className="w-full space-y-3">
        <EventQueryState
          query={query}
          itemCount={invocations.length}
          emptyLabel={`No ${surface.toUpperCase()} calls.`}
        />
        {invocations.length > 0 ? (
          <ol className="rounded-workbench-surface border border-border bg-surface">
            {invocations.map((invocation) => (
              <CommandInvocationRow key={invocation.id} invocation={invocation} selected={selectedId === invocation.id}
                onSelect={() => { setSelectedId(invocation.id); inspector?.onOpen(); }} />
            ))}
          </ol>
        ) : null}
        <LoadMore query={query} />
        <SettingsControls><SettingsInspectorSection title="Selected call">
          {selected ? <CommandInvocationDetails key={selected.id} invocation={selected} /> : <p className="text-xs text-muted">Select a call to view its details.</p>}
        </SettingsInspectorSection></SettingsControls>
      </section>
    </>
  );
}

function invocationPresentation(invocation: CommandInvocation) {
  const actor = invocation.actor
    ? formatActor(invocation.actor, invocation.actor_account_id)
    : `${invocation.caller_kind === "agent" ? "Agent" : "User"} ${shortId(invocation.actor_account_id)}`;
  const operation = invocation.surface === "api"
    ? invocation.op ?? "HTTP request"
    : invocation.op ? `${invocation.tool} · ${invocation.op}` : invocation.tool;
  const status = invocation.outcome === "success"
    ? "Success"
    : `Error${invocation.error_code ? ` · ${invocation.error_code}` : ""}`;
  const duration = invocation.duration_ms < 1_000
    ? `${invocation.duration_ms} ms`
    : `${(invocation.duration_ms / 1_000).toFixed(2)} s`;
  const purpose = invocation.purpose
    ?? (invocation.surface === "api" ? "API v2 request" : invocation.tool === "me" ? "Checked caller identity" : "Purpose not recorded");

  return { actor, operation, status, duration, purpose };
}

function CommandInvocationRow({ invocation, selected, onSelect }: { invocation: CommandInvocation; selected: boolean; onSelect: () => void }) {
  const { actor, operation, status, purpose } = invocationPresentation(invocation);
  return (
    <li className="group relative border-b border-seam last:border-b-0">
      <ListItemButton aria-label={`View ${invocation.surface.toUpperCase()} call ${invocation.id}: ${purpose}. ${actor}. ${status}`} selected={selected}
        onClick={onSelect} className="items-start">
        <div className="relative flex w-4 shrink-0 justify-center" aria-hidden="true">
          <span className="absolute bottom-[-0.75rem] top-3 w-px bg-seam group-last:hidden" />
          <span className={`relative mt-1.5 size-2 rounded-full ring-4 ring-surface ${invocation.outcome === "success" ? "bg-success" : "bg-danger"}`} />
        </div>
        <div className="min-w-0 flex-1">
          <div className="flex items-baseline justify-between gap-3">
            <div className="min-w-0 line-clamp-2 break-words text-workbench font-medium text-text">
              {purpose}
            </div>
            <EventTime value={invocation.created_at} />
          </div>
          {!invocation.purpose && invocation.tool !== "me" ? <p className="mt-0.5 truncate font-mono text-xs text-muted" title={operation}>{operation}</p> : null}
          <div className="mt-0.5 flex min-w-0 flex-wrap items-center gap-1.5 text-xs text-muted">
            {invocation.space_name ? (
              <>
                <span className="truncate" title={invocation.space_name}>Space {invocation.space_name}</span>
                <span aria-hidden="true">·</span>
              </>
            ) : null}
            <span className="truncate" title={invocation.actor_account_id}>{actor}</span>
            <span aria-hidden="true">·</span>
            <span className={invocation.outcome === "success" ? "text-success" : "text-danger"}>{status}</span>
          </div>
        </div>
        <ChevronRight size={15} className="mt-1 shrink-0 text-muted" />
      </ListItemButton>
    </li>
  );
}

function CommandInvocationDetails({ invocation }: { invocation: CommandInvocation }) {
  const { actor, operation, status, duration, purpose } = invocationPresentation(invocation);
  return <section aria-label={`Details for ${invocation.surface.toUpperCase()} call ${invocation.id}`} className="min-w-0 space-y-3 text-sm">
    <p className="break-words font-medium">{purpose}</p>
    <p><EventTime value={invocation.created_at} /></p>
    <dl className="space-y-3 text-xs">
      <div><dt className="text-muted">Operation</dt><dd className="break-all font-mono">{operation}</dd></div>
      {invocation.invocation_id ? <div><dt className="text-muted">Call ID</dt><dd className="break-all font-mono">{invocation.invocation_id}</dd></div> : null}
      <div><dt className="text-muted">Actor</dt><dd className="break-words">{actor}</dd></div>
      {invocation.space_name ? <div><dt className="text-muted">Space</dt><dd className="break-words">{invocation.space_name}</dd></div> : null}
      <div><dt className="text-muted">Result</dt><dd className={invocation.outcome === "success" ? "text-success" : "text-danger"}>{status}</dd></div>
      <div><dt className="text-muted">Duration</dt><dd>{duration}</dd></div>
    </dl>
    <InvocationDetails stateKey={`history-${invocation.surface}-${invocation.id}-input`} label="Input" value={invocation.input} />
    <InvocationDetails stateKey={`history-${invocation.surface}-${invocation.id}-response`} label="Response" value={invocation.response} />
  </section>;
}

function InvocationDetails({
  stateKey,
  label,
  value
}: {
  stateKey: string;
  label: "Input" | "Response";
  value: Record<string, unknown> | null | undefined;
}) {
  const [open, setOpen] = useSettingsViewState(stateKey, false);
  return (
    <details open={open}
      className="mt-2 text-xs"
      onToggle={(event) => { setOpen(event.currentTarget.open); }}
    >
      <summary className="min-h-workbench-control cursor-pointer select-none content-center text-muted hover:text-text">{label}</summary>
      {open && value ? (
        <pre className="mt-2 max-h-64 overflow-auto whitespace-pre-wrap break-all rounded-workbench-surface bg-bg p-3 font-mono text-text">
          {JSON.stringify(value, null, 2)}
        </pre>
      ) : null}
      {open && !value ? (
        <p className="mt-2 rounded-workbench-surface bg-bg p-3 text-muted">
          Not recorded. This call predates response logging.
        </p>
      ) : null}
    </details>
  );
}
