import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { ApiProvider } from "../api/ApiProvider";
import type { Space } from "../api/types";
import { SettingsModal } from "./SettingsModal";

const page = { limit: 100, returned: 0, has_more: false, next_cursor: null };

const space: Space = {
  id: "space-1",
  name: "Personal",
  sort_order: 0,
  permission: "owner",
  root_node_id: "root-1",
  created_at: "2026-06-13T00:00:00Z",
  updated_at: "2026-06-13T00:00:00Z"
};

function jsonResponse(body: unknown) {
  return Promise.resolve(new Response(JSON.stringify(body), { status: 200, headers: { "content-type": "application/json" } }));
}

function mockSettingsApi() {
  vi.spyOn(globalThis, "fetch").mockImplementation((input) => {
    const path = String(input);
    if (path.includes("/api/v1/me/keys")) return jsonResponse({ keys: [], page });
    if (path.includes("/api/v1/agents")) return jsonResponse({ agents: [], page });
    if (path.includes("/api/v1/spaces/") && path.includes("/agents")) return jsonResponse({ connections: [], page });
    if (path.includes("/api/v1/me")) {
      return jsonResponse({ account: { id: "user-1", kind: "user", display_name: "Kang" }, user: { email: "kang@example.com" } });
    }
    return jsonResponse({});
  });
}

function renderSettings(activeSpace: Space | null = space) {
  render(
    <ApiProvider apiKey="test-key">
      <SettingsModal activeSpace={activeSpace} onClose={vi.fn()} onSignOut={vi.fn()} />
    </ApiProvider>
  );
}

describe("SettingsModal", () => {
  beforeEach(mockSettingsApi);

  it("switches between top-level settings tabs", async () => {
    const user = userEvent.setup();
    renderSettings();

    expect(screen.getByRole("heading", { name: "Settings" })).toBeInTheDocument();
    expect(screen.getByText("Appearance")).toBeInTheDocument();

    await user.click(screen.getByRole("tab", { name: "API Keys" }));
    expect(await screen.findByText("No active keys.")).toBeInTheDocument();

    await user.click(screen.getByRole("tab", { name: "Agents" }));
    expect(screen.getByText("New agent name")).toBeInTheDocument();
    expect(await screen.findByText("No agents yet.")).toBeInTheDocument();

    await user.click(screen.getByRole("tab", { name: "Connections" }));
    expect(await screen.findByText(/Connect agents to/)).toBeInTheDocument();
    expect(await screen.findByText("Create an agent first (Agents tab).")).toBeInTheDocument();
  });

  it("shows a clear connections empty state when no space is selected", async () => {
    const user = userEvent.setup();
    renderSettings(null);

    await user.click(screen.getByRole("tab", { name: "Connections" }));

    expect(screen.getByText("Select a space to manage agent connections.")).toBeInTheDocument();
  });
});
