import { Bot, ChevronRight, Plus, Trash2 } from "lucide-react";
import { useState } from "react";

import { Button, Card, EmptyState, IconButton, SectionHeader, TextField } from "../../shared/ui";
import { KeyManager } from "./KeyManager";
import { useAgentKeyManagerProps, useAgentsQuery, useCreateAgentMutation, useDeleteAgentMutation } from "./useSettingsQueries";

export function AgentsTab() {
  const agentsQuery = useAgentsQuery();
  const [name, setName] = useState("");
  const [openId, setOpenId] = useState<string | null>(null);
  const [confirmId, setConfirmId] = useState<string | null>(null);

  const createMutation = useCreateAgentMutation(() => setName(""));
  const deleteMutation = useDeleteAgentMutation(() => setConfirmId(null));

  const agents = agentsQuery.data?.agents ?? [];
  return (
    <div className="space-y-4">
      <div className="flex items-end gap-2">
        <TextField
          label="New agent name"
          className="min-w-0 flex-1"
          value={name}
          onChange={(e) => setName(e.target.value)}
          onKeyDown={(e) => { if (e.key === "Enter" && name.trim()) createMutation.mutate(name.trim()); }}
          placeholder="e.g. ci-bot, importer"
        />
        <Button onClick={() => createMutation.mutate(name.trim())} disabled={!name.trim() || createMutation.isPending}><Plus size={15} /> Create</Button>
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
                  <AgentKeyManager agentId={agent.id} />
                </div>
              ) : null}
            </Card>
          ))}
        </ul>
      )}
    </div>
  );
}

function AgentKeyManager({ agentId }: { agentId: string }) {
  const keyManagerProps = useAgentKeyManagerProps(agentId);
  return <KeyManager {...keyManagerProps} emptyLabel="No keys for this agent." />;
}
