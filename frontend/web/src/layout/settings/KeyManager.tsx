import type { QueryKey } from "@tanstack/react-query";
import { Copy, KeyRound, Plus, Trash2 } from "lucide-react";
import { useState } from "react";

import type { ApiKeyListResponse, MintedKey } from "../../api/keys";
import { copyText } from "../../shared/lib/clipboard";
import { Button, Card, EmptyState, IconButton, SelectField, TextField } from "../../shared/ui";
import { useUiStore } from "../../stores/uiStore";
import { useApiKeysQuery, useCreateApiKeyMutation, useRevokeApiKeyMutation } from "./useSettingsQueries";

function expiryOptions(maxTtlDays: number) {
  const presets = maxTtlDays > 30 ? [7, 14, 30, 90, 180, maxTtlDays] : [7, 14, maxTtlDays];
  return Array.from(new Set(presets.filter((days) => days <= maxTtlDays))).map((days) => ({
    label: `${days} days`,
    days
  }));
}

// Shared list/create/revoke UI for both user keys (/me/keys) and agent keys.
export function KeyManager({ queryKey, list, create, revoke, emptyLabel = "No active keys.", maxTtlDays = 30 }: {
  queryKey: QueryKey;
  list: () => Promise<ApiKeyListResponse>;
  create: (input: { name: string; expires_at: string }) => Promise<MintedKey>;
  revoke: (id: string) => Promise<void>;
  emptyLabel?: string;
  maxTtlDays?: number;
}) {
  const showToast = useUiStore((state) => state.showToast);
  const keysQuery = useApiKeysQuery(queryKey, list);
  const [name, setName] = useState("");
  const [days, setDays] = useState(Math.min(30, maxTtlDays));
  const [created, setCreated] = useState<MintedKey | null>(null);
  const [confirmId, setConfirmId] = useState<string | null>(null);

  const createMutation = useCreateApiKeyMutation(queryKey, create, (key) => {
    setCreated(key);
    setName("");
  });
  const revokeMutation = useRevokeApiKeyMutation(queryKey, revoke, () => setConfirmId(null));

  const canCreate = name.trim().length > 0 && !createMutation.isPending;
  const options = expiryOptions(maxTtlDays);

  function createInput() {
    // 5-min buffer keeps max-duration selections within the server limit despite clock skew.
    return { name: name.trim(), expires_at: new Date(Date.now() + days * 86_400_000 - 300_000).toISOString() };
  }

  function createKey() {
    if (!canCreate) return;
    createMutation.mutate(createInput());
  }

  async function copyCreatedToken() {
    if (!created) return;
    showToast((await copyText(created.token)) ? "Copied" : "Could not copy token");
  }

  const keys = keysQuery.data?.keys ?? [];
  return (
    <div className="space-y-4">
      {created ? (
        <Card tone="success">
          <div className="text-sm font-semibold text-text">Key “{created.name}” created</div>
          <p className="mt-1 text-xs text-muted">Copy it now — the token is shown only once.</p>
          <div className="mt-3 flex items-center gap-2">
            <code className="min-w-0 flex-1 truncate rounded-lg border border-border bg-bg px-3 py-2 font-mono text-xs">{created.token}</code>
            <IconButton label="Copy token" onClick={() => { void copyCreatedToken(); }}><Copy size={15} /></IconButton>
          </div>
          <div className="mt-3 text-right"><Button secondary onClick={() => setCreated(null)}>Done</Button></div>
        </Card>
      ) : null}

      <div className="flex flex-wrap items-end gap-2">
        <TextField
          label="Name"
          className="min-w-0 flex-1"
          value={name}
          onChange={(e) => setName(e.target.value)}
          onKeyDown={(e) => { if (e.key === "Enter") createKey(); }}
          placeholder="e.g. cli, ci, claude-test"
        />
        <SelectField label="Expires" value={days} onChange={(e) => setDays(Number(e.target.value))} className="w-36">
          {options.map((option) => <option key={option.days} value={option.days}>{option.label}</option>)}
        </SelectField>
        <Button onClick={createKey} disabled={!canCreate}><Plus size={15} /> Create</Button>
      </div>

      {keysQuery.isLoading ? (
        <div className="text-sm text-muted">Loading…</div>
      ) : keys.length === 0 ? (
        <EmptyState>{emptyLabel}</EmptyState>
      ) : (
        <Card padding="none" as="ul" className="divide-y divide-seam">
          {keys.map((key) => (
            <li key={key.id} className="flex items-center gap-3 px-4 py-3 text-sm">
              <KeyRound size={15} className="shrink-0 text-muted" />
              <div className="min-w-0 flex-1">
                <div className="truncate font-medium">{key.name}</div>
                <div className="text-xs text-muted">created {key.created_at.slice(0, 10)} · expires {key.expires_at.slice(0, 10)}</div>
              </div>
              {confirmId === key.id ? (
                <div className="flex shrink-0 items-center gap-1">
                  <Button size="sm" variant="danger" onClick={() => revokeMutation.mutate(key.id)}>Confirm</Button>
                  <Button size="sm" secondary onClick={() => setConfirmId(null)}>Cancel</Button>
                </div>
              ) : (
                <IconButton label={`Revoke ${key.name}`} onClick={() => setConfirmId(key.id)}><Trash2 size={15} /></IconButton>
              )}
            </li>
          ))}
        </Card>
      )}
    </div>
  );
}
