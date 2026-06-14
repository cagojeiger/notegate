import { afterEach, describe, expect, it, vi } from "vitest";

import { logout } from "./auth";

describe("auth api", () => {
  afterEach(() => {
    vi.restoreAllMocks();
  });

  it("logs out through the browser session endpoint", async () => {
    const fetchMock = vi.spyOn(globalThis, "fetch").mockResolvedValue(new Response(null, { status: 204 }));

    await logout();

    expect(fetchMock).toHaveBeenCalledWith("/auth/logout", {
      method: "POST",
      credentials: "same-origin",
    });
  });
});
