import { useQueryClient, type QueryClient } from "@tanstack/react-query";
import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { useLayoutEffect } from "react";
import { expect, it, vi } from "vitest";

import type { Me } from "../api/types";
import { AuthAccessManager } from "../features/settings/AuthAccessManager";
import { App } from "./App";

const probes = vi.hoisted(() => ({ clients: new Map<string, QueryClient>(), signOut: vi.fn() }));
vi.mock("./AuthenticatedWorkspace", () => ({
  default: function SessionWorkspace({ me, onSignOut }: { me: Me; onSignOut: () => void }) {
    const client = useQueryClient();
    useLayoutEffect(() => { probes.clients.set(me.account.id, client); });
    return <>
      <p>Owner {me.account.id}</p>
      <AuthAccessManager kind="browser" onSignOut={() => { probes.signOut(); onSignOut(); }} />
    </>;
  }
}));

it("does not sign out the new owner when the previous owner's current-session revocation finishes", async () => {
  let owner = "a";
  let finish!: () => void;
  const pending = new Promise<void>((resolve) => { finish = resolve; });
  let deleteStarted = false;
  const dates = { created_at: "2026-10-22T00:00:00Z", expires_at: "2026-11-22T00:00:00Z", idle_expires_at: "2026-11-10T00:00:00Z" };
  vi.spyOn(globalThis, "fetch").mockImplementation(async (input, init) => {
    const path = String(input);
    if (path === "/api/v1/me/sessions/session-a" && init?.method === "DELETE") {
      deleteStarted = true;
      await pending;
      return new Response(null, { status: 204 });
    }
    if (path === "/api/v1/me/sessions") return new Response(JSON.stringify({ sessions: [{
      ...dates, id: `session-${owner}`, current: true, client_environment: `Browser ${owner}`, last_used_at: null
    }] }));
    if (path === "/auth/session/activity") return new Response(null, { status: 204 });
    if (path === "/api/v1/me") return new Response(JSON.stringify({
      account: { id: owner, kind: "user", display_name: owner }, user: { email: `${owner}@example.invalid` },
      capabilities: { can_create_space: true, can_manage_agents: true }
    }));
    throw new Error(`Unhandled ${init?.method} ${path}`);
  });
  render(<App />);
  fireEvent.click(await screen.findByRole("button", { name: "Sign out Browser a (this session)" }));
  fireEvent.click(screen.getByRole("button", { name: "Confirm" }));
  await waitFor(() => expect(deleteStarted).toBe(true));
  const previous = probes.clients.get("a")!;
  const operation = previous.getMutationCache().getAll()[0];
  owner = "b";
  await act(async () => { await previous.invalidateQueries({ queryKey: ["me"] }); });
  await screen.findByText("Owner b");
  const current = probes.clients.get("b")!;
  expect(current).not.toBe(previous);
  expect(previous.getMutationCache().getAll()).toHaveLength(0);
  await act(async () => { finish(); await pending; });
  await waitFor(() => expect(operation.state.status).toBe("success"));

  expect(probes.signOut).not.toHaveBeenCalled();
  expect(probes.clients.get("b")).toBe(current);
  expect(screen.getByText("Owner b")).toBeInTheDocument();
});
