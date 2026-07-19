import type { ApiClient } from "./client";
import { ApiError } from "./errors";
import type { BeginFileUploadResponse, FileResponse } from "./types";

export function statFile(client: ApiClient, spaceId: string, nodeId: string): Promise<FileResponse> {
  return client.get<FileResponse>(`/api/v1/spaces/${spaceId}/files/${nodeId}`);
}

export async function uploadFile(client: ApiClient, spaceId: string, input: { parentNodeId: string; name: string; file: File }): Promise<FileResponse> {
  const upload = await client.post<BeginFileUploadResponse>(`/api/v1/spaces/${spaceId}/file-uploads`, {
    parent_node_id: input.parentNodeId,
    name: input.name,
    byte_len: input.file.size,
    media_type: input.file.type || "application/octet-stream",
    original_filename: input.file.name
  });

  const transferResponse = await fetch(upload.transfer.url, {
    method: "PUT",
    headers: upload.transfer.headers,
    credentials: "omit",
    body: input.file
  });
  if (!transferResponse.ok) {
    throw new ApiError("File upload failed", transferResponse.status, "object_upload_failed");
  }

  const completePath = `/api/v1/spaces/${spaceId}/file-uploads/${upload.upload_id}/complete`;
  try {
    return await client.post<FileResponse>(completePath);
  } catch (error) {
    if (error instanceof ApiError && error.status < 500) throw error;
    return client.post<FileResponse>(completePath);
  }
}

export function downloadFile(client: ApiClient, spaceId: string, nodeId: string): Promise<Blob> {
  return client.download(`/api/v1/spaces/${spaceId}/files/${nodeId}/content`);
}
