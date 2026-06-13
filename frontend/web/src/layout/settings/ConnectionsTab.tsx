import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { Bot } from "lucide-react";

import { useApiClient } from "../../api/ApiProvider";
import { listAgents } from "../../api/agents";
import { connectAgent, disconnectAgent, listConnections, type Permission } from "../../api/connections";
import { queryKeys } from "../../api/queryKeys";
import type { Space } from "../../api/types";

export function ConnectionsTab({ activeSpace }: { activeSpace: Space | null }) {
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
