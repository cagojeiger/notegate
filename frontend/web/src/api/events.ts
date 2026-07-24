import type { ApiClient } from "./client";
import type { AuditEventListResponse, FileChangeEventListResponse } from "./types";

const DEFAULT_EVENT_LIMIT = 50;

export function listAuditEvents(client: ApiClient, cursor?: string | null): Promise<AuditEventListResponse> {
  const params = new URLSearchParams({ limit: String(DEFAULT_EVENT_LIMIT) });
  if (cursor) params.set("cursor", cursor);
  return client.get<AuditEventListResponse>(`/api/v1/me/audit-events?${params}`);
}

export function listFileChangeEvents(
  client: ApiClient,
  spaceId: string,
  options: { nodeId?: string | null; cursor?: string | null; limit?: number } = {}
): Promise<FileChangeEventListResponse> {
  const params = new URLSearchParams({ limit: String(options.limit ?? DEFAULT_EVENT_LIMIT) });
  if (options.nodeId) params.set("node_id", options.nodeId);
  if (options.cursor) params.set("cursor", options.cursor);
  return client.get<FileChangeEventListResponse>(`/api/v1/spaces/${spaceId}/file-change-events?${params}`);
}
