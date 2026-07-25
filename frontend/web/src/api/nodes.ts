import type { ApiClient } from "./client";
import type {
  BatchChildrenResponse,
  ChildrenResponse,
  NodeKind,
  NodeRevealResponse,
  NodeSummary,
  RestNode,
  RestNodeListResponse
} from "./types";

export const MAX_BATCH_CHILDREN_PARENTS = 16;

export function getNode(client: ApiClient, spaceId: string, nodeId: string): Promise<RestNode> {
  return client.get<RestNode>(`/api/v1/spaces/${spaceId}/nodes/${nodeId}`);
}

type WireNodeSummary = Omit<NodeSummary, "space_id" | "original_filename">;
type WireChildrenResponse = Omit<ChildrenResponse, "children"> & {
  children: WireNodeSummary[];
};
type WireNodeListResponse = Omit<RestNodeListResponse, "nodes"> & {
  nodes: WireNodeSummary[];
};
type WireBatchChildrenResponse = Omit<BatchChildrenResponse, "results"> & {
  results: Array<Omit<BatchChildrenResponse["results"][number], "children"> & {
    children: WireNodeSummary[];
  }>;
};

export async function listChildren(client: ApiClient, spaceId: string, nodeId: string, cursor?: string | null): Promise<ChildrenResponse> {
  const params = new URLSearchParams({ limit: "100", view: "summary" });
  if (cursor) params.set("cursor", cursor);
  const response = await client.get<WireChildrenResponse>(
    `/api/v1/spaces/${spaceId}/nodes/${nodeId}/children?${params}`
  );
  return {
    ...response,
    children: withSpaceId(spaceId, response.children)
  };
}

export async function batchListChildren(
  client: ApiClient,
  spaceId: string,
  parentIds: string[]
): Promise<BatchChildrenResponse> {
  const response = await client.post<WireBatchChildrenResponse>(
    `/api/v1/spaces/${spaceId}/nodes:batchListChildren`,
    { parent_ids: parentIds, limit: 100 }
  );
  return {
    ...response,
    results: response.results.map((result) => ({
      ...result,
      children: withSpaceId(spaceId, result.children)
    }))
  };
}

export async function listNodes(
  client: ApiClient,
  spaceId: string,
  options: { kind?: NodeKind; sort?: "updated_at_desc" | "name_asc"; cursor?: string | null } = {}
): Promise<RestNodeListResponse> {
  const params = new URLSearchParams({
    limit: "50",
    sort: options.sort ?? "updated_at_desc",
    view: "summary"
  });
  if (options.kind) params.set("kind", options.kind);
  if (options.cursor) params.set("cursor", options.cursor);
  const response = await client.get<WireNodeListResponse>(
    `/api/v1/spaces/${spaceId}/nodes?${params}`
  );
  return {
    ...response,
    nodes: withSpaceId(spaceId, response.nodes)
  };
}

function withSpaceId(spaceId: string, nodes: WireNodeSummary[]): NodeSummary[] {
  return nodes.map((node) => ({ ...node, space_id: spaceId }));
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
