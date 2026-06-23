import { render, screen, waitFor } from "@testing-library/react";
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

type SpacePermission = "read" | "write" | "none";

function mockSettingsApi(me: unknown = userMe, options: { failPermissionUpdate?: boolean; initialSpacePermission?: SpacePermission } = {}) {
  let spacePermission: SpacePermission = options.initialSpacePermission ?? "write";
  vi.spyOn(globalThis, "fetch").mockImplementation((input, init) => {
    const path = String(input);
    if (path.includes("/api/v1/me/keys")) return jsonResponse({ keys: [], page });
    if (path.includes("/api/v1/agents/agent-1/keys")) return jsonResponse({ keys: [], page });
    if (path.includes("/api/v1/spaces/space-1/agents/agent-1")) {
      if (init?.method === "PUT") {
        if (options.failPermissionUpdate) {
          return Promise.resolve(new Response(JSON.stringify({ error: "update failed" }), { status: 500, headers: { "content-type": "application/json" } }));
        }
        const body = JSON.parse(String(init.body)) as { permission: "read" | "write" };
        spacePermission = body.permission;
        return jsonResponse({ agent: { id: "agent-1", kind: "agent", display_name: "ci-bot" }, permission: spacePermission, connected_at: "2026-06-13T00:00:00Z" });
      }
      if (init?.method === "DELETE") {
        if (options.failPermissionUpdate) {
          return Promise.resolve(new Response(JSON.stringify({ error: "update failed" }), { status: 500, headers: { "content-type": "application/json" } }));
        }
        spacePermission = "none";
        return Promise.resolve(new Response(null, { status: 204 }));
      }
    }
    if (path.includes("/api/v1/spaces/space-1/agents")) {
      const connections = spacePermission === "none"
        ? []
        : [{ agent: { id: "agent-1", kind: "agent", display_name: "ci-bot" }, permission: spacePermission, connected_at: "2026-06-13T00:00:00Z" }];
      return jsonResponse({ connections, page });
    }
    if (path.includes("/api/v1/spaces?")) return jsonResponse({ spaces: [space], page });
    if (path.endsWith("/api/v1/agents?limit=100")) return jsonResponse({ agents: [{ id: "agent-1", name: "ci-bot", owner_user_id: "user-1" }], page });
    if (path.includes("/api/v1/me")) {
      return jsonResponse(me);
    }
    return jsonResponse({});
  });
}

function renderSettings(me = userMe, onResetSavedWorkspace = vi.fn()) {
  render(
    <ApiProvider apiKey="test-key" authCacheKey="test-key:0">
      <SettingsModal me={me} onClose={vi.fn()} onSignOut={vi.fn()} onResetSavedWorkspace={onResetSavedWorkspace} />
    </ApiProvider>
  );
}

