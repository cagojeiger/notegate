import { afterEach, describe, expect, it, vi } from "vitest";

import { createApiClient } from "./client";
import { ApiError } from "./errors";

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
});
