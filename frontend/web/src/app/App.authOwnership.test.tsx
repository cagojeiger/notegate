import { useQueryClient, type QueryClient } from "@tanstack/react-query";
import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { StrictMode, useLayoutEffect } from "react";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { ApiError } from "../api/errors";
import type { MintedKey } from "../api/keys";
import type { Me } from "../api/types";
import { useCreateApiKeyMutation } from "../features/settings/useSettingsQueries";
import { App } from "./App";

const mocks = vi.hoisted(() => ({
  create: vi.fn(),
  clients: new Map<string, QueryClient>(),
  commits: [] as { owner: string; tokens: unknown[] }[]
}));

vi.mock("./AuthenticatedWorkspace", () => ({
  default: function OwnershipWorkspace({ me, onSignOut }: { me: Me; onSignOut: () => void }) {
    const client = useQueryClient();
    const issuance = useCreateApiKeyMutation(["agents", "agent-a", "keys"], mocks.create);
    useLayoutEffect(() => {
      mocks.clients.set(me.account.id, client);
      mocks.commits.push({ owner: me.account.id, tokens: client.getMutationCache().getAll()
        .map((item) => (item.state.data as MintedKey | undefined)?.token).filter(Boolean) });
    });
    return <>
      <p>Owner {me.account.id}</p>
      <button onClick={() => issuance.mutate({ name: "Private key", expires_at: "2026-11-22T00:00:00Z" })}>Mint key</button>
      {issuance.data ? <code>{issuance.data.token}</code> : null}
      <button onClick={onSignOut}>Sign out</button>
    </>;
  }
}));

const minted: MintedKey = {
  id: "key-a", name: "Private key", token: "dummy-owner-a-token",
  created_at: "2026-10-23T00:00:00Z", expires_at: "2026-11-22T00:00:00Z"
};
let owner: string | null;

function currentClient() {
  return mocks.clients.get(owner!)!;
}

async function refreshOwner(client: QueryClient, nextOwner: string | null) {
  owner = nextOwner;
  await act(async () => { await client.invalidateQueries({ queryKey: ["me"] }); });
}

describe("authenticated cache ownership", () => {
  beforeEach(() => {
    owner = "a";
    mocks.clients.clear();
    mocks.commits.length = 0;
    mocks.create.mockReset().mockResolvedValue(minted);
    vi.spyOn(globalThis, "fetch").mockImplementation(async (path) => {
      if (path === "/auth/session/activity") return new Response(null, { status: 204 });
      if (path !== "/api/v1/me") throw new Error(`Unhandled request: ${String(path)}`);
      return new Response(JSON.stringify(owner ? {
        account: { id: owner, kind: "user", display_name: owner },
        user: { email: `${owner}@example.invalid` },
        capabilities: { can_create_space: true, can_manage_agents: true }
      } : { message: "Unauthorized", kind: "unauthorized" }), { status: owner ? 200 : 401 });
    });
  });

  it("keeps the same owner's retained token through a successful session refresh", async () => {
    render(<StrictMode><App /></StrictMode>);
    fireEvent.click(await screen.findByRole("button", { name: "Mint key" }));
    await screen.findByText(minted.token);
    const previous = currentClient();
    await refreshOwner(previous, "a");
    expect(screen.getByText(minted.token)).toBeInTheDocument();
    expect(currentClient()).toBe(previous);
  });

  it("replaces the cache before committing protected UI for a different successful /me owner", async () => {
    render(<StrictMode><App /></StrictMode>);
    fireEvent.click(await screen.findByRole("button", { name: "Mint key" }));
    await screen.findByText(minted.token);
    const previous = currentClient();
    await refreshOwner(previous, "b");
    await screen.findByText("Owner b");

    expect(currentClient()).not.toBe(previous);
    expect(screen.queryByText(minted.token)).not.toBeInTheDocument();
    expect(mocks.commits.filter((item) => item.owner === "b").every((item) => item.tokens.length === 0)).toBe(true);
    expect(previous.getMutationCache().getAll()).toHaveLength(0);
  });

  it.each(["success", "unauthorized"] as const)("isolates a previous owner's late mint %s from the new cache", async (outcome) => {
    let finish!: (key: MintedKey) => void;
    let fail!: (error: Error) => void;
    mocks.create.mockImplementationOnce(() => new Promise<MintedKey>((resolve, reject) => { finish = resolve; fail = reject; }));
    render(<App />);
    fireEvent.click(await screen.findByRole("button", { name: "Mint key" }));
    await waitFor(() => expect(mocks.create).toHaveBeenCalledTimes(1));
    const previous = currentClient();
    await refreshOwner(previous, "b");
    await screen.findByText("Owner b");
    const next = currentClient();
    await act(async () => {
      if (outcome === "success") finish(minted);
      else fail(new ApiError("Old session expired", 401));
    });

    expect(currentClient()).toBe(next);
    expect(next).not.toBe(previous);
    expect(screen.queryByText(minted.token)).not.toBeInTheDocument();
    expect(next.getMutationCache().getAll()).toHaveLength(0);
    expect(previous.getMutationCache().getAll()).toHaveLength(0);
  });

  it.each(["sign out", "session 401"])("discards retained tokens after %s", async (transition) => {
    render(<App />);
    fireEvent.click(await screen.findByRole("button", { name: "Mint key" }));
    await screen.findByText(minted.token);
    const previous = currentClient();
    if (transition === "sign out") {
      owner = null;
      fireEvent.click(screen.getByRole("button", { name: "Sign out" }));
    } else await refreshOwner(previous, null);
    await screen.findByText("Continue to NoteGate");
    expect(screen.queryByText(minted.token)).not.toBeInTheDocument();
    expect(previous.getMutationCache().getAll()).toHaveLength(0);
  });
});
