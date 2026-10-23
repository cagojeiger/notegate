import { afterEach, describe, expect, it, vi } from "vitest";

import { createApiClient } from "./client";

const downloadMocks = vi.hoisted(() => ({ downloadUrl: vi.fn() }));

vi.mock("../shared/lib/downloadUrl", () => downloadMocks);

describe("createApiClient", () => {
  afterEach(() => {
    vi.restoreAllMocks();
  });

  it("uses same-origin cookies without an Authorization header", async () => {
    const fetchMock = vi.spyOn(globalThis, "fetch").mockResolvedValue(new Response(JSON.stringify({ ok: true }), { status: 200 }));
    const client = createApiClient();

    await client.get<{ ok: boolean }>("/api/v1/me");

    const [, init] = fetchMock.mock.calls[0];
    expect((init?.headers as Headers).has("authorization")).toBe(false);
    expect(init?.credentials).toBe("same-origin");
  });

  it("normalizes api errors", async () => {
    vi.spyOn(globalThis, "fetch").mockResolvedValue(
      new Response(JSON.stringify({ error: "forbidden", kind: "forbidden", message: "nope" }), { status: 403 })
    );
    const client = createApiClient();

    await expect(client.get("/api/v1/me")).rejects.toMatchObject({ status: 403, kind: "forbidden", message: "nope" });
  });

  it("starts a browser-native download for cookie sessions", async () => {
    const fetchMock = vi.spyOn(globalThis, "fetch");
    const client = createApiClient();

    await client.download("/api/v1/files/file-1/content", "report.pdf");

    expect(downloadMocks.downloadUrl).toHaveBeenCalledWith("/api/v1/files/file-1/content", "report.pdf");
    expect(fetchMock).not.toHaveBeenCalled();
  });
});
