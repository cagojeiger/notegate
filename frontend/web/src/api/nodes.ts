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
