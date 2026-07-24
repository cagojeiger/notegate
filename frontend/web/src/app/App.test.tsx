import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

import type { RestNode } from "../api/types";
import { useUiStore } from "../stores/uiStore";
import { persistLastActiveSpace, persistSpaceWorkbench, workbenchSpaceKey } from "../stores/workbenchStorage";
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
    useUiStore.setState(useUiStore.getInitialState(), true);
  });

  it("checks /api/v1/me on mount with the stored API key", async () => {
    window.sessionStorage.setItem("notegate.devApiKey", "ngk_v1_test");
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
    expect((init?.headers as Headers).get("authorization")).toBe("Bearer ngk_v1_test");
  });

  it("shows the login gate and clears a stored API key when /me returns 401", async () => {
    window.sessionStorage.setItem("notegate.devApiKey", "expired_key");
    seedAuthenticatedWorkbench();
    vi.spyOn(globalThis, "fetch").mockResolvedValue(new Response(JSON.stringify({ error: "unauthorized", kind: "unauthorized", message: "unauthorized" }), { status: 401 }));

    render(<App />);

    await screen.findByText("Continue to NoteGate");
    await waitFor(() => expect(window.sessionStorage.getItem("notegate.devApiKey")).toBeNull());
    expect(useUiStore.getState().activeSpaceId).toBeNull();
    expect(useUiStore.getState().editorGroups[0]?.node).toBeNull();
    expect(window.localStorage.getItem(workbenchSpaceKey("space-1"))).toBeNull();
  });

  it("clears workbench state when a browser session is unauthorized", async () => {
    seedAuthenticatedWorkbench();
    vi.spyOn(globalThis, "fetch").mockResolvedValue(new Response(JSON.stringify({ error: "unauthorized", kind: "unauthorized", message: "unauthorized" }), { status: 401 }));

    render(<App />);

    await screen.findByText("Continue to NoteGate");
    await waitFor(() => expect(useUiStore.getState().activeSpaceId).toBeNull());
    expect(useUiStore.getState().editorGroups[0]?.node).toBeNull();
    expect(window.localStorage.getItem(workbenchSpaceKey("space-1"))).toBeNull();
  });

  it("keeps a browser session retryable when /me is temporarily unavailable", async () => {
    seedAuthenticatedWorkbench();
    vi.spyOn(globalThis, "fetch")
      .mockResolvedValueOnce(
        new Response(JSON.stringify({ error: "auth_unavailable", kind: "auth_unavailable", message: "auth service temporarily unavailable" }), {
          status: 503
        })
      )
      .mockResolvedValue(new Response(JSON.stringify(meResponse()), { status: 200 }));

    render(<App />);

    await screen.findByText("Authentication temporarily unavailable");
    expect(screen.queryByText("Continue to NoteGate")).not.toBeInTheDocument();
    expect(useUiStore.getState().activeSpaceId).toBe("space-1");
    expect(window.localStorage.getItem(workbenchSpaceKey("space-1"))).not.toBeNull();

    fireEvent.click(screen.getByRole("button", { name: "Retry" }));

    await screen.findByText("Mock sign out");
  });

  it("switches back to the login gate when the workbench signs out", async () => {
    seedAuthenticatedWorkbench();
    vi.spyOn(globalThis, "fetch").mockResolvedValue(new Response(JSON.stringify(meResponse()), { status: 200 }));

    render(<App />);

    fireEvent.click(await screen.findByText("Mock sign out"));

    await screen.findByText("Continue to NoteGate");
    expect(useUiStore.getState().activeSpaceId).toBeNull();
    expect(useUiStore.getState().editorGroups[0]?.node).toBeNull();
    expect(window.localStorage.getItem(workbenchSpaceKey("space-1"))).toBeNull();
  });
});

function seedAuthenticatedWorkbench() {
  const openedNode = node();
  useUiStore.setState({
    activeSpaceId: "space-1",
    editorGroups: [{ id: 4, node: openedNode, mode: "preview" }],
    activeGroupIndex: 0,
    nextGroupId: 5
  });
  persistSpaceWorkbench("space-1", [{ id: 4, node: openedNode, mode: "preview" }], 0);
  persistLastActiveSpace("space-1");
}

function node(): RestNode {
  return {
    id: "node-1",
    space_id: "space-1",
    parent_id: "space-1-root",
    name: "private.md",
    kind: "text",
    path: "/private.md",
    sort_order: 0,
    metadata: {},
    has_children: false,
    created_by: { id: "user-1", kind: "user", display_name: "User" },
    updated_by: { id: "user-1", kind: "user", display_name: "User" },
    created_at: "2026-07-24T00:00:00Z",
    updated_at: "2026-07-24T00:00:00Z"
  };
}
