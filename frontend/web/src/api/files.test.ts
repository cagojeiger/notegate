import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import type { ApiClient } from "./client";
import { ApiError } from "./errors";
import { beginFileUpload, completeFileUpload, transferFile } from "./files";
import type { BeginFileUploadResponse } from "./types";

const upload: BeginFileUploadResponse = {
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

  send(body: XMLHttpRequestBodyInit | null) {
    this.body = body;
  }

  abort() {
    this.onabort?.();
  }

  progress(loaded: number, total: number) {
    this.upload.onprogress?.({ lengthComputable: true, loaded, total } as ProgressEvent);
  }

  respond(status: number) {
    this.status = status;
    this.onload?.();
  }

  fail() {
    this.onerror?.();
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
    const api = { post: vi.fn().mockResolvedValue(upload) } as unknown as ApiClient;
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

  it("uploads through the presigned request and reports progress", async () => {
    const file = new File(["hello"], "hello.txt", { type: "text/plain" });
    const onProgress = vi.fn();
    const pending = transferFile(upload, file, { onProgress });
    const request = FakeXmlHttpRequest.instances[0];

    request.progress(2, 5);
    request.respond(200);
    await pending;

    expect(request.method).toBe("PUT");
    expect(request.url).toBe(upload.transfer.url);
    expect(request.withCredentials).toBe(false);
    expect(request.headers).toEqual(new Map(Object.entries(upload.transfer.headers)));
    expect(request.body).toBe(file);
    expect(onProgress).toHaveBeenNthCalledWith(1, 2, 5);
    expect(onProgress).toHaveBeenLastCalledWith(5, 5);
  });

  it("maps object transfer failures to an API error", async () => {
    const pending = transferFile(upload, new File(["data"], "file.bin"));

    FakeXmlHttpRequest.instances[0].respond(503);

    await expect(pending).rejects.toMatchObject({ status: 503, kind: "object_upload_failed" });
  });

  it("aborts the object request when the transfer is canceled", async () => {
    const controller = new AbortController();
    const pending = transferFile(upload, new File(["data"], "file.bin"), { signal: controller.signal });

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

  it("does not retry completion after a permanent API failure", async () => {
    const api = { post: vi.fn().mockRejectedValue(new ApiError("conflict", 409, "conflict")) } as unknown as ApiClient;

    await expect(completeFileUpload(api, "space-1", "upload-1"))
      .rejects.toMatchObject({ status: 409, kind: "conflict" });

    expect(api.post).toHaveBeenCalledTimes(1);
  });
});
