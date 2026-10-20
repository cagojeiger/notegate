import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { ChevronRight, Monitor, Terminal, Unplug } from "lucide-react";
import { useEffect, useRef, useState, type ReactNode } from "react";

import { useApiClient } from "../../api/ApiProvider";
import { invalidateAuditEvents } from "../../api/queryInvalidation";
import { queryKeys } from "../../api/queryKeys";
import { listBrowserSessions, listOAuthGrants } from "../../api/sessions";
import { Button, Card, SectionHeader } from "../../shared/ui";
import { formatEventTime } from "../events/eventDisplay";
import { SettingsControls, SettingsInspectorSection, useSettingsInspector } from "./SettingsInspector";

import { SettingsPage } from "./SettingsPage";
import { useSettingsScroll, useSettingsViewState } from "./settingsViewState";

type Kind = "browser" | "cli" | "mcp";
type ConnectionView = { selectedId?: string; visibleCount: number };
type Revocation = { path: string; label: string; signOut?: boolean };
const PAGE_SIZE = 10;
const TITLES = { browser: "Browser sessions", cli: "CLI connections", mcp: "MCP connections" };

export function AuthAccessManager({ kind = "browser", onSignOut, summary, navigation }: {
  kind?: Kind;
  onSignOut: () => void;
  summary?: ReactNode;
  navigation?: ReactNode;
}) {
  const client = useApiClient();
  const cache = useQueryClient();
  const inspector = useSettingsInspector();
  const [views, setViews] = useSettingsViewState<Record<Kind, ConnectionView>>("access-views", () => ({
    browser: { visibleCount: PAGE_SIZE }, cli: { visibleCount: PAGE_SIZE }, mcp: { visibleCount: PAGE_SIZE }
  }));
  const { selectedId, visibleCount } = views[kind];
  const updateView = (value: Partial<ConnectionView>) => setViews((current) => ({ ...current, [kind]: { ...current[kind], ...value } }));
  const scroll = useSettingsScroll<HTMLUListElement>(`account-list-${kind}`);
  const [confirmation, setConfirmation] = useState<Revocation | null>(null);
  const [notice, setNotice] = useState<string | null>(null);
  const confirmationRef = useRef<HTMLDivElement>(null);
  const noticeRef = useRef<HTMLParagraphElement>(null);
  const actionRef = useRef<HTMLElement | null>(null);
  const sessions = useQuery({ queryKey: queryKeys.browserSessions, queryFn: () => listBrowserSessions(client), enabled: kind === "browser" });
  const grants = useQuery({ queryKey: queryKeys.oauthGrants, queryFn: () => listOAuthGrants(client), enabled: kind !== "browser" });
  const revoke = useMutation({
    mutationFn: (action: Revocation) => client.delete(action.path),
    meta: { silentError: true },
    onSuccess: (_result, action) => {
      setConfirmation(null);
      if (action.signOut) {
        onSignOut();
        return;
      }
      setNotice("Access revoked.");
      void cache.invalidateQueries({ queryKey: queryKeys.browserSessions });
      void cache.invalidateQueries({ queryKey: queryKeys.oauthGrants });
      invalidateAuditEvents(cache);
    }
  });
  useEffect(() => {
    if (confirmation) confirmationRef.current?.focus();
  }, [confirmation, inspector?.container]);
  useEffect(() => {
    if (notice) noticeRef.current?.focus();
  }, [notice, inspector?.container]);

  const browserSessions = sessions.data?.sessions ?? [];
  const connections = (grants.data?.grants ?? []).filter((grant) => grant.kind === kind);
  const session = kind === "browser" ? browserSessions.find((item) => item.id === (selectedId ?? browserSessions[0]?.id)) : undefined;
  const grant = kind !== "browser" ? connections.find((item) => item.id === (selectedId ?? connections[0]?.id)) : undefined;
  const query = kind === "browser" ? sessions : grants;
  const rows = kind === "browser" ? browserSessions.map((item) => ({
    id: item.id, label: item.client_environment ?? "Environment not recorded", current: item.current,
    description: `Signed in ${formatEventTime(item.created_at)}`
  })) : connections.map((item) => ({
    id: item.id, label: item.client_name, current: false,
    description: `${item.client_environment ?? "Execution environment not recorded"} · Connected ${formatEventTime(item.created_at)}`
  }));
  const RowIcon = kind === "browser" ? Monitor : kind === "cli" ? Terminal : Unplug;
  const count = query.isSuccess ? <span className="text-xs text-muted">{rows.length} loaded</span> : undefined;
  const ask = (action: Revocation) => {
    actionRef.current = document.activeElement instanceof HTMLElement ? document.activeElement : null;
    revoke.reset(); setNotice(null); setConfirmation(action);
  };
  const select = (id: string) => {
    updateView({ selectedId: id }); setConfirmation(null); setNotice(null); revoke.reset(); inspector?.onOpen();
  };

  return (
    <SettingsPage title={kind === "browser" ? "Account" : TITLES[kind]} label={TITLES[kind]}
      inspectorLabel={kind === "browser" ? "Open session actions" : "Choose connection view"}
      disabled={revoke.isPending} summary={summary} scrollKey={`access-${kind}`} contained
      actions={kind === "browser" ? undefined : count}>
      {kind === "browser" ? <SectionHeader title="Browser sessions" actions={count} /> : null}
      {query.isLoading ? <p role="status" className="text-sm text-muted">Loading connections…</p> : query.isError ? (
        <div><p role="alert">Could not load {TITLES[kind].toLowerCase()}.</p><Button secondary onClick={() => { void query.refetch(); }}>Retry {kind === "browser" ? "sessions" : `${kind.toUpperCase()} connections`}</Button></div>
      ) : rows.length === 0 ? <p className="text-sm text-muted">{kind === "browser" ? "No active browser sessions." : `No active ${kind.toUpperCase()} connections.`}</p> : (
        <Card key={kind} ref={scroll} as="ul" aria-label={TITLES[kind]} padding="none" className="min-h-0 flex-1 divide-y divide-seam overflow-y-auto overscroll-contain">
          {rows.slice(0, visibleCount).map((row) => {
            const active = row.id === (session?.id ?? grant?.id);
            return <li key={row.id}>
              <button type="button" aria-label={`Select ${row.label}${row.current ? " (this session)" : ""}`} aria-pressed={active} disabled={revoke.isPending}
                onClick={() => select(row.id)} className={`flex min-h-[var(--ng-min-target-size)] w-full items-center gap-3 border-l-2 px-3 py-3 text-left disabled:cursor-not-allowed ${active ? "border-primary bg-[var(--ng-selection)]" : "border-transparent hover:bg-[var(--ng-hover)]"}`}>
                <RowIcon size={16} className="shrink-0 text-muted" />
                <span className="min-w-0 flex-1"><span className="flex items-center gap-2"><span className="truncate text-sm font-medium">{row.label}</span>{row.current ? <span className="shrink-0 text-xs text-primary">This session</span> : null}</span><span className="block truncate text-xs text-muted" title={row.description}>{row.description}</span></span>
                <ChevronRight size={15} className="shrink-0 text-muted" />
              </button>
            </li>;
          })}
        </Card>
      )}
      <div className="mt-3 flex shrink-0 flex-wrap items-center justify-between gap-2 text-xs text-muted">
        <span>{query.isSuccess ? `Showing ${Math.min(visibleCount, rows.length)} of ${rows.length} loaded` : ""}</span>
        {rows.length > visibleCount ? <Button secondary size="sm" disabled={revoke.isPending} onClick={() => updateView({ visibleCount: visibleCount + PAGE_SIZE })}>Show more ({Math.min(PAGE_SIZE, rows.length - visibleCount)})</Button> : null}
      </div>
      {navigation ? <SettingsControls slot="navigation"><fieldset disabled={revoke.isPending} className="min-w-0">{navigation}</fieldset></SettingsControls> : null}
      <SettingsControls>
        <SettingsInspectorSection title="Selected connection">
          <div role="region" aria-label="Selected connection">
            {notice ? <p ref={noticeRef} role="status" tabIndex={-1} className="mb-3 outline-none">{notice}</p> : null}
            {session ? <>
              <p className="break-words font-medium">{session.client_environment ?? "Environment not recorded"}{session.current ? " · This session" : ""}</p>
              <dl className="my-4 space-y-3 text-xs">
                <div><dt className="text-muted">Signed in</dt><dd>{formatEventTime(session.created_at)}</dd></div>
                <div><dt className="text-muted">Last used</dt><dd>{formatEventTime(session.last_used_at ?? session.created_at)}</dd></div>
                <div><dt className="text-muted">Expires</dt><dd>{formatEventTime(session.idle_expires_at < session.expires_at ? session.idle_expires_at : session.expires_at)}</dd></div>
              </dl>
              <Button size="sm" variant="danger" disabled={revoke.isPending} aria-label={`Sign out ${session.client_environment ?? "session"}${session.current ? " (this session)" : ""}`} onClick={() => ask({ path: `/api/v1/me/sessions/${session.id}`, label: `Sign out ${session.client_environment ?? "this session"}`, signOut: session.current })}>Sign out</Button>
              <p className="mt-3 text-xs text-muted">Environment labels are reported by the client; they do not identify a physical device.</p>
            </> : grant ? <>
              <p className="break-words font-medium">{grant.client_name} · {grant.kind.toUpperCase()}</p>
              <dl className="my-4 space-y-3 text-xs">
                <div><dt className="text-muted">Environment</dt><dd className="break-words">{grant.client_environment ?? "Execution environment not recorded"}</dd></div>
                <div><dt className="text-muted">Connected</dt><dd>{formatEventTime(grant.created_at)}</dd></div>
                <div><dt className="text-muted">Last token issued</dt><dd>{grant.last_token_at ? formatEventTime(grant.last_token_at) : "Not recorded"}</dd></div>
                <div><dt className="text-muted">Expires</dt><dd>{formatEventTime(grant.idle_expires_at < grant.expires_at ? grant.idle_expires_at : grant.expires_at)}</dd></div>
                <div><dt className="text-muted">Credentials</dt><dd>{grant.offline_access ? "Access & refresh tokens" : "Access token only"}</dd></div>
              </dl>
              <Button size="sm" variant="danger" disabled={revoke.isPending} aria-label={`Revoke ${grant.client_name}`} onClick={() => ask({ path: `/api/v1/me/oauth-grants/${grant.id}`, label: `Revoke ${grant.client_name}` })}>Revoke</Button>
            </> : <p className="text-xs text-muted">Select a connection to view its details.</p>}
          </div>
        </SettingsInspectorSection>
        {confirmation ? <div ref={confirmationRef} tabIndex={-1} role="group" aria-label="Confirm revocation" className="outline-none">
          <Card className="space-y-3 text-sm">
            <p>{confirmation.label}?</p>
            {revoke.isError ? <p role="alert">Could not revoke access. Try again.</p> : null}
            <div className="flex flex-wrap gap-2">
              <Button variant="danger" disabled={revoke.isPending} onClick={() => revoke.mutate(confirmation)}>Confirm</Button>
              <Button secondary disabled={revoke.isPending} onClick={() => { setConfirmation(null); requestAnimationFrame(() => actionRef.current?.focus()); }}>Cancel</Button>
            </div>
          </Card>
        </div> : null}
        <SettingsInspectorSection title={kind === "browser" ? "Browser actions" : `${kind.toUpperCase()} actions`}>
          <div className="flex flex-wrap gap-2">
            {kind === "browser" ? <>
              <Button size="sm" variant="danger" disabled={revoke.isPending} onClick={() => ask({ path: "/api/v1/me/sessions/others", label: "Sign out all other browser sessions" })}>Sign out other browsers</Button>
              <Button size="sm" variant="danger" disabled={revoke.isPending} onClick={() => ask({ path: "/api/v1/me/sessions", label: "Sign out all browsers, including this session", signOut: true })}>Sign out all browsers</Button>
            </> : <Button size="sm" variant="danger" disabled={revoke.isPending} onClick={() => ask({ path: `/api/v1/me/oauth-grants?kind=${kind}`, label: `Revoke all ${kind.toUpperCase()} connections` })}>Revoke all {kind.toUpperCase()} connections</Button>}
          </div>
          <p className="mt-3 text-xs text-muted">{kind === "browser" ? "Browser sign-out leaves CLI, MCP and Agent API keys connected." : `Revoking a ${kind.toUpperCase()} connection disables its access and refresh tokens.`}</p>
        </SettingsInspectorSection>
        <p className="text-xs text-muted">Agent API keys are managed in the Agents tab. Sign-in and revocation events appear in Audit history.</p>
      </SettingsControls>
    </SettingsPage>
  );
}
