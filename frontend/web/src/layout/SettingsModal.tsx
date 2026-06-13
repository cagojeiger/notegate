import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { Bot, ChevronRight, Moon, Plus, Sun, Trash2 } from "lucide-react";
import { useState } from "react";

import { useApiClient } from "../api/ApiProvider";
import { createAgent, deleteAgent, listAgentKeys, listAgents, createAgentKey, revokeAgentKey } from "../api/agents";
import { connectAgent, disconnectAgent, listConnections, type Permission } from "../api/connections";
import { createMyKey, listMyKeys, revokeMyKey } from "../api/keys";
import { getMe } from "../api/me";
import { queryKeys } from "../api/queryKeys";
import type { Space } from "../api/types";
import { useUiStore } from "../stores/uiStore";
import { Button, Modal } from "../shared/ui";
import { KeyManager } from "./settings/KeyManager";

type Tab = "account" | "keys" | "agents" | "connections";

const TABS: { id: Tab; label: string }[] = [
  { id: "account", label: "Account" },
  { id: "keys", label: "API Keys" },
  { id: "agents", label: "Agents" },
  { id: "connections", label: "Connections" }
];

export function SettingsModal({ onClose, onSignOut, activeSpace }: { onClose: () => void; onSignOut: () => void; activeSpace: Space | null }) {
  const [tab, setTab] = useState<Tab>("account");
  const client = useApiClient();
  return (
    <Modal title="Settings" onClose={onClose} width="max-w-2xl">
      <div role="tablist" className="mb-5 flex gap-1 border-b border-seam">
        {TABS.map((t) => (
          <button key={t.id} role="tab" aria-selected={tab === t.id} onClick={() => setTab(t.id)} className={`-mb-px border-b-2 px-3 py-2 text-sm font-medium transition ${tab === t.id ? "border-primary text-text" : "border-transparent text-muted hover:text-text"}`}>{t.label}</button>
        ))}
      </div>
      {tab === "account" ? <AccountTab onSignOut={onSignOut} /> : null}
      {tab === "keys" ? (
        <KeyManager queryKey={queryKeys.myKeys} list={() => listMyKeys(client)} create={(input) => createMyKey(client, input)} revoke={(id) => revokeMyKey(client, id)} emptyLabel="No active keys." />
      ) : null}
      {tab === "agents" ? <AgentsTab /> : null}
      {tab === "connections" ? <ConnectionsTab activeSpace={activeSpace} /> : null}
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

function AgentsTab() {
  const client = useApiClient();
  const queryClient = useQueryClient();
  const agentsQuery = useQuery({ queryKey: queryKeys.agents, queryFn: () => listAgents(client) });
  const [name, setName] = useState("");
  const [openId, setOpenId] = useState<string | null>(null);
  const [confirmId, setConfirmId] = useState<string | null>(null);

  const createMutation = useMutation({
    mutationFn: () => createAgent(client, name.trim()),
    onSuccess: () => { setName(""); void queryClient.invalidateQueries({ queryKey: queryKeys.agents }); }
  });
  const deleteMutation = useMutation({
    mutationFn: (id: string) => deleteAgent(client, id),
    onSuccess: () => { setConfirmId(null); void queryClient.invalidateQueries({ queryKey: queryKeys.agents }); }
  });

  const agents = agentsQuery.data?.agents ?? [];
  return (
    <div className="space-y-4">
      <div className="flex items-end gap-2">
        <label className="min-w-0 flex-1 text-sm">
          <span className="mb-1.5 block text-xs text-muted">New agent name</span>
          <input value={name} onChange={(e) => setName(e.target.value)} onKeyDown={(e) => { if (e.key === "Enter" && name.trim()) createMutation.mutate(); }} placeholder="e.g. ci-bot, importer" className="w-full rounded-lg border border-border bg-surface px-3 py-2 text-text outline-none" />
        </label>
        <Button onClick={() => createMutation.mutate()} disabled={!name.trim() || createMutation.isPending}><Plus size={15} /> Create</Button>
      </div>

      {agentsQuery.isLoading ? (
        <div className="text-sm text-muted">Loading…</div>
      ) : agents.length === 0 ? (
        <div className="rounded-xl border border-border bg-surface p-4 text-sm text-muted">No agents yet.</div>
      ) : (
        <ul className="space-y-2">
          {agents.map((agent) => (
            <li key={agent.id} className="rounded-xl border border-border bg-surface">
              <div className="flex items-center gap-3 px-4 py-3 text-sm">
                <button type="button" onClick={() => setOpenId(openId === agent.id ? null : agent.id)} className="flex min-w-0 flex-1 items-center gap-2 text-left">
                  <ChevronRight size={14} className={`shrink-0 text-muted transition ${openId === agent.id ? "rotate-90" : ""}`} />
                  <Bot size={15} className="shrink-0 text-muted" />
                  <span className="truncate font-medium">{agent.name}</span>
                </button>
                {confirmId === agent.id ? (
                  <div className="flex shrink-0 items-center gap-1">
                    <button type="button" onClick={() => deleteMutation.mutate(agent.id)} className="rounded-lg bg-danger px-2.5 py-1 text-xs font-semibold text-primary-contrast hover:opacity-90">Confirm</button>
                    <button type="button" onClick={() => setConfirmId(null)} className="rounded-lg border border-border px-2.5 py-1 text-xs text-muted hover:text-text">Cancel</button>
                  </div>
                ) : (
                  <button type="button" onClick={() => setConfirmId(agent.id)} aria-label={`Delete ${agent.name}`} className="grid size-8 shrink-0 place-items-center rounded-lg text-muted hover:bg-danger/10 hover:text-danger"><Trash2 size={15} /></button>
                )}
              </div>
              {openId === agent.id ? (
                <div className="border-t border-seam p-4">
                  <h4 className="mb-3 text-xs font-bold uppercase tracking-wide text-muted">{agent.name} keys</h4>
                  <KeyManager
                    queryKey={queryKeys.agentKeys(agent.id)}
                    list={() => listAgentKeys(client, agent.id)}
                    create={(input) => createAgentKey(client, agent.id, input)}
                    revoke={(keyId) => revokeAgentKey(client, agent.id, keyId)}
                    emptyLabel="No keys for this agent."
                  />
                </div>
              ) : null}
            </li>
          ))}
        </ul>
      )}
    </div>
  );
}

function ConnectionsTab({ activeSpace }: { activeSpace: Space | null }) {
  const client = useApiClient();
  const queryClient = useQueryClient();
  const agentsQuery = useQuery({ queryKey: queryKeys.agents, queryFn: () => listAgents(client) });
  const spaceId = activeSpace?.id ?? "";
  const connQuery = useQuery({ queryKey: queryKeys.connections(spaceId), queryFn: () => listConnections(client, spaceId), enabled: !!spaceId });

  const connectMutation = useMutation({
    mutationFn: ({ agentId, permission }: { agentId: string; permission: Permission }) => connectAgent(client, spaceId, agentId, permission),
    onSuccess: () => void queryClient.invalidateQueries({ queryKey: queryKeys.connections(spaceId) })
  });
  const disconnectMutation = useMutation({
    mutationFn: (agentId: string) => disconnectAgent(client, spaceId, agentId),
    onSuccess: () => void queryClient.invalidateQueries({ queryKey: queryKeys.connections(spaceId) })
  });

  if (!activeSpace) return <div className="rounded-xl border border-border bg-surface p-4 text-sm text-muted">Select a space to manage agent connections.</div>;

  const agents = agentsQuery.data?.agents ?? [];
  const connByAgent = new Map((connQuery.data?.connections ?? []).map((c) => [c.agent.id, c.permission] as const));
  return (
    <div className="space-y-3">
      <p className="text-xs text-muted">Connect agents to <span className="font-medium text-text">{activeSpace.name}</span> and grant read or write.</p>
      {agentsQuery.isLoading ? (
        <div className="text-sm text-muted">Loading…</div>
      ) : agents.length === 0 ? (
        <div className="rounded-xl border border-border bg-surface p-4 text-sm text-muted">Create an agent first (Agents tab).</div>
      ) : (
        <ul className="divide-y divide-seam rounded-xl border border-border bg-surface">
          {agents.map((agent) => {
            const permission = connByAgent.get(agent.id);
            return (
              <li key={agent.id} className="flex items-center gap-3 px-4 py-3 text-sm">
                <Bot size={15} className="shrink-0 text-muted" />
                <span className="min-w-0 flex-1 truncate font-medium">{agent.name}</span>
                {permission ? (
                  <div className="flex shrink-0 items-center gap-2">
                    <span className="rounded-full border border-border px-2 py-0.5 text-xs capitalize text-muted">{permission}</span>
                    <button type="button" onClick={() => disconnectMutation.mutate(agent.id)} className="rounded-lg border border-border px-2.5 py-1 text-xs text-muted hover:text-text">Disconnect</button>
                  </div>
                ) : (
                  <div className="flex shrink-0 items-center gap-1">
                    <button type="button" onClick={() => connectMutation.mutate({ agentId: agent.id, permission: "read" })} className="rounded-lg border border-border px-2.5 py-1 text-xs text-muted hover:text-text">Read</button>
                    <button type="button" onClick={() => connectMutation.mutate({ agentId: agent.id, permission: "write" })} className="rounded-lg border border-border px-2.5 py-1 text-xs text-muted hover:text-text">Write</button>
                  </div>
                )}
              </li>
            );
          })}
        </ul>
      )}
    </div>
  );
}
