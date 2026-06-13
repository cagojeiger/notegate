import type { ApiClient } from "./client";
import type { ApiKeyListResponse, MintedKey } from "./keys";

export type Agent = { id: string; name: string; owner_user_id: string };
export type AgentsListResponse = {
  agents: Agent[];
  page: { limit: number; returned: number; has_more: boolean; next_cursor: string | null };
};

export function listAgents(client: ApiClient): Promise<AgentsListResponse> {
  return client.get<AgentsListResponse>("/api/v1/agents?limit=100");
}

export function createAgent(client: ApiClient, name: string): Promise<Agent> {
  return client.post<Agent>("/api/v1/agents", { name });
}

export function deleteAgent(client: ApiClient, agentId: string): Promise<void> {
  return client.delete<void>(`/api/v1/agents/${agentId}`);
}

export function listAgentKeys(client: ApiClient, agentId: string): Promise<ApiKeyListResponse> {
  return client.get<ApiKeyListResponse>(`/api/v1/agents/${agentId}/keys?limit=100`);
}

export function createAgentKey(client: ApiClient, agentId: string, input: { name: string; expires_at: string }): Promise<MintedKey> {
  return client.post<MintedKey>(`/api/v1/agents/${agentId}/keys`, { name: input.name, expires_at: input.expires_at });
}

export function revokeAgentKey(client: ApiClient, agentId: string, keyId: string): Promise<void> {
  return client.delete<void>(`/api/v1/agents/${agentId}/keys/${keyId}`);
}
