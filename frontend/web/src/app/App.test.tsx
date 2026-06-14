import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { App } from "./App";

vi.mock("../layout/AppShell", () => ({
  AppShell: ({ onSignOut }: { onSignOut: () => void }) => <button onClick={onSignOut}>Mock sign out</button>
}));

function meResponse() {
  return {
    account: { id: "acct_1", kind: "user", display_name: "Kang" },
    user: { id: "user_1", email: "kang@example.com" }
  };
}

describe("App auth boundary", () => {
  beforeEach(() => {
    vi.restoreAllMocks();
  });

  it("checks /api/v1/me on mount with the stored API key", async () => {
    window.sessionStorage.setItem("notegate.devApiKey", "ng_user_test");
    const fetchMock = vi.spyOn(globalThis, "fetch").mockResolvedValue(new Response(JSON.stringify(meResponse()), { status: 200 }));

    render(<App />);

    await screen.findByText("Mock sign out");
    expect(fetchMock).toHaveBeenCalledWith(
      "/api/v1/me",
      expect.objectContaining({
        method: "GET",
        credentials: "same-origin",
        headers: expect.any(Headers)
      })
    );
    const [, init] = fetchMock.mock.calls[0];
    expect((init?.headers as Headers).get("authorization")).toBe("Bearer ng_user_test");
  });

  it("shows the login gate and clears a stored API key when /me returns 401", async () => {
    window.sessionStorage.setItem("notegate.devApiKey", "expired_key");
    vi.spyOn(globalThis, "fetch").mockResolvedValue(new Response(JSON.stringify({ error: { message: "unauthorized" } }), { status: 401 }));

    render(<App />);

    await screen.findByText("Sign in to Notegate");
    await waitFor(() => expect(window.sessionStorage.getItem("notegate.devApiKey")).toBeNull());
  });

  it("switches back to the login gate when the workbench signs out", async () => {
    vi.spyOn(globalThis, "fetch").mockResolvedValue(new Response(JSON.stringify(meResponse()), { status: 200 }));

    render(<App />);

    fireEvent.click(await screen.findByText("Mock sign out"));

    await screen.findByText("Sign in to Notegate");
  });
});
