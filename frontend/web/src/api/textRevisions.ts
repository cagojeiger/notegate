import type { ApiClient } from "./client";
import type { Page } from "./types";

export type TextRevision = {
  id: string;
  node_id: string;
  content_sha256: string;
  byte_len: number;
  line_count: number;
  written_at: string;
  author_id: string;
  group_id: string;
  source: string;
  superseded_at: string;
};
export type TextRevisionBody = { revision: TextRevision; content: string };
export type TextRevisionList = { revisions: TextRevision[]; page: Page };
export type RestoredText = { node_id: string; content_sha256: string; byte_len: number; line_count: number };

function path(spaceId: string, nodeId: string) {
  return `/api/v1/spaces/${spaceId}/text/${nodeId}/revisions`;
}
export function listTextRevisions(client: ApiClient, spaceId: string, nodeId: string, cursor: string | null) {
  const params = new URLSearchParams({ limit: "50" });
  if (cursor) params.set("cursor", cursor);
  return client.get<TextRevisionList>(`${path(spaceId, nodeId)}?${params}`);
}
export function readTextRevision(client: ApiClient, spaceId: string, nodeId: string, revisionId: string) {
  return client.get<TextRevisionBody>(`${path(spaceId, nodeId)}/${revisionId}`);
}
export function restoreTextRevision(client: ApiClient, spaceId: string, nodeId: string, revisionId: string, sha: string) {
  return client.post<RestoredText>(`${path(spaceId, nodeId)}/${revisionId}/restore`, { expected_sha256: sha });
}
