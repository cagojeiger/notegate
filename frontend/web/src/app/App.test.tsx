import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { App } from "./App";

vi.mock("../layout/AppShell", () => ({
  AppShell: ({ onSignOut }: { onSignOut: () => void }) => <button onClick={onSignOut}>Mock sign out</button>
}));

function meResponse() {
  return {
    account: { id: "acct_1", kind: "user", display_name: "Kang" },
    user: { email: "kang@example.com" },
    capabilities: { can_create_space: true, can_manage_agents: true }
  };
}

describe("App auth boundary", () => {
  beforeEach(() => {
    vi.restoreAllMocks();
    window.history.replaceState(null, "", "/");
  });

  it("checks /api/v1/me on mount with the browser session", async () => {
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
    expect((init?.headers as Headers).has("authorization")).toBe(false);
  });

  it("keeps the initial session check in the shared authentication page until it completes", async () => {
    let completeCheck: (response: Response) => void = () => undefined;
    vi.spyOn(globalThis, "fetch").mockImplementation(() => new Promise<Response>((resolve) => { completeCheck = resolve; }));

    render(<App />);

    expect(screen.getByRole("heading", { name: "Checking session" })).toBeInTheDocument();
    expect(screen.getByRole("main")).toHaveClass("ng-auth-screen");
    expect(screen.getByRole("status")).toHaveAttribute("aria-busy", "true");
    expect(screen.getByRole("link", { name: "Privacy" })).toHaveAttribute("href", "/privacy");
    expect(screen.queryByRole("button", { name: "Continue with Google" })).not.toBeInTheDocument();

    await act(async () => { completeCheck(new Response(JSON.stringify({ error: "unauthorized", kind: "unauthorized", message: "unauthorized" }), { status: 401 })); });

    expect(await screen.findByRole("heading", { name: "Continue to NoteGate" })).toBeInTheDocument();
  });

  it("shows the login gate when the browser session is missing", async () => {
    vi.spyOn(globalThis, "fetch").mockResolvedValue(new Response(JSON.stringify({ error: "unauthorized", kind: "unauthorized", message: "unauthorized" }), { status: 401 }));

    render(<App />);

    await screen.findByText("Continue to NoteGate");
    expect(screen.getByRole("link", { name: "Terms" })).toHaveAttribute("href", "/terms");
    expect(screen.getByRole("link", { name: "Privacy" })).toHaveAttribute("href", "/privacy");
    expect(screen.getByRole("link", { name: "Support" })).toHaveAttribute("href", "/support");
  });

  it.each([
    { path: "/terms", title: "Terms of Service" },
    { path: "/privacy", title: "Privacy Policy" },
    { path: "/support", title: "Help and contact" }
  ])("opens $path without checking or changing the browser session", ({ path, title }) => {
    const fetchMock = vi.spyOn(globalThis, "fetch").mockRejectedValue(new Error("Session service unavailable"));
    window.history.replaceState(null, "", path);

    render(<App />);

    expect(screen.getByRole("heading", { name: title })).toBeInTheDocument();
    expect(screen.getByText("In preparation")).toBeInTheDocument();
    expect(screen.getByRole("link", { name: "Back to NoteGate" })).toHaveAttribute("href", "/");
    expect(fetchMock).not.toHaveBeenCalled();
    expect(screen.queryByRole("checkbox")).not.toBeInTheDocument();
    window.history.replaceState(null, "", "/");
  });

  it("previews agreement without persisting it or starting authentication", () => {
    const fetchMock = vi.spyOn(globalThis, "fetch").mockRejectedValue(new Error("No backend in preview"));
    const storedBefore = [Object.entries(localStorage), Object.entries(sessionStorage)];
    window.history.replaceState(null, "", "/signup/preview");
    const preview = render(<App />);

    const checkbox = screen.getByRole("checkbox", { name: "I agree to the Terms of Service." });
    const continueButton = screen.getByRole("button", { name: "Agree and continue" });
    expect(continueButton).toBeDisabled();
    fireEvent.click(checkbox);
    expect(continueButton).toBeEnabled();
    fireEvent.click(continueButton);
    expect(screen.getByRole("status")).toHaveTextContent("Preview complete. No agreement or account was saved.");
    expect(fetchMock).not.toHaveBeenCalled();
    expect([Object.entries(localStorage), Object.entries(sessionStorage)]).toEqual(storedBefore);

    preview.unmount();
    render(<App />);
    expect(screen.getByRole("checkbox")).not.toBeChecked();
    expect(screen.queryByRole("status")).not.toBeInTheDocument();
    window.history.replaceState(null, "", "/");
  });

  it("keeps a browser session retryable when /me is temporarily unavailable", async () => {
    let completeRetry: (response: Response) => void = () => undefined;
    vi.spyOn(globalThis, "fetch")
      .mockResolvedValueOnce(
        new Response(JSON.stringify({ error: "auth_unavailable", kind: "auth_unavailable", message: "auth service temporarily unavailable" }), {
          status: 503
        })
      )
      .mockImplementationOnce(() => new Promise<Response>((resolve) => { completeRetry = resolve; }))
      .mockResolvedValue(new Response(null, { status: 204 }));

    render(<App />);

    await screen.findByText("Authentication temporarily unavailable");
    expect(screen.queryByText("Continue to NoteGate")).not.toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: "Retry" }));

    await waitFor(() => expect(screen.getByRole("button", { name: "Retry" })).toBeDisabled());
    expect(screen.getByRole("button", { name: "Retry" })).toHaveAttribute("aria-busy", "true");
    expect(screen.getByRole("button", { name: "Retry" })).toHaveTextContent("Retrying…");
    await act(async () => { completeRetry(new Response(JSON.stringify(meResponse()), { status: 200 })); });

    await screen.findByText("Mock sign out");
  });

  it("switches back to the login gate when the workbench signs out", async () => {
    let checks = 0;
    vi.spyOn(globalThis, "fetch").mockImplementation(async (path) => {
      if (path === "/auth/session/activity") return new Response(null, { status: 204 });
      checks += 1;
      return checks === 1
        ? new Response(JSON.stringify(meResponse()), { status: 200 })
        : new Response(JSON.stringify({ error: "unauthorized", kind: "unauthorized", message: "unauthorized" }), { status: 401 });
    });

    render(<App />);

    fireEvent.click(await screen.findByText("Mock sign out"));

    await screen.findByText("Continue to NoteGate");
  });
});
