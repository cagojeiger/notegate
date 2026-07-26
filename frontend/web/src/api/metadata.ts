import type { ApiClient } from "./client";
import type { RestNode } from "./types";

export function replaceMetadata(client: ApiClient, spaceId: string, nodeId: string, metadata: Record<string, unknown>): Promise<RestNode> {
  return client.put<RestNode>(`/api/v1/spaces/${spaceId}/nodes/${nodeId}/metadata`, { metadata });
}
