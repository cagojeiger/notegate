import type { ApiClient } from "./client";
import type {
  LinkReferenceDirection,
  LinkReferenceListResponse,
  SpaceLinkIndexResponse
} from "./types";

const DEFAULT_LINK_REFERENCE_LIMIT = 50;

export function listNodeLinkReferences(
  client: ApiClient,
  spaceId: string,
  nodeId: string,
  direction: LinkReferenceDirection,
  cursor?: string | null
) {
  const params = new URLSearchParams({ limit: String(DEFAULT_LINK_REFERENCE_LIMIT) });
  if (cursor) params.set("cursor", cursor);
  return client.get<LinkReferenceListResponse>(
    `/api/v1/spaces/${spaceId}/nodes/${nodeId}/links/${direction}?${params}`
  );
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
