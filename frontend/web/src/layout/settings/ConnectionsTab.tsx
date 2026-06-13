import { Bot } from "lucide-react";

import type { Space } from "../../api/types";
import { Badge, Button, Card, EmptyState } from "../../shared/ui";
import { useAgentsQuery, useConnectAgentMutation, useConnectionsQuery, useDisconnectAgentMutation } from "./useSettingsQueries";

export function ConnectionsTab({ activeSpace }: { activeSpace: Space | null }) {
  const agentsQuery = useAgentsQuery();
  const spaceId = activeSpace?.id ?? "";
  const connQuery = useConnectionsQuery(spaceId);
  const connectMutation = useConnectAgentMutation(spaceId);
  const disconnectMutation = useDisconnectAgentMutation(spaceId);

  if (!activeSpace) return <EmptyState>Select a space to manage agent connections.</EmptyState>;

  const agents = agentsQuery.data?.agents ?? [];
  const connByAgent = new Map((connQuery.data?.connections ?? []).map((connection) => [connection.agent.id, connection.permission] as const));
  return (
    <div className="space-y-3">
      <p className="text-xs text-muted">Connect agents to <span className="font-medium text-text">{activeSpace.name}</span> and grant read or write.</p>
      {agentsQuery.isLoading ? (
        <div className="text-sm text-muted">Loading…</div>
      ) : agents.length === 0 ? (
        <EmptyState>Create an agent first (Agents tab).</EmptyState>
      ) : (
        <Card as="ul" padding="none" className="divide-y divide-seam">
          {agents.map((agent) => {
            const permission = connByAgent.get(agent.id);
            return (
              <li key={agent.id} className="flex items-center gap-3 px-4 py-3 text-sm">
                <Bot size={15} className="shrink-0 text-muted" />
                <span className="min-w-0 flex-1 truncate font-medium">{agent.name}</span>
                {permission ? (
                  <div className="flex shrink-0 items-center gap-2">
                    <Badge>{permission}</Badge>
                    <Button size="sm" secondary onClick={() => disconnectMutation.mutate(agent.id)}>Disconnect</Button>
                  </div>
                ) : (
                  <div className="flex shrink-0 items-center gap-1">
                    <Button size="sm" secondary onClick={() => connectMutation.mutate({ agentId: agent.id, permission: "read" })}>Read</Button>
                    <Button size="sm" secondary onClick={() => connectMutation.mutate({ agentId: agent.id, permission: "write" })}>Write</Button>
                  </div>
                )}
              </li>
            );
          })}
        </Card>
      )}
    </div>
  );
}