describe("SettingsModal", () => {
  beforeEach(() => {
    vi.restoreAllMocks();
    mockSettingsApi();
  });

  it("keeps user API keys inside the account tab", async () => {
    const user = userEvent.setup();
    renderSettings();

    expect(screen.getByRole("heading", { name: "Settings" })).toBeInTheDocument();
    await user.click(screen.getByRole("tab", { name: "Account" }));

    expect(screen.getByText("Appearance")).toBeInTheDocument();
    expect(screen.getByText("My API Keys")).toBeInTheDocument();
    expect(await screen.findByText("No user API keys.")).toBeInTheDocument();
    expect(screen.queryByRole("tab", { name: "API Keys" })).not.toBeInTheDocument();
    expect(screen.queryByRole("tab", { name: "Connections" })).not.toBeInTheDocument();
  });

  it("shows browser workspace reset controls in the general tab", async () => {
    const user = userEvent.setup();
    const onResetSavedWorkspace = vi.fn();
    renderSettings(userMe, onResetSavedWorkspace);

    expect(screen.getByText("Saved workspace")).toBeInTheDocument();
    expect(screen.getByText("Open panes and panel visibility are restored on this browser.")).toBeInTheDocument();

    await user.click(screen.getByRole("button", { name: "Reset" }));

    expect(onResetSavedWorkspace).toHaveBeenCalledTimes(1);
  });

  it("shows the MCP connection cheat sheet in its own tab", async () => {
    const user = userEvent.setup();
    renderSettings();

    await user.click(screen.getByRole("tab", { name: "MCP" }));

    expect(screen.getByText("External clients use one endpoint and one bearer header.")).toBeInTheDocument();
    expect(screen.getByText("http://localhost:3000/mcp")).toBeInTheDocument();
    expect(screen.getByText("Authorization: Bearer <credential>")).toBeInTheDocument();
    expect(screen.getByText("OAuth login")).toBeInTheDocument();
    expect(screen.getByText("Agent API key")).toBeInTheDocument();
    expect(screen.getByText("User API key")).toBeInTheDocument();
  });

  it("shows agent keys and space permissions inside an expanded agent", async () => {
    const user = userEvent.setup();
    renderSettings();

    await user.click(await screen.findByRole("tab", { name: "Agents" }));
    await user.click(await screen.findByRole("button", { name: "Toggle ci-bot details" }));

    expect(screen.getByText("Agent API Keys")).toBeInTheDocument();
    expect(await screen.findByText("No keys for this agent.")).toBeInTheDocument();
    expect(screen.getByText("Space permissions")).toBeInTheDocument();
    expect(await screen.findByText("Personal")).toBeInTheDocument();
    expect(await screen.findByRole("combobox", { name: "Personal permission" })).toHaveValue("write");
  });

  it("updates agent space permissions from the permission select", async () => {
    const user = userEvent.setup();
    renderSettings();

    await user.click(await screen.findByRole("tab", { name: "Agents" }));
    await user.click(await screen.findByRole("button", { name: "Toggle ci-bot details" }));

    const permissionSelect = await screen.findByRole("combobox", { name: "Personal permission" });
    await user.selectOptions(permissionSelect, "read");

    await waitFor(() => expect(permissionSelect).toHaveValue("read"));
    expect(vi.mocked(globalThis.fetch)).toHaveBeenCalledWith(
      "/api/v1/spaces/space-1/agents/agent-1",
      expect.objectContaining({ method: "PUT", body: JSON.stringify({ permission: "read" }) })
    );
  });

  it("connects a space from no access using the permission select", async () => {
    vi.restoreAllMocks();
    mockSettingsApi(userMe, { initialSpacePermission: "none" });

    const user = userEvent.setup();
    renderSettings();

    await user.click(await screen.findByRole("tab", { name: "Agents" }));
    await user.click(await screen.findByRole("button", { name: "Toggle ci-bot details" }));

    const permissionSelect = await screen.findByRole("combobox", { name: "Personal permission" });
    expect(permissionSelect).toHaveValue("none");

    await user.selectOptions(permissionSelect, "read");

    await waitFor(() => expect(permissionSelect).toHaveValue("read"));
    expect(vi.mocked(globalThis.fetch)).toHaveBeenCalledWith(
      "/api/v1/spaces/space-1/agents/agent-1",
      expect.objectContaining({ method: "PUT", body: JSON.stringify({ permission: "read" }) })
    );
  });

  it("rolls back an optimistic permission update when the request fails", async () => {
    vi.restoreAllMocks();
    mockSettingsApi(userMe, { failPermissionUpdate: true });

    const user = userEvent.setup();
    renderSettings();

    await user.click(await screen.findByRole("tab", { name: "Agents" }));
    await user.click(await screen.findByRole("button", { name: "Toggle ci-bot details" }));

    const permissionSelect = await screen.findByRole("combobox", { name: "Personal permission" });
    expect(permissionSelect).toHaveValue("write");

    await user.selectOptions(permissionSelect, "read");

    await waitFor(() => expect(permissionSelect).toHaveValue("write"));
    expect(vi.mocked(globalThis.fetch)).toHaveBeenCalledWith(
      "/api/v1/spaces/space-1/agents/agent-1",
      expect.objectContaining({ method: "PUT", body: JSON.stringify({ permission: "read" }) })
    );
  });

  it("disconnects agent space access from the permission select", async () => {
    const user = userEvent.setup();
    renderSettings();

    await user.click(await screen.findByRole("tab", { name: "Agents" }));
    await user.click(await screen.findByRole("button", { name: "Toggle ci-bot details" }));

    const permissionSelect = await screen.findByRole("combobox", { name: "Personal permission" });
    await user.selectOptions(permissionSelect, "none");

    await waitFor(() => expect(permissionSelect).toHaveValue("none"));
    expect(vi.mocked(globalThis.fetch)).toHaveBeenCalledWith(
      "/api/v1/spaces/space-1/agents/agent-1",
      expect.objectContaining({ method: "DELETE" })
    );
  });

  it("hides agent management for callers without that capability", async () => {
    vi.restoreAllMocks();
    mockSettingsApi(agentMe);

    renderSettings(agentMe);

    await userEvent.setup().click(screen.getByRole("tab", { name: "Account" }));
    expect(await screen.findByText("ci-bot")).toBeInTheDocument();
    expect(screen.queryByRole("tab", { name: "Agents" })).not.toBeInTheDocument();
  });
});
