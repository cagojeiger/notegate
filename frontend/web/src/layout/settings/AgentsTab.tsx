import { Bot, ChevronRight, Plus, Trash2 } from "lucide-react";
import { useState } from "react";

import type { Permission } from "../../api/connections";
import type { Space } from "../../api/types";
import { canWriteSpace } from "../../auth/permissions";
import { Badge, Button, Card, EmptyState, IconButton, SectionHeader, TextField } from "../../shared/ui";
import {
  useAgentKeyManagerProps,
  useAgentsQuery,
  useConnectAgentToSpaceMutation,
  useConnectionsQuery,
  useCreateAgentMutation,
  useDeleteAgentMutation,
  useDisconnectAgentFromSpaceMutation,
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
                    <SectionHeader title="Space Access" description="Connect this agent to spaces." />
                    <AgentSpaceAccess agentId={agent.id} />
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

function AgentSpaceAccess({ agentId }: { agentId: string }) {
  const spacesQuery = useSettingsSpacesQuery(true);
  const spaces = (spacesQuery.data?.spaces ?? []).filter(canWriteSpace);

  if (spacesQuery.isLoading) return <div className="text-sm text-muted">Loading spaces…</div>;
  if (spaces.length === 0) return <EmptyState>No writable spaces.</EmptyState>;

  return (
    <Card as="ul" padding="none" className="max-h-72 divide-y divide-seam overflow-y-auto">
      {spaces.map((space) => <SpaceAccessRow key={space.id} space={space} agentId={agentId} />)}
    </Card>
  );
}

function SpaceAccessRow({ space, agentId }: { space: Space; agentId: string }) {
  const connectionsQuery = useConnectionsQuery(space.id);
  const connectMutation = useConnectAgentToSpaceMutation();
  const disconnectMutation = useDisconnectAgentFromSpaceMutation();
  const connection = connectionsQuery.data?.connections.find((item) => item.agent.id === agentId);
  const busy = connectMutation.isPending || disconnectMutation.isPending;

  const connect = (permission: Permission) => connectMutation.mutate({ spaceId: space.id, agentId, permission });

  return (
    <li className="flex items-center gap-3 px-4 py-3 text-sm">
      <div className="min-w-0 flex-1">
        <div className="truncate font-medium">{space.name}</div>
        <div className="text-xs text-muted">{connection ? "Connected" : "Not connected"}</div>
      </div>
      {connectionsQuery.isLoading ? (
        <span className="text-xs text-muted">Loading…</span>
      ) : connection ? (
        <div className="flex shrink-0 items-center gap-2">
          <Badge>{connection.permission}</Badge>
          <Button size="sm" secondary disabled={busy || connection.permission === "read"} onClick={() => connect("read")}>Read</Button>
          <Button size="sm" secondary disabled={busy || connection.permission === "write"} onClick={() => connect("write")}>Write</Button>
          <Button size="sm" secondary disabled={busy} onClick={() => disconnectMutation.mutate({ spaceId: space.id, agentId })}>Disconnect</Button>
        </div>
      ) : (
        <div className="flex shrink-0 items-center gap-1">
          <Button size="sm" secondary disabled={busy} onClick={() => connect("read")}>Connect read</Button>
          <Button size="sm" secondary disabled={busy} onClick={() => connect("write")}>Connect write</Button>
        </div>
      )}
    </li>
  );
}
