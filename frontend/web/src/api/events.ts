import type { ApiClient } from "./client";
import type {
  AuditEventListResponse,
  FileChangeEventListResponse,
  FileChangeSyncResponse
} from "./types";

const DEFAULT_EVENT_LIMIT = 50;
const SYNC_EVENT_LIMIT = 100;

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

export function syncFileChanges(
  client: ApiClient,
  spaceId: string,
  afterId?: number
): Promise<FileChangeSyncResponse> {
  const params = new URLSearchParams({ limit: String(SYNC_EVENT_LIMIT) });
  if (afterId !== undefined) params.set("after_id", String(afterId));
  return client.get<FileChangeSyncResponse>(`/api/v1/spaces/${spaceId}/file-change-sync?${params}`);
}

export async function drainFileChanges(
  client: ApiClient,
  spaceId: string,
  afterId?: number
): Promise<FileChangeSyncResponse> {
  let page = await syncFileChanges(client, spaceId, afterId);
  if (page.resync_required || !page.has_more) return page;

  const changes = [...page.changes];
  while (page.has_more) {
    const afterId = page.next_after_id;
    page = await syncFileChanges(client, spaceId, afterId);
    if (page.resync_required) return page;
    if (page.has_more && page.next_after_id <= afterId) {
      throw new Error("file change sync token did not advance");
    }
    changes.push(...page.changes);
  }
  return { ...page, changes };
}
