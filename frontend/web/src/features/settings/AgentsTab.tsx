import { Bot, ChevronRight, Plus, Trash2 } from "lucide-react";
import { useState } from "react";

import type { Agent } from "../../api/agents";
import type { Space } from "../../entities/space/model";
import { canWriteSpace } from "../../auth/permissions";
import { Button, Card, EmptyState, IconButton, SectionHeader, TextField } from "../../shared/ui";
import {
  type AgentSpaceAccessValue,
  useAgentKeyManagerProps,
  useAgentsQuery,
  useConnectionsQuery,
  useCreateAgentMutation,
  useDeleteAgentMutation,
  useSetAgentSpaceAccessMutation,
  useSettingsSpacesQuery
} from "./useSettingsQueries";
import { KeyManager } from "./KeyManager";

export function AgentsTab({ canManageAgents }: { canManageAgents: boolean }) {
  const agentsQuery = useAgentsQuery(canManageAgents);
  const [name, setName] = useState("");
  const [openId, setOpenId] = useState<string | null>(null);
  const [confirmId, setConfirmId] = useState<string | null>(null);

  const createMutation = useCreateAgentMutation(() => setName(""));
  const deleteMutation = useDeleteAgentMutation(() => setConfirmId(null));

  function confirmDelete(agentId: string) {
    if (openId === agentId) setOpenId(null);
    deleteMutation.mutate(agentId);
  }

  const agents = agentsQuery.data?.agents ?? [];
  if (!canManageAgents) return <EmptyState>Agent management is unavailable for this account.</EmptyState>;

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
                <button type="button" aria-expanded={openId === agent.id} aria-label={`Toggle ${agent.name} details`} onClick={() => setOpenId(openId === agent.id ? null : agent.id)} className="flex min-w-0 flex-1 items-center gap-2 rounded-lg px-2 py-1.5 text-left transition hover:bg-[var(--ng-hover)] hover:text-text">
                  <ChevronRight size={14} className={`shrink-0 text-muted transition ${openId === agent.id ? "rotate-90" : ""}`} />
                  <Bot size={15} className="shrink-0 text-muted" />
                  <span className="truncate font-medium">{agent.name}</span>
                </button>
                {confirmId === agent.id ? (
                  <div className="flex shrink-0 items-center gap-1">
                    <Button size="sm" variant="danger" onClick={() => confirmDelete(agent.id)}>Confirm</Button>
                    <Button size="sm" secondary onClick={() => setConfirmId(null)}>Cancel</Button>
                  </div>
                ) : (
                  <IconButton label={`Delete ${agent.name}`} onClick={() => setConfirmId(agent.id)}><Trash2 size={15} /></IconButton>
                )}
              </div>
              {openId === agent.id ? (
                <div className="space-y-4 border-t border-seam p-4">
                  <section>
                    <SectionHeader title="Agent API Keys" description="Keys authenticate as this agent." />
                    <AgentKeyManager agentId={agent.id} />
                  </section>
                  <section>
                    <SectionHeader title="Space permissions" description="Choose which spaces this agent can access." />
                    <AgentSpaceAccess agent={agent} />
                  </section>
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
  return <KeyManager {...keyManagerProps} emptyLabel="No keys for this agent." maxTtlDays={365} />;
}

function AgentSpaceAccess({ agent }: { agent: Agent }) {
  const spacesQuery = useSettingsSpacesQuery(true);
  const spaces = (spacesQuery.data?.spaces ?? []).filter(canWriteSpace);

  if (spacesQuery.isLoading) return <div className="text-sm text-muted">Loading spaces…</div>;
  if (spaces.length === 0) return <EmptyState>No writable spaces.</EmptyState>;

  return (
    <ul className="max-h-72 divide-y divide-seam overflow-y-auto rounded-xl border border-border bg-surface">
      {spaces.map((space) => <SpaceAccessRow key={space.id} space={space} agent={agent} />)}
    </ul>
  );
}

const ACCESS_LABELS: Record<AgentSpaceAccessValue, string> = {
  none: "No access",
  read: "Read only",
  write: "Read & write"
};

function SpaceAccessRow({ space, agent }: { space: Space; agent: Agent }) {
  const connectionsQuery = useConnectionsQuery(space.id);
  const setAccessMutation = useSetAgentSpaceAccessMutation();
  const connection = connectionsQuery.data?.connections.find((item) => item.agent.id === agent.id);
  const access: AgentSpaceAccessValue = connection?.permission ?? "none";

  const updateAccess = (nextAccess: AgentSpaceAccessValue) => {
    if (nextAccess === access) return;
    if (nextAccess === "none" && !connection) return;
    setAccessMutation.mutate({ spaceId: space.id, agent, access: nextAccess });
  };

  return (
    <li className="flex items-center gap-3 px-4 py-3 text-sm max-sm:flex-col max-sm:items-stretch">
      <div className="min-w-0 flex-1">
        <div className="truncate font-medium">{space.name}</div>
        <div className="text-xs text-muted">{ACCESS_LABELS[access]}</div>
      </div>
      {connectionsQuery.isLoading ? (
        <span className="text-xs text-muted">Loading…</span>
      ) : (
        <select
          aria-label={`${space.name} permission`}
          className="h-9 w-36 shrink-0 rounded-lg border border-border-strong bg-surface px-3 text-sm text-text outline-none transition disabled:cursor-not-allowed disabled:opacity-50 max-sm:w-full"
          value={access}
          disabled={setAccessMutation.isPending}
          onChange={(event) => updateAccess(event.currentTarget.value as AgentSpaceAccessValue)}
        >
          <option value="none">{ACCESS_LABELS.none}</option>
          <option value="read">{ACCESS_LABELS.read}</option>
          <option value="write">{ACCESS_LABELS.write}</option>
        </select>
      )}
    </li>
  );
}
