import { act, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { ApiProvider } from "../../api/ApiProvider";
import type { Me } from "../../api/types";
import { makeSpace } from "../../test/fixtures";
import { SettingsView } from "./SettingsView";

const page = { limit: 100, returned: 0, has_more: false, next_cursor: null };

const space = makeSpace({
  name: "Personal",
});

function jsonResponse(body: unknown, status = 200) {
  return Promise.resolve(new Response(JSON.stringify(body), { status, headers: { "content-type": "application/json" } }));
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

type SettingsApiOptions = { failPermissionUpdate?: boolean; initialSpacePermission?: SpacePermission; failedRead?: "agents" | "spaces" | "connections"; createAgent?: () => Promise<void> };

function mockSettingsApi(me: unknown = userMe, options: SettingsApiOptions = {}) {
  let spacePermission: SpacePermission = options.initialSpacePermission ?? "write";
  const agents = [{ id: "agent-1", name: "ci-bot", owner_user_id: "user-1" }];
  vi.spyOn(globalThis, "fetch").mockImplementation((input, init) => {
    const path = String(input);
    if (path === "/api/v1/agents" && init?.method === "POST") {
      return Promise.resolve(options.createAgent?.()).then(() => {
        const agent = { id: "agent-2", name: JSON.parse(String(init.body)).name as string, owner_user_id: "user-1" };
        agents.push(agent);
        return jsonResponse(agent, 201);
      });
    }
    if (path === "/api/v1/me/sessions") return jsonResponse({ sessions: [] });
    if (path === "/api/v1/me/oauth-grants") return jsonResponse({ grants: [] });
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
      if (options.failedRead === "connections") return jsonResponse({ message: "Unavailable" }, 503);
      const connections = spacePermission === "none"
        ? []
        : [{ agent: { id: "agent-1", kind: "agent", display_name: "ci-bot" }, permission: spacePermission, connected_at: "2026-06-13T00:00:00Z" }];
      return jsonResponse({ connections, page });
    }
    if (path.includes("/api/v1/spaces?")) return options.failedRead === "spaces" ? jsonResponse({ message: "Unavailable" }, 503) : jsonResponse({ spaces: [space], page });
    if (path.endsWith("/api/v1/agents?limit=100")) return options.failedRead === "agents" ? jsonResponse({ message: "Unavailable" }, 503) : jsonResponse({ agents, page });
    if (path.includes("/api/v1/me")) {
      return jsonResponse(me);
    }
    return jsonResponse({});
  });
}

function renderSettings(me = userMe, onResetSavedWorkspace = vi.fn(), props: Partial<Parameters<typeof SettingsView>[0]> = {}) {
  return render(
    <ApiProvider authCacheKey="browser-session:0">
      <SettingsView me={me} onClose={vi.fn()} onSignOut={vi.fn()} onResetSavedWorkspace={onResetSavedWorkspace} {...props} />
    </ApiProvider>
  );
}

describe("SettingsView", () => {
  beforeEach(() => {
    vi.restoreAllMocks();
    mockSettingsApi();
  });

  it.each([false, true])("keeps primary actions in the body with the Inspector closed (mobile: %s)", async (isMobile) => {
    const user = userEvent.setup();
    renderSettings(userMe, vi.fn(), { isMobile, inspectorOpen: false });
    expect(within(screen.getByRole("tabpanel", { name: "General" })).getByRole("button", { name: "Reset" })).toBeInTheDocument();
    expect(screen.queryByRole("complementary")).not.toBeInTheDocument();

    await user.click(screen.getByRole("tab", { name: "Agents" }));
    const agents = within(screen.getByRole("tabpanel", { name: "Agents" }));
    await user.click(await agents.findByRole("button", { name: "Select ci-bot" }));
    expect(await screen.findByRole("region", { name: "Settings for ci-bot" })).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Back to agents" }));
    expect(agents.getByRole("button", { name: "Add agent" })).toBeInTheDocument();

    await user.click(screen.getByRole("tab", { name: "Connections" }));
    const connections = within(screen.getByRole("tabpanel", { name: "Connections" }));
    expect(connections.getByRole("tab", { name: "MCP" })).toBeInTheDocument();
    expect(connections.getByRole("tab", { name: "CLI" })).toBeInTheDocument();
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
  });

  it("keeps profile and browser sessions in Account and external access in Connections", async () => {
    const user = userEvent.setup();
    renderSettings();

    expect(screen.getByRole("heading", { name: "Settings" })).toBeInTheDocument();
    await user.click(screen.getByRole("tab", { name: "Account" }));

    expect(screen.queryByText("Appearance")).not.toBeInTheDocument();
    expect(screen.getByText("Profile")).toBeInTheDocument();
    expect(screen.queryByText("User MCP")).not.toBeInTheDocument();
    expect(screen.queryByText("My API Keys")).not.toBeInTheDocument();
    expect(screen.queryByRole("tab", { name: "MCP" })).not.toBeInTheDocument();
    expect(screen.getByRole("tab", { name: "Connections" })).toBeInTheDocument();
    expect(screen.queryByText("Agent MCP")).not.toBeInTheDocument();
    expect(screen.queryByText("REST API")).not.toBeInTheDocument();
    expect(vi.mocked(fetch).mock.calls.some(([path]) => String(path).endsWith("/oauth-grants"))).toBe(false);
    await user.click(screen.getByRole("tab", { name: "Connections" }));
    expect(screen.getByText("User MCP")).toBeInTheDocument();
    expect(screen.getByText("http://localhost:3000/mcp")).toBeInTheDocument();
    expect(vi.mocked(fetch).mock.calls.some(([path]) => String(path).endsWith("/oauth-grants"))).toBe(false);
    await user.click(screen.getByRole("tab", { name: "CLI" }));
    await screen.findByText("No active CLI connections.");
    expect(vi.mocked(fetch).mock.calls.some(([path]) => String(path).endsWith("/oauth-grants"))).toBe(true);
  });

  it("restores the selected agent across settings tabs and forgets it after settings closes", async () => {
    const user = userEvent.setup();
    const view = renderSettings();
    await user.click(screen.getByRole("tab", { name: "Agents" }));
    await user.click(await screen.findByRole("button", { name: "Select ci-bot" }));
    expect(await screen.findByRole("region", { name: "Settings for ci-bot" })).toBeInTheDocument();
    await user.click(screen.getByRole("tab", { name: "Account" }));
    await user.click(screen.getByRole("tab", { name: "Agents" }));
    expect(await screen.findByRole("region", { name: "Settings for ci-bot" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Back to agents" })).toBeInTheDocument();
    view.unmount();
    renderSettings();
    await user.click(screen.getByRole("tab", { name: "Agents" }));
    await screen.findByRole("button", { name: "Select ci-bot" });
    expect(screen.queryByRole("region", { name: "Settings for ci-bot" })).not.toBeInTheDocument();
  });

  it("selects the newly created agent instead of returning to the previous agent", async () => {
    const user = userEvent.setup();
    renderSettings();
    await user.click(screen.getByRole("tab", { name: "Agents" }));
    await user.click(await screen.findByRole("button", { name: "Select ci-bot" }));
    await user.click(screen.getByRole("button", { name: "Back to agents" }));
    await user.click(screen.getByRole("button", { name: "Add agent" }));
    await user.type(screen.getByRole("textbox", { name: "New agent name" }), "review-bot");
    await user.click(screen.getByRole("button", { name: "Create" }));

    expect(await screen.findByRole("region", { name: "Settings for review-bot" })).toBeInTheDocument();
    expect(screen.getByText("Agent created.")).toHaveAttribute("role", "status");
    expect(screen.getByRole("heading", { name: "review-bot" })).toBeInTheDocument();
    expect(screen.queryByRole("region", { name: "Settings for ci-bot" })).not.toBeInTheDocument();
  });

  it("keeps agent creation retryable without losing the entered name", async () => {
    vi.restoreAllMocks();
    let rejectCreation: (error: Error) => void = () => undefined;
    const createAgent = vi.fn()
      .mockImplementationOnce(() => new Promise<void>((_resolve, reject) => { rejectCreation = reject; }))
      .mockResolvedValue(undefined);
    mockSettingsApi(userMe, { createAgent });
    const user = userEvent.setup();
    renderSettings();
    await user.click(screen.getByRole("tab", { name: "Agents" }));
    await user.click(screen.getByRole("button", { name: "Add agent" }));
    await user.type(screen.getByLabelText("New agent name"), "review-bot");
    const create = screen.getByRole("button", { name: "Create" });
    await user.click(create);
    expect(create).toBeDisabled();
    expect(create).toHaveAttribute("aria-busy", "true");
    expect(screen.getByRole("status")).toHaveTextContent("Creating agent…");
    await act(async () => { rejectCreation(new Error("Unavailable")); });
    expect(await screen.findByRole("alert")).toHaveTextContent("Could not create this agent.");
    expect(screen.getByLabelText("New agent name")).toHaveValue("review-bot");
    await user.click(create);
    expect(await screen.findByText("Agent created.")).toHaveAttribute("role", "status");
    expect(await screen.findByRole("region", { name: "Settings for review-bot" })).toBeInTheDocument();
    expect(screen.queryByRole("alert")).not.toBeInTheDocument();
    expect(createAgent).toHaveBeenCalledTimes(2);
  });

  it("reports a late agent creation failure after leaving the view", async () => {
    vi.restoreAllMocks();
    let rejectCreation: (error: Error) => void = () => undefined;
    mockSettingsApi(userMe, { createAgent: () => new Promise<void>((_resolve, reject) => { rejectCreation = reject; }) });
    const onMutationError = vi.fn();
    const user = userEvent.setup();
    render(<ApiProvider authCacheKey="browser-session:0" onMutationError={onMutationError}>
      <SettingsView me={userMe} onClose={vi.fn()} onSignOut={vi.fn()} />
    </ApiProvider>);
    await user.click(screen.getByRole("tab", { name: "Agents" }));
    await user.click(screen.getByRole("button", { name: "Add agent" }));
    await user.type(screen.getByLabelText("New agent name"), "review-bot");
    await user.click(screen.getByRole("button", { name: "Create" }));
    await user.click(screen.getByRole("tab", { name: "General" }));
    await act(async () => { rejectCreation(new Error("Agent service unavailable")); });
    await waitFor(() => expect(onMutationError).toHaveBeenCalledWith("Agent service unavailable"));
    expect(screen.queryByRole("alert")).not.toBeInTheDocument();
    expect(screen.getByRole("tabpanel", { name: "General" })).toBeInTheDocument();
  });

  it("collects user MCP, agent MCP and REST API endpoints in Connections", async () => {
    const user = userEvent.setup();
    renderSettings();

    await user.click(await screen.findByRole("tab", { name: "Connections" }));

    expect(screen.getByRole("heading", { name: "Connections" })).toBeInTheDocument();
    expect(screen.getByText("Agent MCP")).toBeInTheDocument();
    expect(screen.getByText("http://localhost:3000/mcp/v2")).toBeInTheDocument();
    expect(screen.getByText("REST API")).toBeInTheDocument();
    expect(screen.getByText("http://localhost:3000/api/v2")).toBeInTheDocument();
    expect(screen.queryByText("REST API v2")).not.toBeInTheDocument();
    expect(screen.getByRole("link", { name: "API documentation" })).toHaveAttribute("href", "http://localhost:3000/swagger-ui/v2/");
    expect(screen.getByRole("link", { name: "API documentation" })).toHaveAttribute("target", "_blank");
  });

  it("shows browser workspace reset controls in the general tab", async () => {
    const user = userEvent.setup();
    const onResetSavedWorkspace = vi.fn();
    renderSettings(userMe, onResetSavedWorkspace);

    expect(screen.getByText("Saved workspace")).toBeInTheDocument();
    expect(screen.getByText("Open panes and panel visibility are restored on this browser.")).toBeInTheDocument();
    expect(screen.getByText("About")).toBeInTheDocument();
    expect(screen.getByLabelText("Version vdevelopment")).toHaveTextContent("Version vdevelopment");
    expect(screen.getByRole("link", { name: "Open NoteGate on GitHub" })).toHaveAttribute("href", "https://github.com/cagojeiger/notegate");
    expect(screen.getByRole("link", { name: "Open NoteGate on GitHub" })).toHaveAttribute("target", "_blank");

    await user.click(screen.getByRole("button", { name: "Reset" }));

    expect(onResetSavedWorkspace).toHaveBeenCalledTimes(1);
  });

  it("keeps space usage out of settings", () => {
    renderSettings();

    expect(screen.queryByRole("tab", { name: "Usage" })).not.toBeInTheDocument();
  });

  it("shows only permissions and keys inside an expanded agent", async () => {
    const user = userEvent.setup();
    renderSettings();

    await user.click(await screen.findByRole("tab", { name: "Agents" }));
    const agentToggle = await screen.findByRole("button", { name: "Select ci-bot" });
    await user.click(agentToggle);
    const agentCard = screen.getByRole("region", { name: "Settings for ci-bot" });
    expect(agentCard).not.toBeNull();
    const agentDetails = within(agentCard as HTMLElement);

    expect(agentDetails.getByText("Agent API Keys")).toBeInTheDocument();
    expect(await agentDetails.findByText("No keys for this agent.")).toBeInTheDocument();
    expect(agentDetails.getByText("Space permissions")).toBeInTheDocument();
    expect(await agentDetails.findByText("Personal")).toBeInTheDocument();
    expect(await agentDetails.findByRole("combobox", { name: "Personal permission" })).toHaveValue("write");
    expect(agentDetails.queryByText("Agent MCP")).not.toBeInTheDocument();
    expect(agentDetails.queryByText("REST API")).not.toBeInTheDocument();
  });

  it.each([
    { failedRead: "agents", message: "Could not load agents.", empty: "No agents yet.", retry: "Retry agents" },
    { failedRead: "spaces", message: "Could not load spaces.", empty: "No writable spaces.", retry: "Retry spaces" }
  ] as const)("distinguishes a failed $failedRead read from an empty list and retries it", async ({ failedRead, message, empty, retry }) => {
    vi.restoreAllMocks();
    const options: SettingsApiOptions = { failedRead };
    mockSettingsApi(userMe, options);
    const user = userEvent.setup();
    renderSettings();
    await user.click(screen.getByRole("tab", { name: "Agents" }));
    if (failedRead === "spaces") await user.click(await screen.findByRole("button", { name: "Select ci-bot" }));

    expect(await screen.findByRole("alert", {}, { timeout: 3_000 })).toHaveTextContent(message);
    expect(screen.queryByText(empty)).not.toBeInTheDocument();
    options.failedRead = undefined;
    await user.click(screen.getByRole("button", { name: retry }));

    if (failedRead === "agents") {
      expect(await screen.findByRole("button", { name: "Select ci-bot" })).toBeInTheDocument();
    } else {
      expect(await screen.findByRole("combobox", { name: "Personal permission" })).toHaveValue("write");
    }
    expect(screen.queryByRole("alert")).not.toBeInTheDocument();
  });

  it("does not describe unavailable permissions as no access and retries the read", async () => {
    vi.restoreAllMocks();
    const options: SettingsApiOptions = { failedRead: "connections" };
    mockSettingsApi(userMe, options);
    const user = userEvent.setup();
    renderSettings();
    await user.click(screen.getByRole("tab", { name: "Agents" }));
    await user.click(await screen.findByRole("button", { name: "Select ci-bot" }));

    expect(await screen.findByRole("alert", {}, { timeout: 3_000 })).toHaveTextContent("Could not load permissions.");
    expect(screen.queryByText("No access")).not.toBeInTheDocument();
    expect(screen.queryByRole("combobox", { name: "Personal permission" })).not.toBeInTheDocument();
    options.failedRead = undefined;
    await user.click(screen.getByRole("button", { name: "Retry Personal permissions" }));

    expect(await screen.findByRole("combobox", { name: "Personal permission" })).toHaveValue("write");
    expect(screen.queryByRole("alert")).not.toBeInTheDocument();
  });

  it("updates agent space permissions from the permission select", async () => {
    const user = userEvent.setup();
    renderSettings();

    await user.click(await screen.findByRole("tab", { name: "Agents" }));
    await user.click(await screen.findByRole("button", { name: "Select ci-bot" }));

    const permissionSelect = await screen.findByRole("combobox", { name: "Personal permission" });
    await user.selectOptions(permissionSelect, "read");

    expect(await screen.findByText("Space access updated.")).toHaveAttribute("role", "status");
    expect(permissionSelect).toHaveValue("read");
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
    await user.click(await screen.findByRole("button", { name: "Select ci-bot" }));

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
    const options = { failPermissionUpdate: true };
    mockSettingsApi(userMe, options);

    const user = userEvent.setup();
    renderSettings();

    await user.click(await screen.findByRole("tab", { name: "Agents" }));
    await user.click(await screen.findByRole("button", { name: "Select ci-bot" }));

    const permissionSelect = await screen.findByRole("combobox", { name: "Personal permission" });
    expect(permissionSelect).toHaveValue("write");

    await user.selectOptions(permissionSelect, "read");

    await waitFor(() => expect(permissionSelect).toHaveValue("write"));
    expect(await screen.findByRole("alert")).toHaveTextContent("Could not update access.");
    expect(vi.mocked(globalThis.fetch)).toHaveBeenCalledWith(
      "/api/v1/spaces/space-1/agents/agent-1",
      expect.objectContaining({ method: "PUT", body: JSON.stringify({ permission: "read" }) })
    );
    options.failPermissionUpdate = false;
    await user.selectOptions(permissionSelect, "read");
    expect(await screen.findByText("Space access updated.")).toHaveAttribute("role", "status");
    expect(permissionSelect).toHaveValue("read");
    expect(screen.queryByRole("alert")).not.toBeInTheDocument();
  });

  it("disconnects agent space access from the permission select", async () => {
    const user = userEvent.setup();
    renderSettings();

    await user.click(await screen.findByRole("tab", { name: "Agents" }));
    await user.click(await screen.findByRole("button", { name: "Select ci-bot" }));

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
    const user = userEvent.setup();

    await user.click(screen.getByRole("tab", { name: "Account" }));
    expect(await screen.findByText("ci-bot")).toBeInTheDocument();
    expect(screen.queryByText("User MCP")).not.toBeInTheDocument();
    expect(screen.queryByRole("tab", { name: "Agents" })).not.toBeInTheDocument();
    expect(screen.getByRole("tab", { name: "Connections" })).toBeInTheDocument();
    expect(screen.queryByRole("tab", { name: "Usage" })).not.toBeInTheDocument();
    expect(screen.queryByText("Agent MCP")).not.toBeInTheDocument();
    expect(screen.queryByText("REST API")).not.toBeInTheDocument();
    await user.click(screen.getByRole("tab", { name: "Connections" }));
    expect(screen.getByText("User MCP")).toBeInTheDocument();
    expect(screen.queryByRole("tab", { name: "CLI" })).not.toBeInTheDocument();
    expect(screen.queryByRole("tab", { name: "MCP" })).not.toBeInTheDocument();
  });
});
