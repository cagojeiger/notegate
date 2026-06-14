import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { ApiProvider } from "../api/ApiProvider";
import type { Me, Space } from "../api/types";
import { SettingsModal } from "./SettingsModal";

const page = { limit: 100, returned: 0, has_more: false, next_cursor: null };

const space: Space = {
  id: "space-1",
  name: "Personal",
  sort_order: 0,
  permission: "write",
  root_node_id: "root-1",
  created_at: "2026-06-13T00:00:00Z",
  updated_at: "2026-06-13T00:00:00Z"
};

function jsonResponse(body: unknown) {
  return Promise.resolve(new Response(JSON.stringify(body), { status: 200, headers: { "content-type": "application/json" } }));
}

const userMe: Me = {
  account: { id: "user-1", kind: "user", display_name: "Kang" },
  user: { email: "kang@example.com" },
  capabilities: { can_create_space: true, can_manage_agents: true }
};

const agentMe: Me = {
  account: { id: "agent-1", kind: "agent", display_name: "ci-bot" },
  agent: { name: "ci-bot" },
  capabilities: { can_create_space: false, can_manage_agents: false }
};

function mockSettingsApi(me: unknown = userMe) {
  vi.spyOn(globalThis, "fetch").mockImplementation((input) => {
    const path = String(input);
    if (path.includes("/api/v1/me/keys")) return jsonResponse({ keys: [], page });
    if (path.includes("/api/v1/agents/agent-1/keys")) return jsonResponse({ keys: [], page });
    if (path.includes("/api/v1/spaces/space-1/agents")) {
      return jsonResponse({ connections: [{ agent: { id: "agent-1", kind: "agent", display_name: "ci-bot" }, permission: "write", connected_at: "2026-06-13T00:00:00Z" }], page });
    }
    if (path.includes("/api/v1/spaces?")) return jsonResponse({ spaces: [space], page });
    if (path.endsWith("/api/v1/agents?limit=100")) return jsonResponse({ agents: [{ id: "agent-1", name: "ci-bot", owner_user_id: "user-1" }], page });
    if (path.includes("/api/v1/me")) {
      return jsonResponse(me);
    }
    return jsonResponse({});
  });
}

function renderSettings(me = userMe) {
  render(
    <ApiProvider apiKey="test-key" authCacheKey="test-key:0">
      <SettingsModal me={me} onClose={vi.fn()} onSignOut={vi.fn()} />
    </ApiProvider>
  );
}

describe("SettingsModal", () => {
  beforeEach(() => {
    vi.restoreAllMocks();
    mockSettingsApi();
  });

  it("keeps user API keys inside the account tab", async () => {
    renderSettings();

    expect(screen.getByRole("heading", { name: "Settings" })).toBeInTheDocument();
    expect(screen.getByText("Appearance")).toBeInTheDocument();
    expect(screen.getByText("My API Keys")).toBeInTheDocument();
    expect(await screen.findByText("No user API keys.")).toBeInTheDocument();
    expect(screen.queryByRole("tab", { name: "API Keys" })).not.toBeInTheDocument();
    expect(screen.queryByRole("tab", { name: "Connections" })).not.toBeInTheDocument();
  });

  it("shows agent keys and space access inside an expanded agent", async () => {
    const user = userEvent.setup();
    renderSettings();

    await user.click(await screen.findByRole("tab", { name: "Agents" }));
    await user.click(await screen.findByRole("button", { name: "Toggle ci-bot details" }));

    expect(screen.getByText("Agent API Keys")).toBeInTheDocument();
    expect(await screen.findByText("No keys for this agent.")).toBeInTheDocument();
    expect(screen.getByText("Space Access")).toBeInTheDocument();
    expect(await screen.findByText("Personal")).toBeInTheDocument();
    expect(await screen.findByText("write")).toBeInTheDocument();
  });

  it("hides agent management for callers without that capability", async () => {
    vi.restoreAllMocks();
    mockSettingsApi(agentMe);

    renderSettings(agentMe);

    expect(await screen.findByText("ci-bot")).toBeInTheDocument();
    expect(screen.queryByRole("tab", { name: "Agents" })).not.toBeInTheDocument();
  });
});
