import type { ApiClient } from "./client";
import type { Page } from "./types";

export type Permission = "read" | "write";

export type ConnectionAgentRef = { id: string; kind: string; display_name: string };
export type Connection = { agent: ConnectionAgentRef; permission: Permission; connected_at: string };
export type ConnectionListResponse = {
  connections: Connection[];
  page: Page;
};

export function listConnections(client: ApiClient, spaceId: string): Promise<ConnectionListResponse> {
  return client.get<ConnectionListResponse>(`/api/v1/spaces/${spaceId}/agents?limit=100`);
}

export function connectAgent(client: ApiClient, spaceId: string, agentId: string, permission: Permission): Promise<Connection> {
  return client.put<Connection>(`/api/v1/spaces/${spaceId}/agents/${agentId}`, { permission });
}

export function disconnectAgent(client: ApiClient, spaceId: string, agentId: string): Promise<void> {
  return client.delete<void>(`/api/v1/spaces/${spaceId}/agents/${agentId}`);
}
