import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { Copy, KeyRound, Moon, Plus, Sun, Trash2 } from "lucide-react";
import { useState } from "react";

import { useApiClient } from "../api/ApiProvider";
import { createMyKey, listMyKeys, revokeMyKey, type CreatedApiKey } from "../api/keys";
import { getMe } from "../api/me";
import { queryKeys } from "../api/queryKeys";
import { useUiStore } from "../stores/uiStore";
import { Button, Modal } from "../shared/ui";

type Tab = "account" | "keys";

const TABS: { id: Tab; label: string }[] = [
  { id: "account", label: "Account" },
  { id: "keys", label: "API Keys" }
];

export function SettingsModal({ onClose, onSignOut }: { onClose: () => void; onSignOut: () => void }) {
  const [tab, setTab] = useState<Tab>("account");
  return (
    <Modal title="Settings" onClose={onClose} width="max-w-2xl">
      <div role="tablist" className="mb-5 flex gap-1 border-b border-seam">
        {TABS.map((t) => (
          <button
            key={t.id}
            role="tab"
            aria-selected={tab === t.id}
            onClick={() => setTab(t.id)}
            className={`-mb-px border-b-2 px-3 py-2 text-sm font-medium transition ${tab === t.id ? "border-primary text-text" : "border-transparent text-muted hover:text-text"}`}
          >
            {t.label}
          </button>
        ))}
      </div>
      {tab === "account" ? <AccountTab onSignOut={onSignOut} /> : <ApiKeysTab />}
    </Modal>
  );
}

function AccountTab({ onSignOut }: { onSignOut: () => void }) {
  const client = useApiClient();
  const meQuery = useQuery({ queryKey: queryKeys.me, queryFn: () => getMe(client) });
  const theme = useUiStore((state) => state.theme);
  const toggleTheme = useUiStore((state) => state.toggleTheme);
  const me = meQuery.data;
  return (
    <div className="space-y-4">
      <section>
        <h3 className="mb-2 text-xs font-bold uppercase tracking-wide text-muted">Account</h3>
        <div className="rounded-xl border border-border bg-surface p-4 text-sm">
          <div className="font-medium">{me?.account.display_name ?? "…"}</div>
          <div className="text-muted">{me?.user?.email ?? me?.account.kind ?? ""}</div>
        </div>
      </section>
      <section>
        <h3 className="mb-2 text-xs font-bold uppercase tracking-wide text-muted">Appearance</h3>
        <button type="button" onClick={toggleTheme} className="flex w-full items-center justify-between rounded-xl border border-border bg-surface p-4 text-sm hover:bg-panel">
          <span>Theme</span>
          <span className="flex items-center gap-2 capitalize text-muted">{theme === "light" ? <Sun size={16} /> : <Moon size={16} />} {theme}</span>
        </button>
      </section>
      <button type="button" onClick={onSignOut} className="w-full rounded-lg border border-danger/40 px-4 py-2 text-sm font-semibold text-danger transition hover:bg-danger/10">Sign out</button>
    </div>
  );
}

// Backend caps API-key lifetime to within 30 days.
const EXPIRY_OPTIONS = [
  { label: "7 days", days: 7 },
  { label: "14 days", days: 14 },
  { label: "30 days", days: 30 }
];

