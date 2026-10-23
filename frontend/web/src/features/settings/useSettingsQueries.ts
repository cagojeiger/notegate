import { useMutation, useMutationState, useQuery, useQueryClient, type MutationState, type QueryKey } from "@tanstack/react-query";

import { useApiClient } from "../../api/ApiProvider";
import { createAgent, createAgentKey, deleteAgent, listAgentKeys, listAgents, revokeAgentKey, type Agent } from "../../api/agents";
import { connectAgent, disconnectAgent, listConnections, type Connection, type ConnectionListResponse, type Permission } from "../../api/connections";
import type { ApiKeyListResponse, MintedKey } from "../../api/keys";
import { invalidateAgentsList, invalidateAuditEvents } from "../../api/queryInvalidation";
import { queryKeys } from "../../api/queryKeys";
import { listSpaces } from "../../api/spaces";
import { useSettingsMutation } from "./useSettingsMutation";

export function useAgentsQuery(enabled = true) {
  const client = useApiClient();
  return useQuery({ queryKey: queryKeys.agents, queryFn: () => listAgents(client), enabled });
}

export function useSettingsSpacesQuery(enabled = true) {
  const client = useApiClient();
  return useQuery({ queryKey: queryKeys.spaces, queryFn: () => listSpaces(client), enabled });
}

export function useCreateAgentMutation(onCreated: (agent: Agent) => void) {
  const client = useApiClient();
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (name: string) => createAgent(client, name),
    onSuccess: (agent) => {
      onCreated(agent);
      invalidateAgentsList(queryClient);
      invalidateAuditEvents(queryClient);
    }
  });
}

export function useDeleteAgentMutation() {
  const client = useApiClient();
  const queryClient = useQueryClient();
  return useSettingsMutation("delete-agent", {
    meta: { silentError: true },
    mutationFn: (id: string) => deleteAgent(client, id),
    meta: { silentError: true },
    onSuccess: () => {
      invalidateAgentsList(queryClient);
      invalidateAuditEvents(queryClient);
    }
  });
}

export function useConnectionsQuery(spaceId: string) {
  const client = useApiClient();
  return useQuery({ queryKey: queryKeys.connections(spaceId), queryFn: () => listConnections(client, spaceId), enabled: !!spaceId });
}

export type AgentSpaceAccessValue = Permission | "none";

type AgentSpaceAccessInput = {
  access: AgentSpaceAccessValue;
  agent: Pick<Agent, "id" | "name">;
  spaceId: string;
};

export function useSetAgentSpaceAccessMutation() {
  const client = useApiClient();
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: async ({ access, agent, spaceId }: AgentSpaceAccessInput) => {
      if (access === "none") {
        await disconnectAgent(client, spaceId, agent.id);
        return null;
      }
      return connectAgent(client, spaceId, agent.id, access);
    },
    onMutate: async (variables) => {
      const queryKey = queryKeys.connections(variables.spaceId);
      await queryClient.cancelQueries({ queryKey });
      const previous = queryClient.getQueryData<ConnectionListResponse>(queryKey);
      queryClient.setQueryData<ConnectionListResponse>(queryKey, (current) => setAgentSpaceAccessInCache(current, variables));
      return { previous, queryKey };
    },
    onError: (_error, _variables, context) => {
      if (context?.previous) queryClient.setQueryData(context.queryKey, context.previous);
    },
    onSuccess: (connection, variables) => {
      if (!connection) return;
      queryClient.setQueryData<ConnectionListResponse>(queryKeys.connections(variables.spaceId), (current) => setAgentSpaceAccessInCache(current, variables, connection));
    },
    onSettled: (_connection, error, variables) => {
      void queryClient.invalidateQueries({ queryKey: queryKeys.connections(variables.spaceId) });
      if (!error) invalidateAuditEvents(queryClient);
    }
  });
}

function setAgentSpaceAccessInCache(
  previous: ConnectionListResponse | undefined,
  { access, agent }: AgentSpaceAccessInput,
  serverConnection?: Connection
): ConnectionListResponse | undefined {
  if (!previous) return previous;

  if (access === "none") {
    return {
      ...previous,
      connections: previous.connections.filter((item) => item.agent.id !== agent.id)
    };
  }

  const existingIndex = previous.connections.findIndex((item) => item.agent.id === agent.id);
  const nextConnection = serverConnection ?? {
    agent: existingIndex >= 0 ? previous.connections[existingIndex].agent : { id: agent.id, kind: "agent", display_name: agent.name },
    connected_at: existingIndex >= 0 ? previous.connections[existingIndex].connected_at : new Date().toISOString(),
    permission: access
  };

  if (existingIndex < 0) {
    return {
      ...previous,
      connections: [...previous.connections, nextConnection]
    };
  }

  const connections = [...previous.connections];
  connections[existingIndex] = nextConnection;
  return { ...previous, connections };
}

export function useApiKeysQuery(queryKey: QueryKey, list: () => Promise<ApiKeyListResponse>) {
  return useQuery({ queryKey, queryFn: list });
}

export function useCreateApiKeyMutation(queryKey: QueryKey, create: (input: { name: string; expires_at: string }) => Promise<MintedKey>) {
  const queryClient = useQueryClient();
  const cache = queryClient.getMutationCache();
  const filters = { mutationKey: ["api-key-issuance", queryKey], exact: true };
  const mutation = useMutation({
    mutationKey: filters.mutationKey,
    // The one-time token stays in this authenticated cache until explicitly acknowledged.
    gcTime: Infinity,
    meta: { silentError: true },
    mutationFn: create,
    onSuccess: () => {
      void queryClient.invalidateQueries({ queryKey });
      invalidateAuditEvents(queryClient);
    }
  });
  const states = useMutationState<MutationState<MintedKey, Error, { name: string; expires_at: string }>>({
    filters, select: (item) => item.state
  });
  const state = states[states.length - 1];
  const retained = () => cache.findAll(filters).some((item) => item.state.status === "pending" || item.state.status === "success");
  const clear = () => cache.findAll(filters).forEach((item) => cache.remove(item));

  return {
    isPending: state?.status === "pending",
    isError: state?.status === "error",
    error: state?.error,
    data: state?.data,
    mutate: (input: { name: string; expires_at: string }) => {
      // Include calls from a remounted view or a stale event handler.
      if (retained()) return;
      clear();
      mutation.mutate(input);
    },
    reset: () => {
      if (cache.find({ ...filters, status: "pending" })) return;
      mutation.reset();
      clear();
    }
  };
}

export function useRevokeApiKeyMutation(queryKey: QueryKey, revoke: (id: string) => Promise<void>, onRevoked: () => void) {
  const queryClient = useQueryClient();
  return useSettingsMutation("revoke-api-key", {
    meta: { silentError: true },
    mutationFn: revoke,
    meta: { silentError: true },
    onSuccess: () => {
      onRevoked();
      void queryClient.invalidateQueries({ queryKey });
      invalidateAuditEvents(queryClient);
    }
  }, queryKey);
}

export function useAgentKeyManagerProps(agentId: string) {
  const client = useApiClient();
  return {
    queryKey: queryKeys.agentKeys(agentId),
    list: () => listAgentKeys(client, agentId),
    create: (input: { name: string; expires_at: string }) => createAgentKey(client, agentId, input),
    revoke: (keyId: string) => revokeAgentKey(client, agentId, keyId)
  };
}
