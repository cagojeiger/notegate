import type { ApiClient } from "./client";
import type { NodeLinkIndexResponse, SpaceLinkIndexResponse } from "./types";

export function getNodeLinkIndex(client: ApiClient, spaceId: string, nodeId: string) {
  return client.get<NodeLinkIndexResponse>(`/api/v1/spaces/${spaceId}/nodes/${nodeId}/links`);
}

export function syncNodeLinkIndex(client: ApiClient, spaceId: string, nodeId: string) {
  return client.post<{ status: "queued" }>(`/api/v1/spaces/${spaceId}/nodes/${nodeId}/links/sync`);
}

export function getSpaceLinkIndex(client: ApiClient, spaceId: string) {
  return client.get<SpaceLinkIndexResponse>(`/api/v1/spaces/${spaceId}/link-index`);
}

export function reindexSpaceLinks(client: ApiClient, spaceId: string) {
  return client.post<{ status: "queued" }>(`/api/v1/spaces/${spaceId}/link-index/reindex`);
}
