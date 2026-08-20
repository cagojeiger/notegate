import type { ApiClient } from "./client";
import { postAsyncCommand } from "./asyncCommands";
import { isApiRouteNotFound } from "./errors";
import type { AsyncCommandAck, CommandAvailability, Page } from "./types";

export type NodeLinkProjectionStatus = {
  status: "idle" | "pending" | "syncing" | "failed";
  space_pending: boolean;
  projected_at: string | null;
  failure_code: string | null;
  failed_at: string | null;
  availability: CommandAvailability;
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
  status: "idle" | "pending" | "unknown";
  availability: CommandAvailability;
};

type NodeLinkProjectionStatusWire = Omit<NodeLinkProjectionStatus, "availability"> & {
  availability?: CommandAvailability;
};

export async function getNodeLinkStatus(
  client: ApiClient,
  spaceId: string,
  nodeId: string
): Promise<NodeLinkProjectionStatus> {
  const status = await client.get<NodeLinkProjectionStatusWire>(
    `/api/v1/spaces/${spaceId}/nodes/${nodeId}/links`
  );
  const pending = status.status === "pending" || status.status === "syncing" || status.space_pending;
  return {
    ...status,
    availability: status.availability ?? (pending
      ? { can_trigger: false, reason: "pending", retry_at: null }
      : { can_trigger: true, reason: null, retry_at: null })
  };
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

export async function requestNodeLinkSync(
  client: ApiClient,
  spaceId: string,
  nodeId: string
): Promise<AsyncCommandAck> {
  return postAsyncCommand(
    client,
    `/api/v1/spaces/${spaceId}/nodes/${nodeId}/actions/reindex-links`,
    `/api/v1/spaces/${spaceId}/nodes/${nodeId}/links/sync`
  );
}

export async function requestSpaceLinkReindex(
  client: ApiClient,
  spaceId: string
): Promise<AsyncCommandAck> {
  return postAsyncCommand(
    client,
    `/api/v1/spaces/${spaceId}/actions/reindex-links`,
    `/api/v1/spaces/${spaceId}/link-index/reindex`
  );
}

export async function getSpaceLinkIndexStatus(
  client: ApiClient,
  spaceId: string
): Promise<SpaceLinkIndexStatus> {
  try {
    return await client.get<SpaceLinkIndexStatus>(
      `/api/v1/spaces/${spaceId}/link-index`
    );
  } catch (error) {
    if (isApiRouteNotFound(error)) {
      return {
        status: "unknown",
        availability: { can_trigger: true, reason: null, retry_at: null }
      };
    }
    throw error;
  }
}
