import { render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, describe, expect, it, vi } from "vitest";

import { ApiProvider } from "../../api/ApiProvider";
import { AuthAccessManager } from "./AuthAccessManager";

const dates = { created_at: "2026-10-19T01:00:00Z", idle_expires_at: "2026-11-02T01:00:00Z", expires_at: "2026-11-18T01:00:00Z" };

function setup(fail = false) {
  let failed = fail;
  const sessions = [
    { ...dates, id: "current", current: true, client_environment: "Chrome on macOS", last_used_at: null },
    { ...dates, id: "remote", current: false, client_environment: "Chrome on Android", last_used_at: dates.created_at }
  ];
  let grants = [
    { ...dates, id: "cli", client_name: "NoteGate CLI", kind: "cli", client_environment: "NoteGate CLI on Linux", last_token_at: dates.created_at, offline_access: true },
    { ...dates, id: "mcp", client_name: "Remote MCP", kind: "mcp", client_environment: null, last_token_at: null, offline_access: false }
  ];
  const requests: string[] = [];
  vi.spyOn(globalThis, "fetch").mockImplementation(async (input, init) => {
    const path = String(input);
    if (init?.method === "DELETE") {
      requests.push(path);
      if (failed) return new Response(JSON.stringify({ error: { code: "unavailable", message: "Unavailable" } }), { status: 503 });
      if (path.endsWith("/oauth-grants/mcp")) grants = grants.filter((grant) => grant.id !== "mcp");
      const kind = new URL(path, "https://preview.invalid").searchParams.get("kind");
      if (kind) grants = grants.filter((grant) => grant.kind !== kind);
      return new Response(null, { status: 204 });
    }
    const body = path.endsWith("/sessions") ? { sessions } : { grants };
    return new Response(JSON.stringify(body), { headers: { "content-type": "application/json" } });
  });
  const onSignOut = vi.fn();
  render(<ApiProvider authCacheKey="test"><AuthAccessManager onSignOut={onSignOut} /></ApiProvider>);
  return { user: userEvent.setup(), requests, onSignOut, recover: () => { failed = false; } };
}

afterEach(() => vi.restoreAllMocks());

describe("AuthAccessManager", () => {
  it("shows independently managed browser, CLI and MCP access without inventing device information", async () => {
    const { user, requests } = setup();
    expect(await screen.findByText("Chrome on macOS · This session")).toBeInTheDocument();
    expect(await screen.findByText("NoteGate CLI on Linux")).toBeInTheDocument();
    expect(screen.getByText("Execution environment not recorded")).toBeInTheDocument();
    expect(screen.getByText(/Access & refresh tokens/)).toBeInTheDocument();
    expect(screen.getByText(/Access token only/)).toBeInTheDocument();
    const cli = screen.getByRole("region", { name: "CLI connections" });
    const mcp = screen.getByRole("region", { name: "MCP connections" });
    expect(within(cli).getByText("NoteGate CLI · CLI")).toBeInTheDocument();
    expect(within(cli).queryByText("Remote MCP · MCP")).not.toBeInTheDocument();
    expect(within(mcp).getByText("Remote MCP · MCP")).toBeInTheDocument();
    expect(within(mcp).queryByText("NoteGate CLI · CLI")).not.toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Revoke Remote MCP" }));
    expect(requests).toEqual([]);
    const confirmation = within(mcp).getByRole("group", { name: "Confirm revocation" });
    expect(confirmation).toHaveFocus();
    expect(screen.getByText("Remote MCP · MCP").closest("li")).toContainElement(confirmation);
    await user.click(screen.getByRole("button", { name: "Cancel" }));
    expect(requests).toEqual([]);
  });

  it.each([
    ["Sign out Chrome on Android", "/api/v1/me/sessions/remote", false],
    ["Sign out Chrome on macOS (this session)", "/api/v1/me/sessions/current", true],
    ["Sign out other browsers", "/api/v1/me/sessions/others", false],
    ["Sign out all browsers", "/api/v1/me/sessions", true],
    ["Revoke NoteGate CLI", "/api/v1/me/oauth-grants/cli", false],
    ["Revoke Remote MCP", "/api/v1/me/oauth-grants/mcp", false],
    ["Revoke all CLI connections", "/api/v1/me/oauth-grants?kind=cli", false],
    ["Revoke all MCP connections", "/api/v1/me/oauth-grants?kind=mcp", false]
  ] as const)("confirms %s with the correct scope", async (label, path, signsOut) => {
    const { user, requests, onSignOut } = setup();
    await user.click(await screen.findByRole("button", { name: label }));
    expect(requests).toEqual([]);
    await user.click(screen.getByRole("button", { name: "Confirm" }));
    await waitFor(() => expect(requests).toEqual([path]));
    await waitFor(() => expect(screen.queryByRole("button", { name: "Confirm" })).not.toBeInTheDocument());
    expect(onSignOut).toHaveBeenCalledTimes(signsOut ? 1 : 0);
  });

  it.each(["CLI", "MCP"])("bulk revocation of %s preserves the other connection list", async (kind) => {
    const { user, onSignOut } = setup();
    await user.click(await screen.findByRole("button", { name: `Revoke all ${kind} connections` }));
    await user.click(screen.getByRole("button", { name: "Confirm" }));
    expect(await screen.findByText(`No active ${kind} connections.`)).toBeInTheDocument();
    expect(screen.getByText(kind === "CLI" ? "Remote MCP · MCP" : "NoteGate CLI · CLI")).toBeInTheDocument();
    expect(onSignOut).not.toHaveBeenCalled();
  });

  it("preserves the list after a failed revocation and allows an explicit retry", async () => {
    const { user, onSignOut, recover } = setup(true);
    await user.click(await screen.findByRole("button", { name: "Revoke Remote MCP" }));
    await user.click(screen.getByRole("button", { name: "Confirm" }));
    expect(await screen.findByRole("alert")).toHaveTextContent("Could not revoke access");
    expect(screen.getByText("Remote MCP · MCP")).toBeInTheDocument();
    expect(onSignOut).not.toHaveBeenCalled();
    recover();
    await user.click(screen.getByRole("button", { name: "Confirm" }));
    await waitFor(() => expect(screen.queryByText("Remote MCP · MCP")).not.toBeInTheDocument());
    expect(onSignOut).not.toHaveBeenCalled();
  });
});
