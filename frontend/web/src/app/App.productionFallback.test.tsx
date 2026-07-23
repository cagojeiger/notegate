import { render, waitFor } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

describe("App production API-key fallback", () => {
  afterEach(() => {
    vi.restoreAllMocks();
    vi.unstubAllEnvs();
    window.sessionStorage.clear();
  });

  it("honors the explicit production fallback opt-in", async () => {
    vi.stubEnv("DEV", false);
    vi.stubEnv("MODE", "production");
    vi.stubEnv("VITE_NOTEGATE_ENABLE_DEV_API_KEY", "true");
    window.sessionStorage.setItem("notegate.devApiKey", "ngk_v1_test");
    const fetchMock = vi.spyOn(globalThis, "fetch").mockResolvedValue(
      new Response(JSON.stringify({ error: "unauthorized", kind: "unauthorized", message: "unauthorized" }), { status: 401 })
    );
    const { App } = await import("./App");

    render(<App />);

    await waitFor(() => expect(fetchMock).toHaveBeenCalled());
    const [, init] = fetchMock.mock.calls[0];
    expect((init?.headers as Headers).get("authorization")).toBe("Bearer ngk_v1_test");
  });
});
