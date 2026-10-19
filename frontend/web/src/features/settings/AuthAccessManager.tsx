import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useState } from "react";

import { useApiClient } from "../../api/ApiProvider";
import { invalidateAuditEvents } from "../../api/queryInvalidation";
import { queryKeys } from "../../api/queryKeys";
import { listBrowserSessions, listOAuthGrants } from "../../api/sessions";
import { Button, Card, SectionHeader } from "../../shared/ui";
import { formatEventTime } from "../events/eventDisplay";

type Revocation = { path: string; label: string; signOut?: boolean };

export function AuthAccessManager({ onSignOut }: { onSignOut: () => void }) {
  const client = useApiClient();
  const cache = useQueryClient();
  const [confirmation, setConfirmation] = useState<Revocation | null>(null);
  const sessions = useQuery({ queryKey: queryKeys.browserSessions, queryFn: () => listBrowserSessions(client) });
  const grants = useQuery({ queryKey: queryKeys.oauthGrants, queryFn: () => listOAuthGrants(client) });
  const revoke = useMutation({
    mutationFn: (action: Revocation) => client.delete(action.path),
    meta: { silentError: true },
    onSuccess: (_result, action) => {
      setConfirmation(null);
      if (action.signOut) {
        onSignOut();
        return;
      }
      void cache.invalidateQueries({ queryKey: queryKeys.browserSessions });
      void cache.invalidateQueries({ queryKey: queryKeys.oauthGrants });
      invalidateAuditEvents(cache);
    }
  });
  const ask = (action: Revocation) => { revoke.reset(); setConfirmation(action); };

  return (
    <div className="space-y-4">
      {confirmation ? (
        <Card tone="danger" className="space-y-3 text-sm">
          <p>{confirmation.label}?</p>
          {revoke.isError ? <p role="alert">Could not revoke access. Try again.</p> : null}
          <div className="flex gap-2">
            <Button variant="danger" disabled={revoke.isPending} onClick={() => revoke.mutate(confirmation)}>Confirm</Button>
            <Button secondary disabled={revoke.isPending} onClick={() => setConfirmation(null)}>Cancel</Button>
          </div>
        </Card>
      ) : null}

      <section>
        <SectionHeader title="Browser sessions" description="Up to 100 active sessions, including this one. Environment labels are reported by the client; they do not identify a physical device." />
        {sessions.isLoading ? <p className="text-sm text-muted">Loading sessions…</p> : sessions.isError ? (
          <div><p role="alert">Could not load browser sessions.</p><Button secondary onClick={() => { void sessions.refetch(); }}>Retry sessions</Button></div>
        ) : (
          <Card as="ul" padding="none" className="divide-y divide-seam">
            {(sessions.data?.sessions ?? []).map((session) => (
              <li key={session.id} className="space-y-2 p-4 text-sm">
                <div className="flex flex-wrap items-center justify-between gap-2">
                  <span className="font-medium">{session.client_environment ?? "Environment not recorded"}{session.current ? " · This session" : ""}</span>
                  <Button size="sm" variant="danger" disabled={revoke.isPending} aria-label={`Sign out ${session.client_environment ?? "session"}${session.current ? " (this session)" : ""}`} onClick={() => ask({ path: `/api/v1/me/sessions/${session.id}`, label: `Sign out ${session.client_environment ?? "this session"}`, signOut: session.current })}>Sign out</Button>
                </div>
                <p className="text-xs text-muted">Signed in {formatEventTime(session.created_at)} · Last used {formatEventTime(session.last_used_at ?? session.created_at)}</p>
                <p className="text-xs text-muted">Expires {formatEventTime(session.idle_expires_at < session.expires_at ? session.idle_expires_at : session.expires_at)}</p>
              </li>
            ))}
          </Card>
        )}
        <div className="mt-3 flex flex-wrap gap-2">
          <Button size="sm" variant="danger" disabled={revoke.isPending} onClick={() => ask({ path: "/api/v1/me/sessions/others", label: "Sign out all other browser sessions" })}>Sign out other browsers</Button>
          <Button size="sm" variant="danger" disabled={revoke.isPending} onClick={() => ask({ path: "/api/v1/me/sessions", label: "Sign out all browsers, including this session", signOut: true })}>Sign out all browsers</Button>
        </div>
        <p className="mt-2 text-xs text-muted">Browser sign-out leaves CLI, MCP and Agent API keys connected.</p>
      </section>

      <section>
        <SectionHeader title="CLI & MCP connections" description="Revoking a connection disables its access and refresh tokens. Remote services may not report their execution environment." />
        {grants.isLoading ? <p className="text-sm text-muted">Loading connections…</p> : grants.isError ? (
          <div><p role="alert">Could not load application connections.</p><Button secondary onClick={() => { void grants.refetch(); }}>Retry connections</Button></div>
        ) : (grants.data?.grants ?? []).length === 0 ? <p className="text-sm text-muted">No active CLI or MCP connections.</p> : (
          <Card as="ul" padding="none" className="divide-y divide-seam">
            {(grants.data?.grants ?? []).map((grant) => (
              <li key={grant.id} className="space-y-2 p-4 text-sm">
                <div className="flex flex-wrap items-center justify-between gap-2">
                  <span className="font-medium">{grant.client_name} · {grant.kind.toUpperCase()}</span>
                  <Button size="sm" variant="danger" disabled={revoke.isPending} aria-label={`Revoke ${grant.client_name}`} onClick={() => ask({ path: `/api/v1/me/oauth-grants/${grant.id}`, label: `Revoke ${grant.client_name}` })}>Revoke</Button>
                </div>
                <p className="text-xs text-muted">{grant.client_environment ?? "Execution environment not recorded"}</p>
                <p className="text-xs text-muted">Connected {formatEventTime(grant.created_at)}{grant.last_token_at ? ` · Last token issued ${formatEventTime(grant.last_token_at)}` : ""}</p>
                <p className="text-xs text-muted">Expires {formatEventTime(grant.idle_expires_at < grant.expires_at ? grant.idle_expires_at : grant.expires_at)} · {grant.offline_access ? "Access & refresh tokens" : "Access token only"}</p>
              </li>
            ))}
          </Card>
        )}
        <Button size="sm" variant="danger" className="mt-3" disabled={revoke.isPending} onClick={() => ask({ path: "/api/v1/me/oauth-grants", label: "Revoke all CLI and MCP connections" })}>Revoke all CLI & MCP connections</Button>
        <p className="mt-2 text-xs text-muted">Agent API keys are managed individually in the Agents tab. Sign-in and revocation events appear in Audit history.</p>
      </section>
    </div>
  );
}
