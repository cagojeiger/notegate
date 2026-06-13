import { useMutation, useQuery, useQueryClient, type QueryKey } from "@tanstack/react-query";
import { Copy, KeyRound, Plus, Trash2 } from "lucide-react";
import { useState } from "react";

import type { ApiKeyListResponse, MintedKey } from "../../api/keys";
import { useUiStore } from "../../stores/uiStore";
import { Button } from "../../shared/ui";

// Backend caps API-key lifetime to within 30 days.
const EXPIRY_OPTIONS = [
  { label: "7 days", days: 7 },
  { label: "14 days", days: 14 },
  { label: "30 days", days: 30 }
];

// Shared list/create/revoke UI for both user keys (/me/keys) and agent keys.
export function KeyManager({ queryKey, list, create, revoke, emptyLabel = "No active keys." }: {
  queryKey: QueryKey;
  list: () => Promise<ApiKeyListResponse>;
  create: (input: { name: string; expires_at: string }) => Promise<MintedKey>;
  revoke: (id: string) => Promise<void>;
  emptyLabel?: string;
}) {
  const queryClient = useQueryClient();
  const showToast = useUiStore((state) => state.showToast);
  const keysQuery = useQuery({ queryKey, queryFn: list });
  const [name, setName] = useState("");
  const [days, setDays] = useState(30);
  const [created, setCreated] = useState<MintedKey | null>(null);
  const [confirmId, setConfirmId] = useState<string | null>(null);

  const createMutation = useMutation({
    // 5-min buffer keeps the 30-day option within the server limit despite clock skew.
    mutationFn: () => create({ name: name.trim(), expires_at: new Date(Date.now() + days * 86_400_000 - 300_000).toISOString() }),
    onSuccess: (key) => {
      setCreated(key);
      setName("");
      void queryClient.invalidateQueries({ queryKey });
    }
  });
  const revokeMutation = useMutation({
    mutationFn: (id: string) => revoke(id),
    onSuccess: () => {
      setConfirmId(null);
      void queryClient.invalidateQueries({ queryKey });
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
            <button type="button" onClick={() => { void navigator.clipboard?.writeText(created.token); showToast("Copied"); }} className="grid size-9 shrink-0 place-items-center rounded-lg border border-border bg-surface text-muted hover:bg-panel hover:text-text" aria-label="Copy token"><Copy size={15} /></button>
          </div>
          <div className="mt-3 text-right"><Button secondary onClick={() => setCreated(null)}>Done</Button></div>
        </div>
      ) : null}

      <div className="flex flex-wrap items-end gap-2">
        <label className="min-w-0 flex-1 text-sm">
          <span className="mb-1.5 block text-xs text-muted">Name</span>
          <input value={name} onChange={(e) => setName(e.target.value)} onKeyDown={(e) => { if (e.key === "Enter" && name.trim()) createMutation.mutate(); }} placeholder="e.g. cli, ci, claude-test" className="w-full rounded-lg border border-border bg-surface px-3 py-2 text-text outline-none" />
        </label>
        <label className="text-sm">
          <span className="mb-1.5 block text-xs text-muted">Expires</span>
          <select value={days} onChange={(e) => setDays(Number(e.target.value))} className="rounded-lg border border-border bg-surface px-3 py-2 text-text outline-none">
            {EXPIRY_OPTIONS.map((o) => <option key={o.days} value={o.days}>{o.label}</option>)}
          </select>
        </label>
        <Button onClick={() => createMutation.mutate()} disabled={!name.trim() || createMutation.isPending}><Plus size={15} /> Create</Button>
      </div>

      {keysQuery.isLoading ? (
        <div className="text-sm text-muted">Loading…</div>
      ) : keys.length === 0 ? (
        <div className="rounded-xl border border-border bg-surface p-4 text-sm text-muted">{emptyLabel}</div>
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
    </div>
  );
}
