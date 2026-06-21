import type { ApiClient } from "./client";
import type { ChildrenResponse, NodeKind, NodeRevealResponse, RestNode, RestNodeListResponse } from "./types";

export function getNode(client: ApiClient, spaceId: string, nodeId: string): Promise<RestNode> {
  return client.get<RestNode>(`/api/v1/spaces/${spaceId}/nodes/${nodeId}`);
}

export function listChildren(client: ApiClient, spaceId: string, nodeId: string, cursor?: string | null): Promise<ChildrenResponse> {
  const params = new URLSearchParams({ limit: "100" });
  if (cursor) params.set("cursor", cursor);
  return client.get<ChildrenResponse>(`/api/v1/spaces/${spaceId}/nodes/${nodeId}/children?${params}`);
}

export function listNodes(
  client: ApiClient,
  spaceId: string,
  options: { kind?: NodeKind; sort?: "updated_at_desc" | "name_asc"; cursor?: string | null } = {}
): Promise<RestNodeListResponse> {
  const params = new URLSearchParams({ limit: "50", sort: options.sort ?? "updated_at_desc" });
  if (options.kind) params.set("kind", options.kind);
  if (options.cursor) params.set("cursor", options.cursor);
  return client.get<RestNodeListResponse>(`/api/v1/spaces/${spaceId}/nodes?${params}`);
}

export function revealNode(client: ApiClient, spaceId: string, nodeId: string): Promise<NodeRevealResponse> {
  return client.get<NodeRevealResponse>(`/api/v1/spaces/${spaceId}/nodes/${nodeId}/reveal`);
}

export function resolveNodePath(client: ApiClient, spaceId: string, path: string): Promise<RestNode> {
  const params = new URLSearchParams({ path });
  return client.get<RestNode>(`/api/v1/spaces/${spaceId}/paths/resolve?${params}`);
}

export function createNode(
  client: ApiClient,
  spaceId: string,
  input: { parent_id: string; kind: "folder" | "text"; name: string; content?: string }
): Promise<RestNode> {
  return client.post<RestNode>(`/api/v1/spaces/${spaceId}/nodes`, input);
}

export function updateNode(client: ApiClient, spaceId: string, nodeId: string, input: { name?: string; sort_order?: number }): Promise<RestNode> {
  return client.patch<RestNode>(`/api/v1/spaces/${spaceId}/nodes/${nodeId}`, input);
}

export function moveNode(
  client: ApiClient,
  spaceId: string,
  nodeId: string,
  input: { new_parent_id: string; new_name?: string; expected_parent_id?: string | null }
): Promise<RestNode> {
  return client.post<RestNode>(`/api/v1/spaces/${spaceId}/nodes/${nodeId}/move`, input);
}

export function deleteNode(client: ApiClient, spaceId: string, nodeId: string, recursive: boolean): Promise<void> {
  const params = new URLSearchParams({ recursive: String(recursive) });
  return client.delete<void>(`/api/v1/spaces/${spaceId}/nodes/${nodeId}?${params}`);
}
