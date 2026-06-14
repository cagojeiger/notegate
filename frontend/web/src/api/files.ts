import type { ApiClient } from "./client";
import type { FileResponse } from "./types";

export function statFile(client: ApiClient, spaceId: string, nodeId: string): Promise<FileResponse> {
  return client.get<FileResponse>(`/api/v1/spaces/${spaceId}/files/${nodeId}`);
}

export function uploadFile(client: ApiClient, spaceId: string, input: { parentNodeId: string; name: string; file: File }): Promise<FileResponse> {
  const form = new FormData();
  form.set("parent_node_id", input.parentNodeId);
  form.set("name", input.name);
  form.set("file", input.file);
  if (input.file.type) form.set("media_type", input.file.type);
  form.set("original_filename", input.file.name);
  return client.upload<FileResponse>(`/api/v1/spaces/${spaceId}/files`, form);
}

export function downloadFile(client: ApiClient, spaceId: string, nodeId: string): Promise<Blob> {
  return client.download(`/api/v1/spaces/${spaceId}/files/${nodeId}/content`);
}
