import { afterEach, describe, expect, it, vi } from "vitest";

import type { ApiClient } from "./client";
import { ApiError } from "./errors";
import { uploadFile } from "./files";

function client() {
  return {
    post: vi
      .fn()
      .mockResolvedValueOnce({
        upload_id: "upload-1",
        transfer: {
          mode: "single",
          url: "https://objects.test/notegate/upload-1",
          headers: { "content-type": "text/plain", "if-none-match": "*" }
        }
      })
      .mockResolvedValueOnce({ node: { id: "node-1" } })
  } as unknown as ApiClient;
}

describe("files api", () => {
  afterEach(() => vi.restoreAllMocks());

  it("uploads every file through the presigned object flow", async () => {
    const api = client();
    const file = new File(["hello"], "hello.txt", { type: "text/plain" });
    const fetchMock = vi.spyOn(globalThis, "fetch").mockResolvedValue(new Response(null, { status: 200 }));

    await uploadFile(api, "space-1", { parentNodeId: "parent-1", name: "hello.txt", file });

    expect(api.post).toHaveBeenNthCalledWith(1, "/api/v1/spaces/space-1/file-uploads", {
      parent_node_id: "parent-1",
      name: "hello.txt",
      byte_len: 5,
      media_type: "text/plain",
      original_filename: "hello.txt"
    });
    expect(fetchMock).toHaveBeenCalledWith("https://objects.test/notegate/upload-1", {
      method: "PUT",
      headers: { "content-type": "text/plain", "if-none-match": "*" },
      credentials: "omit",
      body: file
    });
    expect(api.post).toHaveBeenNthCalledWith(2, "/api/v1/spaces/space-1/file-uploads/upload-1/complete");
  });

  it("does not complete an upload when the object transfer fails", async () => {
    const api = client();
    vi.spyOn(globalThis, "fetch").mockResolvedValue(new Response(null, { status: 503 }));

    await expect(uploadFile(api, "space-1", {
      parentNodeId: "parent-1",
      name: "empty.bin",
      file: new File([], "empty.bin")
    })).rejects.toMatchObject({ status: 503, kind: "object_upload_failed" });

    expect(api.post).toHaveBeenCalledTimes(1);
  });

  it("retries only completion after a transient API failure", async () => {
    const api = client();
    vi.mocked(api.post)
      .mockReset()
      .mockResolvedValueOnce({
        upload_id: "upload-1",
        transfer: { mode: "single", url: "https://objects.test/upload-1", headers: {} }
      })
      .mockRejectedValueOnce(new ApiError("temporary", 503, "object_storage_unavailable"))
      .mockResolvedValueOnce({ node: { id: "node-1" } });
    const fetchMock = vi.spyOn(globalThis, "fetch").mockResolvedValue(new Response(null, { status: 200 }));

    await uploadFile(api, "space-1", {
      parentNodeId: "parent-1",
      name: "file.bin",
      file: new File(["data"], "file.bin")
    });

    expect(fetchMock).toHaveBeenCalledTimes(1);
    expect(api.post).toHaveBeenCalledTimes(3);
    expect(api.post).toHaveBeenNthCalledWith(2, "/api/v1/spaces/space-1/file-uploads/upload-1/complete");
    expect(api.post).toHaveBeenNthCalledWith(3, "/api/v1/spaces/space-1/file-uploads/upload-1/complete");
  });

  it("does not retry completion after a permanent API failure", async () => {
    const api = client();
    vi.mocked(api.post)
      .mockReset()
      .mockResolvedValueOnce({
        upload_id: "upload-1",
        transfer: { mode: "single", url: "https://objects.test/upload-1", headers: {} }
      })
      .mockRejectedValueOnce(new ApiError("conflict", 409, "conflict"));
    vi.spyOn(globalThis, "fetch").mockResolvedValue(new Response(null, { status: 200 }));

    await expect(uploadFile(api, "space-1", {
      parentNodeId: "parent-1",
      name: "file.bin",
      file: new File(["data"], "file.bin")
    })).rejects.toMatchObject({ status: 409, kind: "conflict" });

    expect(api.post).toHaveBeenCalledTimes(2);
  });
});
