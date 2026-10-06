import type { ApiClient } from "./client";
import type { Page } from "./types";

export type TrashItem = {
  id: string;
  space_id: string;
  space_name: string;
  kind: "space" | "folder" | "text" | "file";
  name: string;
  path: string;
  deleted_at: string;
  purge_after: string;
  deletion_operation_id?: string | null;
  recoverable: boolean;
  deletion_pending: boolean;
};

export type TrashList = { items: TrashItem[]; page: Page };

export function listTrash(client: ApiClient, cursor: string | null) {
  const params = new URLSearchParams({ limit: "50" });
  if (cursor) params.set("cursor", cursor);
  return client.get<TrashList>(`/api/v1/me/trash?${params}`);
}

function itemPath(item: TrashItem) {
  const space = `/api/v1/me/trash/spaces/${item.space_id}`;
  return item.kind === "space" ? space : `${space}/nodes/${item.id}`;
}

export function restoreTrash(client: ApiClient, item: TrashItem) {
  return client.post<void>(`${itemPath(item)}/restore`);
}

export function purgeTrash(client: ApiClient, item: TrashItem) {
  return client.delete<{ status: "deletion_requested" }>(itemPath(item));
}
