import type { ApiClient } from "./client";
import { ApiError } from "./errors";
import type { BeginFileUploadResponse, FileResponse } from "./types";

export type FileUploadInput = {
  parentNodeId: string;
  name: string;
  file: File;
};

type FileTransferOptions = {
  signal?: AbortSignal;
  onProgress?: (uploadedBytes: number, totalBytes: number) => void;
};

export function statFile(client: ApiClient, spaceId: string, nodeId: string): Promise<FileResponse> {
  return client.get<FileResponse>(`/api/v1/spaces/${spaceId}/files/${nodeId}`);
}

export function beginFileUpload(client: ApiClient, spaceId: string, input: FileUploadInput): Promise<BeginFileUploadResponse> {
  return client.post<BeginFileUploadResponse>(`/api/v1/spaces/${spaceId}/file-uploads`, {
    parent_node_id: input.parentNodeId,
    name: input.name,
    byte_len: input.file.size,
    media_type: input.file.type || "application/octet-stream",
    original_filename: input.file.name
  });
}

export function transferFile(upload: BeginFileUploadResponse, file: File, options: FileTransferOptions = {}): Promise<void> {
  return new Promise((resolve, reject) => {
    const request = new XMLHttpRequest();
    let settled = false;

    function finish(action: () => void) {
      if (settled) return;
      settled = true;
      options.signal?.removeEventListener("abort", abort);
      action();
    }

    function abort() {
      request.abort();
    }

    request.open("PUT", upload.transfer.url);
    request.withCredentials = false;
    for (const [name, value] of Object.entries(upload.transfer.headers)) {
      request.setRequestHeader(name, value);
    }
    request.upload.onprogress = (event) => {
      const total = event.lengthComputable && event.total > 0 ? event.total : file.size;
      options.onProgress?.(Math.min(event.loaded, total), total);
    };
    request.onload = () => {
      if (request.status >= 200 && request.status < 300) {
        options.onProgress?.(file.size, file.size);
        finish(resolve);
      } else {
        finish(() => reject(new ApiError("File upload failed", request.status, "object_upload_failed")));
      }
    };
    request.onerror = () => finish(() => reject(new ApiError("File upload failed", 503, "object_upload_failed")));
    request.onabort = () => finish(() => reject(new DOMException("File upload canceled", "AbortError")));

    if (options.signal?.aborted) {
      finish(() => reject(new DOMException("File upload canceled", "AbortError")));
      return;
    }
    options.signal?.addEventListener("abort", abort, { once: true });
    request.send(file);
  });
}

export async function completeFileUpload(client: ApiClient, spaceId: string, uploadId: string): Promise<FileResponse> {
  const completePath = `/api/v1/spaces/${spaceId}/file-uploads/${uploadId}/complete`;
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
