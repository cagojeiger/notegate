import type { ApiClient } from "./client";
import type { MetadataResponse, RestNode } from "./types";

export function getMetadata(client: ApiClient, spaceId: string, nodeId: string): Promise<MetadataResponse> {
  return client.get<MetadataResponse>(`/api/v1/spaces/${spaceId}/nodes/${nodeId}/metadata`);
}

export function replaceMetadata(client: ApiClient, spaceId: string, nodeId: string, metadata: Record<string, unknown>): Promise<RestNode> {
  return client.put<RestNode>(`/api/v1/spaces/${spaceId}/nodes/${nodeId}/metadata`, { metadata });
}
