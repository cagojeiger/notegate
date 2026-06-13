import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { Bot, ChevronRight, Plus, Trash2 } from "lucide-react";
import { useState } from "react";

import { useApiClient } from "../../api/ApiProvider";
import { createAgent, createAgentKey, deleteAgent, listAgentKeys, listAgents, revokeAgentKey } from "../../api/agents";
import { queryKeys } from "../../api/queryKeys";
import { Button, Card, EmptyState, IconButton, SectionHeader, TextField } from "../../shared/ui";
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
        <TextField
          label="New agent name"
          className="min-w-0 flex-1"
          value={name}
          onChange={(e) => setName(e.target.value)}
          onKeyDown={(e) => { if (e.key === "Enter" && name.trim()) createMutation.mutate(); }}
          placeholder="e.g. ci-bot, importer"
        />
        <Button onClick={() => createMutation.mutate()} disabled={!name.trim() || createMutation.isPending}><Plus size={15} /> Create</Button>
      </div>

      {agentsQuery.isLoading ? (
        <div className="text-sm text-muted">Loading…</div>
      ) : agents.length === 0 ? (
        <EmptyState>No agents yet.</EmptyState>
      ) : (
        <ul className="space-y-2">
          {agents.map((agent) => (
            <Card key={agent.id} as="li" padding="none">
              <div className="flex items-center gap-3 px-4 py-3 text-sm">
                <button type="button" onClick={() => setOpenId(openId === agent.id ? null : agent.id)} className="flex min-w-0 flex-1 items-center gap-2 text-left">
                  <ChevronRight size={14} className={`shrink-0 text-muted transition ${openId === agent.id ? "rotate-90" : ""}`} />
                  <Bot size={15} className="shrink-0 text-muted" />
                  <span className="truncate font-medium">{agent.name}</span>
                </button>
                {confirmId === agent.id ? (
                  <div className="flex shrink-0 items-center gap-1">
                    <Button size="sm" variant="danger" onClick={() => deleteMutation.mutate(agent.id)}>Confirm</Button>
                    <Button size="sm" secondary onClick={() => setConfirmId(null)}>Cancel</Button>
                  </div>
                ) : (
                  <IconButton label={`Delete ${agent.name}`} onClick={() => setConfirmId(agent.id)}><Trash2 size={15} /></IconButton>
                )}
              </div>
              {openId === agent.id ? (
                <div className="border-t border-seam p-4">
                  <SectionHeader title={`${agent.name} keys`} />
                  <KeyManager
                    queryKey={queryKeys.agentKeys(agent.id)}
                    list={() => listAgentKeys(client, agent.id)}
                    create={(input) => createAgentKey(client, agent.id, input)}
                    revoke={(keyId) => revokeAgentKey(client, agent.id, keyId)}
                    emptyLabel="No keys for this agent."
                  />
                </div>
              ) : null}
            </Card>
          ))}
        </ul>
      )}
    </div>
  );
}
