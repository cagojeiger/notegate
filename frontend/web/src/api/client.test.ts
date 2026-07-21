import { afterEach, describe, expect, it, vi } from "vitest";

import { createApiClient } from "./client";

const downloadMocks = vi.hoisted(() => ({
  downloadBlob: vi.fn(),
  downloadUrl: vi.fn()
}));

vi.mock("../shared/lib/downloadBlob", () => downloadMocks);

describe("createApiClient", () => {
  afterEach(() => {
    vi.restoreAllMocks();
  });

  it("sends bearer credentials and same-origin cookies", async () => {
    const fetchMock = vi.spyOn(globalThis, "fetch").mockResolvedValue(new Response(JSON.stringify({ ok: true }), { status: 200 }));
    const client = createApiClient(() => "user-key");

    await client.get<{ ok: boolean }>("/api/v1/me");

    const [, init] = fetchMock.mock.calls[0];
    expect((init?.headers as Headers).get("authorization")).toBe("Bearer user-key");
    expect(init?.credentials).toBe("same-origin");
  });

  it("normalizes api errors", async () => {
    vi.spyOn(globalThis, "fetch").mockResolvedValue(
      new Response(JSON.stringify({ error: "forbidden", kind: "forbidden", message: "nope" }), { status: 403 })
    );
    const client = createApiClient(() => "user-key");

    await expect(client.get("/api/v1/me")).rejects.toMatchObject({ status: 403, kind: "forbidden", message: "nope" });
  });

  it("starts a browser-native download for cookie sessions", async () => {
    const fetchMock = vi.spyOn(globalThis, "fetch");
    const client = createApiClient(() => null);

    await client.download("/api/v1/files/file-1/content", "report.pdf");

    expect(downloadMocks.downloadUrl).toHaveBeenCalledWith("/api/v1/files/file-1/content", "report.pdf");
    expect(fetchMock).not.toHaveBeenCalled();
  });

  it("keeps the authenticated Blob fallback for development API keys", async () => {
    const blob = new Blob(["file"]);
    const fetchMock = vi.spyOn(globalThis, "fetch").mockResolvedValue({
      ok: true,
      status: 200,
      blob: vi.fn().mockResolvedValue(blob)
    } as unknown as Response);
    const client = createApiClient(() => "user-key");

    await client.download("/api/v1/files/file-1/content", "report.pdf");

    const [, init] = fetchMock.mock.calls[0];
    expect((init?.headers as Headers).get("authorization")).toBe("Bearer user-key");
    expect(downloadMocks.downloadBlob).toHaveBeenCalledWith(blob, "report.pdf");
    expect(downloadMocks.downloadUrl).not.toHaveBeenCalled();
  });
});
