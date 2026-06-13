import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { Bot, ChevronRight, Plus, Trash2 } from "lucide-react";
import { useState } from "react";

import { useApiClient } from "../../api/ApiProvider";
import { createAgent, createAgentKey, deleteAgent, listAgentKeys, listAgents, revokeAgentKey } from "../../api/agents";
import { queryKeys } from "../../api/queryKeys";
import { Button } from "../../shared/ui";
import { KeyManager } from "./KeyManager";

export function AgentsTab() {
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