function ApiKeysTab() {
  const client = useApiClient();
  const queryClient = useQueryClient();
  const showToast = useUiStore((state) => state.showToast);
  const keysQuery = useQuery({ queryKey: queryKeys.myKeys, queryFn: () => listMyKeys(client) });
  const [name, setName] = useState("");
  const [days, setDays] = useState(30);
  const [created, setCreated] = useState<CreatedApiKey | null>(null);
  const [confirmId, setConfirmId] = useState<string | null>(null);

  const createMutation = useMutation({
    // Subtract a 5-min buffer so the 30-day option stays within the server's
    // "within 30 days" limit despite client/server clock skew.
    mutationFn: () => createMyKey(client, { name: name.trim(), expires_at: new Date(Date.now() + days * 86_400_000 - 300_000).toISOString() }),
    onSuccess: (key) => {
      setCreated(key);
      setName("");
      void queryClient.invalidateQueries({ queryKey: queryKeys.myKeys });
    }
  });
  const revokeMutation = useMutation({
    mutationFn: (id: string) => revokeMyKey(client, id),
    onSuccess: () => {
      setConfirmId(null);
      void queryClient.invalidateQueries({ queryKey: queryKeys.myKeys });
    }
  });

  const keys = keysQuery.data?.keys ?? [];
  return (
    <div className="space-y-4">
      {created ? (
        <div className="rounded-xl border border-success/40 bg-success/10 p-4">
          <div className="text-sm font-semibold text-text">Key “{created.name}” created</div>
          <p className="mt-1 text-xs text-muted">Copy it now — the token is shown only once.</p>
          <div className="mt-3 flex items-center gap-2">
            <code className="min-w-0 flex-1 truncate rounded-lg border border-border bg-bg px-3 py-2 font-mono text-xs">{created.token}</code>
            <button
              type="button"
              onClick={() => { void navigator.clipboard?.writeText(created.token); showToast("Copied"); }}
              className="grid size-9 shrink-0 place-items-center rounded-lg border border-border bg-surface text-muted hover:bg-panel hover:text-text"
              aria-label="Copy token"
            ><Copy size={15} /></button>
          </div>
          <div className="mt-3 text-right"><Button secondary onClick={() => setCreated(null)}>Done</Button></div>
        </div>
      ) : null}

      <section>
        <h3 className="mb-2 text-xs font-bold uppercase tracking-wide text-muted">Create key</h3>
        <div className="flex flex-wrap items-end gap-2">
          <label className="min-w-0 flex-1 text-sm">
            <span className="mb-1.5 block text-xs text-muted">Name</span>
            <input
              value={name}
              onChange={(e) => setName(e.target.value)}
              onKeyDown={(e) => { if (e.key === "Enter" && name.trim()) createMutation.mutate(); }}
              placeholder="e.g. cli, ci, claude-test"
              className="w-full rounded-lg border border-border bg-surface px-3 py-2 text-text outline-none"
            />
          </label>
          <label className="text-sm">
            <span className="mb-1.5 block text-xs text-muted">Expires</span>
            <select value={days} onChange={(e) => setDays(Number(e.target.value))} className="rounded-lg border border-border bg-surface px-3 py-2 text-text outline-none">
              {EXPIRY_OPTIONS.map((o) => <option key={o.days} value={o.days}>{o.label}</option>)}
            </select>
          </label>
          <Button onClick={() => createMutation.mutate()} disabled={!name.trim() || createMutation.isPending}><Plus size={15} /> Create</Button>
        </div>
      </section>

      <section>
        <h3 className="mb-2 text-xs font-bold uppercase tracking-wide text-muted">Active keys</h3>
        {keysQuery.isLoading ? (
          <div className="text-sm text-muted">Loading…</div>
        ) : keys.length === 0 ? (
          <div className="rounded-xl border border-border bg-surface p-4 text-sm text-muted">No active keys.</div>
        ) : (
          <ul className="divide-y divide-seam rounded-xl border border-border bg-surface">
            {keys.map((key) => (
              <li key={key.id} className="flex items-center gap-3 px-4 py-3 text-sm">
                <KeyRound size={15} className="shrink-0 text-muted" />
                <div className="min-w-0 flex-1">
                  <div className="truncate font-medium">{key.name}</div>
                  <div className="text-xs text-muted">created {key.created_at.slice(0, 10)} · expires {key.expires_at.slice(0, 10)}</div>
                </div>
                {confirmId === key.id ? (
                  <div className="flex shrink-0 items-center gap-1">
                    <button type="button" onClick={() => revokeMutation.mutate(key.id)} className="rounded-lg bg-danger px-2.5 py-1 text-xs font-semibold text-primary-contrast hover:opacity-90">Confirm</button>
                    <button type="button" onClick={() => setConfirmId(null)} className="rounded-lg border border-border px-2.5 py-1 text-xs text-muted hover:text-text">Cancel</button>
                  </div>
                ) : (
                  <button type="button" onClick={() => setConfirmId(key.id)} aria-label={`Revoke ${key.name}`} className="grid size-8 shrink-0 place-items-center rounded-lg text-muted hover:bg-danger/10 hover:text-danger"><Trash2 size={15} /></button>
                )}
              </li>
            ))}
          </ul>
        )}
      </section>
    </div>
  );
}
