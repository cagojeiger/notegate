import type { QueryKey } from "@tanstack/react-query";
import { Copy, KeyRound, Plus, Trash2 } from "lucide-react";
import { useCallback, useRef, useState } from "react";

import type { ApiKeyListResponse, MintedKey } from "../../api/keys";
import { copyText } from "../../shared/lib/clipboard";
import { AddButton, Button, Card, EmptyState, IconButton, SelectField, TextField } from "../../shared/ui";
import { useUiStore } from "../../stores/uiStore";
import { useApiKeysQuery, useCreateApiKeyMutation, useRevokeApiKeyMutation } from "./useSettingsQueries";

function expiryOptions(maxTtlDays: number) {
  const presets = maxTtlDays > 30 ? [7, 14, 30, 90, 180, maxTtlDays] : [7, 14, maxTtlDays];
  return Array.from(new Set(presets.filter((days) => days <= maxTtlDays))).map((days) => ({
    label: `${days} days`,
    days
  }));
}

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
  const [confirmId, setConfirmId] = useState<string | null>(null);
  const [adding, setAdding] = useState(false);
  const addButtonRef = useRef<HTMLButtonElement>(null);
  const returnToAdd = () => { requestAnimationFrame(() => addButtonRef.current?.focus()); };

  const createMutation = useCreateApiKeyMutation(queryKey, create);
  const created = createMutation.data;
  const focusCreatedToken = useCallback((element: HTMLDivElement | null) => {
    if (!document.activeElement?.closest('[role="tablist"]')) element?.querySelector("button")?.focus();
  }, []);
  const revokeMutation = useRevokeApiKeyMutation(queryKey, revoke, () => setConfirmId(null));

  const canCreate = name.trim().length > 0 && !createMutation.isPending && !created;
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
      {createMutation.isPending ? <p role="status" className="text-sm text-muted">Creating API key…</p> : null}
      {createMutation.isError ? <p role="alert" className="text-sm text-danger">{createMutation.error?.message || "Could not create API key. Try again."}</p> : null}
      {created ? (
        <Card tone="success">
          <div className="text-sm font-semibold text-text">Key “{created.name}” created</div>
          <p className="mt-1 text-xs text-muted">Copy it before selecting Done. You cannot retrieve it afterwards.</p>
          <div ref={focusCreatedToken} className="mt-3 flex items-center gap-2">
            <code className="min-w-0 flex-1 truncate rounded-workbench-surface border border-border bg-bg px-3 py-2 font-mono text-xs">{created.token}</code>
            <IconButton label="Copy token" onClick={() => { void copyCreatedToken(); }}><Copy size={15} /></IconButton>
          </div>
          <div className="mt-3 text-right"><Button secondary onClick={() => { createMutation.reset(); setName(""); setAdding(false); returnToAdd(); }}>Done</Button></div>
        </Card>
      ) : null}

      {keysQuery.isLoading ? (
        <div className="text-sm text-muted">Loading…</div>
      ) : keysQuery.isError ? (
        <Card tone="danger" padding="sm" className="flex flex-wrap items-center justify-between gap-2 text-workbench">
          <p role="alert" className="text-danger">Could not load API keys.</p>
          <Button size="sm" secondary aria-label="Retry API keys" disabled={keysQuery.isFetching} onClick={() => { void keysQuery.refetch(); }}>Retry</Button>
        </Card>
      ) : keys.length === 0 ? (
        <EmptyState>{emptyLabel}</EmptyState>
      ) : (
        <Card padding="none" as="ul" className="divide-y divide-seam">
          {keys.map((key) => (
            <li key={key.id} className="flex items-center gap-3 px-3 py-3 text-workbench">
              <KeyRound size={15} className="shrink-0 text-muted" />
              <div className="min-w-0 flex-1">
                <div className="truncate font-medium">{key.name}</div>
                <div className="text-xs text-muted">created {key.created_at.slice(0, 10)} · expires {key.expires_at.slice(0, 10)}</div>
              </div>
              {confirmId === key.id ? (
                <div className="flex shrink-0 items-center gap-2">
                  <Button size="sm" variant="danger" disabled={revokeMutation.isPending} onClick={() => revokeMutation.mutate(key.id)}>Confirm</Button>
                  <Button size="sm" secondary disabled={revokeMutation.isPending} onClick={() => setConfirmId(null)}>Cancel</Button>
                </div>
              ) : (
                <IconButton label={`Revoke ${key.name}`} disabled={revokeMutation.isPending} onClick={() => setConfirmId(key.id)}><Trash2 size={15} className="text-danger" /></IconButton>
              )}
            </li>
          ))}
        </Card>
      )}

      {adding && !created ? <form onSubmit={(event) => { event.preventDefault(); createKey(); }} className="grid grid-cols-1 items-end gap-2">
        <TextField
          autoFocus
          label="Name"
          className="min-w-0"
          value={name}
          onChange={(e) => setName(e.target.value)}
          placeholder="e.g. cli, ci, claude-test"
        />
        <SelectField label="Expires" value={days} onChange={(e) => setDays(Number(e.target.value))}>
          {options.map((option) => <option key={option.days} value={option.days}>{option.label}</option>)}
        </SelectField>
        <div className="flex gap-2">
          <Button type="submit" disabled={!canCreate}><Plus size={15} /> Create</Button>
          <Button secondary disabled={createMutation.isPending} onClick={() => { setAdding(false); setName(""); returnToAdd(); }}>Cancel</Button>
        </div>
      </form> : <AddButton label="Add API key" ref={addButtonRef} disabled={createMutation.isPending || !!created} onClick={() => setAdding(true)} />}
    </div>
  );
}
