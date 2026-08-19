import type { ApiClient } from "./client";
import type { AsyncOperationResponse, Page } from "./types";

export type NodeLinkProjectionStatus = {
  status: "idle" | "pending" | "syncing" | "failed";
  space_pending: boolean;
  projected_at: string | null;
  failure_code: string | null;
  failed_at: string | null;
};

export type NodeLinkDirection = "outgoing" | "incoming";

export type NodeLink = {
  node_id: string | null;
  path: string;
  kind: "link" | "image";
  occurrence_count: number;
};

export type NodeLinksResponse = {
  links: NodeLink[];
  page: Page;
};

export type SpaceLinkIndexStatus = {
  pending: boolean;
};

export function getNodeLinkStatus(
  client: ApiClient,
  spaceId: string,
  nodeId: string
): Promise<NodeLinkProjectionStatus> {
  return client.get<NodeLinkProjectionStatus>(
    `/api/v1/spaces/${spaceId}/nodes/${nodeId}/links`
  );
}

export function listNodeLinks(
  client: ApiClient,
  spaceId: string,
  nodeId: string,
  direction: NodeLinkDirection,
  cursor?: string | null
): Promise<NodeLinksResponse> {
  const params = new URLSearchParams({ limit: "50" });
  if (cursor) params.set("cursor", cursor);
  return client.get<NodeLinksResponse>(
    `/api/v1/spaces/${spaceId}/nodes/${nodeId}/links/${direction}?${params}`
  );
}

export function requestNodeLinkSync(
  client: ApiClient,
  spaceId: string,
  nodeId: string
): Promise<AsyncOperationResponse> {
  return client.post<AsyncOperationResponse>(
    `/api/v1/spaces/${spaceId}/nodes/${nodeId}/links/sync`
  );
}

export function requestSpaceLinkReindex(
  client: ApiClient,
  spaceId: string
): Promise<AsyncOperationResponse> {
  return client.post<AsyncOperationResponse>(
    `/api/v1/spaces/${spaceId}/link-index/reindex`
  );
}

export function getSpaceLinkIndexStatus(
  client: ApiClient,
  spaceId: string
): Promise<SpaceLinkIndexStatus> {
  return client.get<SpaceLinkIndexStatus>(
    `/api/v1/spaces/${spaceId}/link-index/status`
  );
}
