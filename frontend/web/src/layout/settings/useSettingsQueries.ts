import { useMutation, useQuery, useQueryClient, type QueryKey } from "@tanstack/react-query";

import { useApiClient } from "../../api/ApiProvider";
import { createAgent, createAgentKey, deleteAgent, listAgentKeys, listAgents, revokeAgentKey } from "../../api/agents";
import { connectAgent, disconnectAgent, listConnections, type Permission } from "../../api/connections";
import { createMyKey, listMyKeys, revokeMyKey, type ApiKeyListResponse, type MintedKey } from "../../api/keys";
import { getMe } from "../../api/me";
import { queryKeys } from "../../api/queryKeys";

export function useMeQuery() {
  const client = useApiClient();
  return useQuery({ queryKey: queryKeys.me, queryFn: () => getMe(client) });
}

export function useAgentsQuery() {
  const client = useApiClient();
  return useQuery({ queryKey: queryKeys.agents, queryFn: () => listAgents(client) });
}

export function useCreateAgentMutation(onCreated: () => void) {
  const client = useApiClient();
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (name: string) => createAgent(client, name),
    onSuccess: () => {
      onCreated();
      void queryClient.invalidateQueries({ queryKey: queryKeys.agents });
    }
  });
}

export function useDeleteAgentMutation(onDeleted: () => void) {
  const client = useApiClient();
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (id: string) => deleteAgent(client, id),
    onSuccess: () => {
      onDeleted();
      void queryClient.invalidateQueries({ queryKey: queryKeys.agents });
    }
  });
}

export function useConnectionsQuery(spaceId: string) {
  const client = useApiClient();
  return useQuery({ queryKey: queryKeys.connections(spaceId), queryFn: () => listConnections(client, spaceId), enabled: !!spaceId });
}

export function useConnectAgentMutation(spaceId: string) {
  const client = useApiClient();
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: ({ agentId, permission }: { agentId: string; permission: Permission }) => connectAgent(client, spaceId, agentId, permission),
    onSuccess: () => void queryClient.invalidateQueries({ queryKey: queryKeys.connections(spaceId) })
  });
}

export function useDisconnectAgentMutation(spaceId: string) {
  const client = useApiClient();
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (agentId: string) => disconnectAgent(client, spaceId, agentId),
    onSuccess: () => void queryClient.invalidateQueries({ queryKey: queryKeys.connections(spaceId) })
  });
}

export function useApiKeysQuery(queryKey: QueryKey, list: () => Promise<ApiKeyListResponse>) {
  return useQuery({ queryKey, queryFn: list });
}

export function useCreateApiKeyMutation(queryKey: QueryKey, create: (input: { name: string; expires_at: string }) => Promise<MintedKey>, onCreated: (key: MintedKey) => void) {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: create,
    onSuccess: (key) => {
      onCreated(key);
      void queryClient.invalidateQueries({ queryKey });
    }
  });
}

export function useRevokeApiKeyMutation(queryKey: QueryKey, revoke: (id: string) => Promise<void>, onRevoked: () => void) {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: revoke,
    onSuccess: () => {
      onRevoked();
      void queryClient.invalidateQueries({ queryKey });
    }
  });
}

export function useMyKeyManagerProps() {
  const client = useApiClient();
  return {
    queryKey: queryKeys.myKeys,
    list: () => listMyKeys(client),
    create: (input: { name: string; expires_at: string }) => createMyKey(client, input),
    revoke: (id: string) => revokeMyKey(client, id)
  };
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
