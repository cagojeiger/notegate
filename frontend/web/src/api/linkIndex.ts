import type { ApiClient } from "./client";

export type LinkIndexStatus = "queued" | "running" | "rebuilding" | "ready" | "failed";
export type LinkIndexFreshness = "current" | "updating" | "rebuilding" | "failed";
export type LinkReferenceKind = "link" | "image";
export type LinkReferenceStatus = "resolved" | "deleted" | "missing" | "invalid";

export type LinkIndexState = {
  space_id: string;
  desired_generation: number;
  applied_generation: number;
  status: LinkIndexStatus;
  freshness: LinkIndexFreshness;
  last_indexed_at: string | null;
};

export type LinkReference = {
  id: number;
  kind: LinkReferenceKind;
  status: LinkReferenceStatus;
  raw_href: string;
  normalized_target_path: string | null;
  occurrence_count: number;
  source_node_id: string;
  source_name: string;
  source_path: string | null;
  target_node_id: string | null;
  target_name: string | null;
  target_path: string | null;
};

export type NodeLinkSummary = {
  index: LinkIndexState;
  outgoing_count: number;
  incoming_count: number;
  broken_count: number;
  outgoing: LinkReference[];
  incoming: LinkReference[];
  outgoing_truncated: boolean;
  incoming_truncated: boolean;
};

export function getLinkIndexState(client: ApiClient, spaceId: string): Promise<LinkIndexState> {
  return client.get<LinkIndexState>(`/api/v1/spaces/${spaceId}/link-index`);
}

export function requestLinkReindex(client: ApiClient, spaceId: string): Promise<LinkIndexState> {
  return client.post<LinkIndexState>(`/api/v1/spaces/${spaceId}/link-index/rebuild`, {});
}

export function getNodeLinks(
  client: ApiClient,
  spaceId: string,
  nodeId: string
): Promise<NodeLinkSummary> {
  return client.get<NodeLinkSummary>(`/api/v1/spaces/${spaceId}/nodes/${nodeId}/links`);
}
