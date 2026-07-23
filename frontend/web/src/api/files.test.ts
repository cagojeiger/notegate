import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import type { ApiClient } from "./client";
import { ApiError } from "./errors";
import {
  beginFileUpload,
  completeFileUpload,
  filePreviewStaleTime,
  getFilePreviewUrl,
  transferFile
} from "./files";
import type { BeginFileUploadResponse } from "./types";

const singleUpload: BeginFileUploadResponse = {
  upload_id: "upload-1",
  transfer: {
    mode: "single",
    url: "https://objects.test/notegate/upload-1",
    headers: { "content-type": "text/plain", "if-none-match": "*" }
  }
};

class FakeXmlHttpRequest {
  static instances: FakeXmlHttpRequest[] = [];

  readonly upload = { onprogress: null as ((event: ProgressEvent) => void) | null };
  readonly headers = new Map<string, string>();
  readonly responseHeaders = new Map<string, string>();
  method = "";
  url = "";
  body: XMLHttpRequestBodyInit | null = null;
  status = 0;
  withCredentials = true;
  onload: (() => void) | null = null;
  onerror: (() => void) | null = null;
  onabort: (() => void) | null = null;

  constructor() {
    FakeXmlHttpRequest.instances.push(this);
  }

  open(method: string, url: string) {
    this.method = method;
    this.url = url;
  }

  setRequestHeader(name: string, value: string) {
    this.headers.set(name, value);
  }

  getResponseHeader(name: string) {
    return this.responseHeaders.get(name.toLowerCase()) ?? null;
  }

  send(body: XMLHttpRequestBodyInit | null) {
    this.body = body;
  }

  abort() {
    this.onabort?.();
  }

  progress(loaded: number, total: number) {
    this.upload.onprogress?.({ lengthComputable: true, loaded, total } as ProgressEvent);
  }

  respond(status: number, etag?: string) {
    this.status = status;
    if (etag) this.responseHeaders.set("etag", etag);
    this.onload?.();
  }
}

