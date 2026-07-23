import type { ApiClient } from "./client";
import { ApiError } from "./errors";
import type {
  BeginFileUploadResponse,
  CompletedFileUploadPart,
  FilePreviewUrlResponse,
  FileResponse,
  PreparedFileUploadPart
} from "./types";

export type FileUploadInput = {
  parentNodeId: string;
  name: string;
  file: File;
};

type FileTransferOptions = {
  signal?: AbortSignal;
  onProgress?: (uploadedBytes: number, totalBytes: number) => void;
};

type PreparedPartsResponse = {
  parts: PreparedFileUploadPart[];
};

const PART_URL_BATCH_SIZE = 16;
const PART_UPLOAD_CONCURRENCY = 4;
const PART_UPLOAD_ATTEMPTS = 3;
const FILE_PREVIEW_EXPIRY_SAFETY_MS = 60_000;

export function filePreviewStaleTime(expiresAt: string, cachedAt: number): number {
  const expiresAtMs = Date.parse(expiresAt);
  if (!Number.isFinite(expiresAtMs) || cachedAt <= 0) return 0;
  return Math.max(0, expiresAtMs - cachedAt - FILE_PREVIEW_EXPIRY_SAFETY_MS);
}

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

export async function transferFile(
  client: ApiClient,
  spaceId: string,
  upload: BeginFileUploadResponse,
  file: File,
  options: FileTransferOptions = {}
): Promise<CompletedFileUploadPart[] | undefined> {
  if (upload.transfer.mode === "single") {
    await putBytes(upload.transfer.url, upload.transfer.headers, file, options);
    return undefined;
  }

  const progress = new Map<number, number>();
  const completed: CompletedFileUploadPart[] = [];
  for (let start = 1; start <= upload.transfer.part_count; start += PART_URL_BATCH_SIZE) {
    let pending = Array.from(
      { length: Math.min(PART_URL_BATCH_SIZE, upload.transfer.part_count - start + 1) },
      (_, index) => start + index
    );
    for (let attempt = 1; pending.length > 0 && attempt <= PART_UPLOAD_ATTEMPTS; attempt += 1) {
      const prepared = await prepareFileUploadParts(client, spaceId, upload.upload_id, pending);
      const failures = await uploadPreparedParts(
        prepared,
        file,
        upload.transfer.part_size,
        progress,
        completed,
        options
      );
      pending = failures.map(({ partNumber }) => partNumber);
      for (const partNumber of pending) progress.set(partNumber, 0);
      reportMultipartProgress(progress, file.size, options);
      if (attempt === PART_UPLOAD_ATTEMPTS && failures.length > 0) throw failures[0].error;
    }
  }
  completed.sort((left, right) => left.part_number - right.part_number);
  return completed;
}

export async function completeFileUpload(
  client: ApiClient,
  spaceId: string,
  uploadId: string,
  completedParts?: CompletedFileUploadPart[]
): Promise<FileResponse> {
  const completePath = `/api/v1/spaces/${spaceId}/file-uploads/${uploadId}/complete`;
  const body = completedParts === undefined ? undefined : { completed_parts: completedParts };
  const complete = () => body === undefined
    ? client.post<FileResponse>(completePath)
    : client.post<FileResponse>(completePath, body);
  try {
    return await complete();
  } catch (error) {
    if (error instanceof ApiError && error.status < 500) throw error;
    return complete();
  }
}

export function abortFileUpload(client: ApiClient, spaceId: string, uploadId: string): Promise<void> {
  return client.delete<void>(`/api/v1/spaces/${spaceId}/file-uploads/${uploadId}`);
}

export function getFilePreviewUrl(client: ApiClient, spaceId: string, nodeId: string): Promise<FilePreviewUrlResponse> {
  return client.get<FilePreviewUrlResponse>(`/api/v1/spaces/${spaceId}/files/${nodeId}/preview-url`);
}

export function downloadFile(client: ApiClient, spaceId: string, nodeId: string, filename: string): Promise<void> {
  return client.download(`/api/v1/spaces/${spaceId}/files/${nodeId}/content`, filename);
}

function prepareFileUploadParts(
  client: ApiClient,
  spaceId: string,
  uploadId: string,
  partNumbers: number[]
): Promise<PreparedFileUploadPart[]> {
  return client
    .post<PreparedPartsResponse>(`/api/v1/spaces/${spaceId}/file-uploads/${uploadId}/parts`, {
      part_numbers: partNumbers
    })
    .then(({ parts }) => parts);
}

async function uploadPreparedParts(
  prepared: PreparedFileUploadPart[],
  file: File,
  partSize: number,
  progress: Map<number, number>,
  completed: CompletedFileUploadPart[],
  options: FileTransferOptions
): Promise<Array<{ partNumber: number; error: unknown }>> {
  let nextIndex = 0;
  const failures: Array<{ partNumber: number; error: unknown }> = [];
  const workers = Array.from(
    { length: Math.min(PART_UPLOAD_CONCURRENCY, prepared.length) },
    async () => {
      while (nextIndex < prepared.length) {
        const part = prepared[nextIndex++];
        const offset = (part.part_number - 1) * partSize;
        const bytes = file.slice(offset, offset + part.content_length);
        if (bytes.size !== part.content_length) {
          failures.push({
            partNumber: part.part_number,
            error: new ApiError("Invalid multipart upload geometry", 400, "invalid_multipart")
          });
          continue;
        }
        try {
          const etag = await putBytes(part.url, part.headers, bytes, {
            signal: options.signal,
            onProgress: (uploadedBytes) => {
              progress.set(part.part_number, uploadedBytes);
              reportMultipartProgress(progress, file.size, options);
            }
          }, true);
          progress.set(part.part_number, part.content_length);
          completed.push({ part_number: part.part_number, etag });
        } catch (error) {
          if (isAbortError(error)) throw error;
          failures.push({ partNumber: part.part_number, error });
        }
      }
    }
  );
  await Promise.all(workers);
  return failures;
}

function putBytes(
  url: string,
  headers: Record<string, string>,
  bytes: Blob,
  options: FileTransferOptions,
  requireEtag = false
): Promise<string> {
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

    request.open("PUT", url);
    request.withCredentials = false;
    for (const [name, value] of Object.entries(headers)) request.setRequestHeader(name, value);
    request.upload.onprogress = (event) => {
      const total = event.lengthComputable && event.total > 0 ? event.total : bytes.size;
      options.onProgress?.(Math.min(event.loaded, total), total);
    };
    request.onload = () => {
      if (request.status >= 200 && request.status < 300) {
        const etag = request.getResponseHeader("etag")?.trim() ?? "";
        if (requireEtag && etag.length === 0) {
          finish(() => reject(new ApiError(
            "File storage did not expose the multipart ETag",
            502,
            "multipart_etag_missing"
          )));
          return;
        }
        options.onProgress?.(bytes.size, bytes.size);
        finish(() => resolve(etag));
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
    request.send(bytes);
  });
}

function reportMultipartProgress(
  progress: Map<number, number>,
  totalBytes: number,
  options: FileTransferOptions
) {
  const uploadedBytes = Array.from(progress.values()).reduce((sum, bytes) => sum + bytes, 0);
  options.onProgress?.(Math.min(uploadedBytes, totalBytes), totalBytes);
}

function isAbortError(error: unknown): boolean {
  return error instanceof DOMException && error.name === "AbortError";
}