describe("files api", () => {
  beforeEach(() => {
    FakeXmlHttpRequest.instances = [];
    vi.stubGlobal("XMLHttpRequest", FakeXmlHttpRequest);
  });

  afterEach(() => {
    vi.restoreAllMocks();
    vi.unstubAllGlobals();
  });

  it("creates a presigned upload with the selected file metadata", async () => {
    const api = { post: vi.fn().mockResolvedValue(singleUpload) } as unknown as ApiClient;
    const file = new File(["hello"], "hello.txt", { type: "text/plain" });

    await beginFileUpload(api, "space-1", { parentNodeId: "parent-1", name: "note.txt", file });

    expect(api.post).toHaveBeenCalledWith("/api/v1/spaces/space-1/file-uploads", {
      parent_node_id: "parent-1",
      name: "note.txt",
      byte_len: 5,
      media_type: "text/plain",
      original_filename: "hello.txt"
    });
  });

  it("uploads a single PUT and reports progress", async () => {
    const file = new File(["hello"], "hello.txt", { type: "text/plain" });
    const onProgress = vi.fn();
    const pending = transferFile({} as ApiClient, "space-1", singleUpload, file, { onProgress });
    const request = FakeXmlHttpRequest.instances[0];

    request.progress(2, 5);
    request.respond(200);
    await pending;

    expect(request.method).toBe("PUT");
    expect(request.url).toBe(singleUpload.transfer.mode === "single" ? singleUpload.transfer.url : "");
    expect(request.withCredentials).toBe(false);
    expect(request.headers).toEqual(new Map(Object.entries(
      singleUpload.transfer.mode === "single" ? singleUpload.transfer.headers : {}
    )));
    expect(request.body).toBe(file);
    expect(onProgress).toHaveBeenNthCalledWith(1, 2, 5);
    expect(onProgress).toHaveBeenLastCalledWith(5, 5);
  });

  it("reports a failed single PUT as an object upload failure", async () => {
    const pending = transferFile(
      {} as ApiClient,
      "space-1",
      singleUpload,
      new File(["hello"], "hello.txt")
    );

    FakeXmlHttpRequest.instances[0].respond(503);

    await expect(pending).rejects.toMatchObject({
      status: 503,
      kind: "object_upload_failed"
    });
  });

  it("uploads multipart batches with at most four concurrent requests", async () => {
    const upload = multipartUpload(5, 2);
    const api = multipartApi(2, 10);
    const pending = transferFile(api, "space-1", upload, new File(["0123456789"], "large.bin"));

    await vi.waitFor(() => expect(FakeXmlHttpRequest.instances).toHaveLength(4));
    expect(api.post).toHaveBeenCalledWith(
      "/api/v1/spaces/space-1/file-uploads/upload-1/parts",
      { part_numbers: [1, 2, 3, 4, 5] }
    );
    FakeXmlHttpRequest.instances[0].respond(200, '"etag-1"');
    await vi.waitFor(() => expect(FakeXmlHttpRequest.instances).toHaveLength(5));
    for (let index = 1; index < 5; index += 1) {
      FakeXmlHttpRequest.instances[index].respond(200, `"etag-${index + 1}"`);
    }

    await expect(pending).resolves.toEqual([
      { part_number: 1, etag: '"etag-1"' },
      { part_number: 2, etag: '"etag-2"' },
      { part_number: 3, etag: '"etag-3"' },
      { part_number: 4, etag: '"etag-4"' },
      { part_number: 5, etag: '"etag-5"' }
    ]);
    expect((FakeXmlHttpRequest.instances[4].body as Blob).size).toBe(2);
  });

  it("gets a fresh URL and retries only a failed part", async () => {
    const upload = multipartUpload(2, 3);
    const api = multipartApi(3, 6);
    const pending = transferFile(api, "space-1", upload, new File(["abcdef"], "large.bin"));

    await vi.waitFor(() => expect(FakeXmlHttpRequest.instances).toHaveLength(2));
    FakeXmlHttpRequest.instances[0].respond(200, '"etag-1"');
    FakeXmlHttpRequest.instances[1].respond(503);
    await vi.waitFor(() => expect(FakeXmlHttpRequest.instances).toHaveLength(3));
    FakeXmlHttpRequest.instances[2].respond(200, '"etag-2"');

    await expect(pending).resolves.toEqual([
      { part_number: 1, etag: '"etag-1"' },
      { part_number: 2, etag: '"etag-2"' }
    ]);
    expect(api.post).toHaveBeenNthCalledWith(
      2,
      "/api/v1/spaces/space-1/file-uploads/upload-1/parts",
      { part_numbers: [2] }
    );
  });

  it("fails multipart transfer when ETag is not exposed", async () => {
    const upload = multipartUpload(1, 4);
    const api = multipartApi(4, 4);
    const pending = transferFile(api, "space-1", upload, new File(["data"], "large.bin"));

    for (let attempt = 0; attempt < 3; attempt += 1) {
      await vi.waitFor(() => expect(FakeXmlHttpRequest.instances).toHaveLength(attempt + 1));
      FakeXmlHttpRequest.instances[attempt].respond(200);
    }

    await expect(pending).rejects.toMatchObject({ kind: "multipart_etag_missing" });
  });

  it("aborts active object requests when the transfer is canceled", async () => {
    const controller = new AbortController();
    const pending = transferFile(
      {} as ApiClient,
      "space-1",
      singleUpload,
      new File(["data"], "file.bin"),
      { signal: controller.signal }
    );

    controller.abort();

    await expect(pending).rejects.toMatchObject({ name: "AbortError" });
  });

  it("retries only completion after a transient API failure", async () => {
    const api = {
      post: vi.fn()
        .mockRejectedValueOnce(new ApiError("temporary", 503, "object_storage_unavailable"))
        .mockResolvedValueOnce({ node: { id: "node-1" } })
    } as unknown as ApiClient;

    await completeFileUpload(api, "space-1", "upload-1");

    expect(api.post).toHaveBeenCalledTimes(2);
    expect(api.post).toHaveBeenNthCalledWith(1, "/api/v1/spaces/space-1/file-uploads/upload-1/complete");
    expect(api.post).toHaveBeenNthCalledWith(2, "/api/v1/spaces/space-1/file-uploads/upload-1/complete");
  });

  it("sends multipart ETags when completing", async () => {
    const api = { post: vi.fn().mockResolvedValue({ node: { id: "node-1" } }) } as unknown as ApiClient;
    const parts = [{ part_number: 1, etag: '"etag-1"' }];

    await completeFileUpload(api, "space-1", "upload-1", parts);

    expect(api.post).toHaveBeenCalledWith(
      "/api/v1/spaces/space-1/file-uploads/upload-1/complete",
      { completed_parts: parts }
    );
  });

  it("does not retry completion after a permanent API failure", async () => {
    const api = { post: vi.fn().mockRejectedValue(new ApiError("conflict", 409, "conflict")) } as unknown as ApiClient;

    await expect(completeFileUpload(api, "space-1", "upload-1"))
      .rejects.toMatchObject({ status: 409, kind: "conflict" });

    expect(api.post).toHaveBeenCalledTimes(1);
  });

  it("requests the dedicated file preview URL", async () => {
    const response = {
      url: "https://objects.test/preview",
      media_type: "image/png",
      expires_at: "2026-06-13T00:15:00Z"
    };
    const api = { get: vi.fn().mockResolvedValue(response) } as unknown as ApiClient;

    await expect(getFilePreviewUrl(api, "space-1", "file-1")).resolves.toEqual(response);
    expect(api.get).toHaveBeenCalledWith("/api/v1/spaces/space-1/files/file-1/preview-url");
  });

  it("derives preview cache duration from the server expiry with a safety window", () => {
    const cachedAt = Date.parse("2026-06-13T00:00:00Z");

    expect(filePreviewStaleTime("2026-06-13T00:15:00Z", cachedAt)).toBe(14 * 60_000);
    expect(filePreviewStaleTime("invalid", cachedAt)).toBe(0);
    expect(filePreviewStaleTime("2026-06-13T00:00:30Z", cachedAt)).toBe(0);
  });
});

function multipartUpload(partCount: number, partSize: number): BeginFileUploadResponse {
  return {
    upload_id: "upload-1",
    transfer: { mode: "multipart", part_count: partCount, part_size: partSize }
  };
}

function multipartApi(partSize: number, totalSize: number): ApiClient {
  let requestId = 0;
  return {
    post: vi.fn(async (_path: string, body?: unknown) => {
      const numbers = (body as { part_numbers: number[] }).part_numbers;
      return {
        parts: numbers.map((partNumber) => ({
          part_number: partNumber,
          url: `https://objects.test/part-${partNumber}-${requestId++}`,
          headers: {},
          content_length: Math.min(partSize, totalSize - (partNumber - 1) * partSize)
        }))
      };
    })
  } as unknown as ApiClient;
}
