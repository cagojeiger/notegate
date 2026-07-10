import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";

import { ApiProvider } from "../../api/ApiProvider";
import type { RestNode, Space } from "../../api/types";
import { EventHistoryModal } from "./EventHistoryModal";

const page = { limit: 50, returned: 0, has_more: false, next_cursor: null };

const space: Space = {
  id: "space-1",
  name: "Daily",
  sort_order: 0,
  permission: "write",
  root_node_id: "root-1",
  created_at: "2026-07-01T00:00:00Z",
  updated_at: "2026-07-10T00:00:00Z"
};

function jsonResponse(body: unknown) {
  return Promise.resolve(new Response(JSON.stringify(body), { status: 200, headers: { "content-type": "application/json" } }));
}

describe("EventHistoryModal", () => {
  it("does not call the user-only audit endpoint when audit is unavailable", async () => {
    const fetchMock = vi.spyOn(globalThis, "fetch").mockResolvedValue(await jsonResponse({ events: [], page }));

    render(
      <ApiProvider apiKey="agent-key" authCacheKey="agent-key:0">
        <EventHistoryModal activeSpace={space} activeNode={null} canViewAuditEvents={false} onClose={vi.fn()} />
      </ApiProvider>
    );

    expect(screen.queryByRole("tab", { name: "Audit" })).not.toBeInTheDocument();
    expect(screen.getByRole("tab", { name: "File changes" })).toBeInTheDocument();

    await screen.findByText("No file change events.");
    await waitFor(() => expect(fetchMock).toHaveBeenCalled());
    expect(fetchMock.mock.calls.some(([input]) => String(input).includes("/api/v1/me/audit-events"))).toBe(false);
  });

  it("does not call the audit endpoint when the account loses audit access", async () => {
    const fetchMock = vi.spyOn(globalThis, "fetch").mockImplementation(() => jsonResponse({ events: [], page }));
    const { rerender } = render(
      <ApiProvider apiKey="user-key" authCacheKey="user-key:0">
        <EventHistoryModal activeSpace={space} activeNode={null} canViewAuditEvents onClose={vi.fn()} />
      </ApiProvider>
    );

    await screen.findByText("No audit events.");
    fetchMock.mockClear();

    rerender(
      <ApiProvider apiKey="agent-key" authCacheKey="agent-key:1">
        <EventHistoryModal activeSpace={space} activeNode={null} canViewAuditEvents={false} onClose={vi.fn()} />
      </ApiProvider>
    );

    await screen.findByText("No file change events.");
    expect(fetchMock.mock.calls.some(([input]) => String(input).includes("/api/v1/me/audit-events"))).toBe(false);
  });

  it("loads the next audit page from the server cursor", async () => {
    const user = userEvent.setup();
    const fetchMock = vi.spyOn(globalThis, "fetch").mockImplementation((input) => {
      const path = String(input);
      if (path.includes("cursor=audit-cursor-1")) {
        return jsonResponse({
          events: [auditEvent(2, "space.delete")],
          page: { limit: 50, returned: 1, has_more: false, next_cursor: null }
        });
      }
      return jsonResponse({
        events: [auditEvent(3, "space.update")],
        page: { limit: 50, returned: 1, has_more: true, next_cursor: "audit-cursor-1" }
      });
    });

    render(
      <ApiProvider apiKey="user-key" authCacheKey="user-key:0">
        <EventHistoryModal activeSpace={space} activeNode={null} canViewAuditEvents onClose={vi.fn()} />
      </ApiProvider>
    );

    await screen.findByText("space.update");
    await user.click(screen.getByRole("button", { name: "Load more" }));

    expect(await screen.findByText("space.delete")).toBeInTheDocument();
    expect(fetchMock.mock.calls.map(([input]) => String(input))).toContain("/api/v1/me/audit-events?limit=50&cursor=audit-cursor-1");
  });

  it("loads the next file-change page from the server cursor", async () => {
    const user = userEvent.setup();
    const fetchMock = vi.spyOn(globalThis, "fetch").mockImplementation((input) => {
      const path = String(input);
      if (path.includes("/api/v1/spaces/space-1/file-change-events") && path.includes("cursor=file-cursor-1")) {
        return jsonResponse({
          events: [fileChangeEvent(2, "item.move")],
          page: { limit: 50, returned: 1, has_more: false, next_cursor: null }
        });
      }
      if (path.includes("/api/v1/spaces/space-1/file-change-events")) {
        return jsonResponse({
          events: [fileChangeEvent(3, "text.write")],
          page: { limit: 50, returned: 1, has_more: true, next_cursor: "file-cursor-1" }
        });
      }
      return jsonResponse({ events: [], page });
    });

    render(
      <ApiProvider apiKey="user-key" authCacheKey="user-key:0">
        <EventHistoryModal activeSpace={space} activeNode={null} canViewAuditEvents onClose={vi.fn()} />
      </ApiProvider>
    );

    await user.click(screen.getByRole("tab", { name: "File changes" }));
    await screen.findByText("text.write");
    await user.click(screen.getByRole("button", { name: "Load more" }));

    expect(await screen.findByText("item.move")).toBeInTheDocument();
    expect(fetchMock.mock.calls.map(([input]) => String(input))).toContain("/api/v1/spaces/space-1/file-change-events?limit=50&cursor=file-cursor-1");
  });

  it("filters file changes by the active node", async () => {
    const user = userEvent.setup();
    const activeNode = textNode("node-1", space.id);
    const fetchMock = vi.spyOn(globalThis, "fetch").mockResolvedValue(await jsonResponse({ events: [], page }));

    render(
      <ApiProvider apiKey="user-key" authCacheKey="user-key:0">
        <EventHistoryModal activeSpace={space} activeNode={activeNode} canViewAuditEvents onClose={vi.fn()} />
      </ApiProvider>
    );

    await user.click(screen.getByRole("tab", { name: "File changes" }));
    await user.click(screen.getByRole("button", { name: "Node" }));

    expect(screen.getByText(activeNode.path)).toBeInTheDocument();
    await waitFor(() => {
      expect(fetchMock.mock.calls.map(([input]) => String(input))).toContain(
        "/api/v1/spaces/space-1/file-change-events?limit=50&node_id=node-1"
      );
    });
  });

  it("does not offer node scope for a node from another space", async () => {
    const user = userEvent.setup();
    vi.spyOn(globalThis, "fetch").mockResolvedValue(await jsonResponse({ events: [], page }));

    render(
      <ApiProvider apiKey="user-key" authCacheKey="user-key:0">
        <EventHistoryModal activeSpace={space} activeNode={textNode("node-2", "space-2")} canViewAuditEvents onClose={vi.fn()} />
      </ApiProvider>
    );

    await user.click(screen.getByRole("tab", { name: "File changes" }));

    expect(screen.getByRole("button", { name: "Node" })).toBeDisabled();
  });
});

function textNode(id: string, spaceId: string): RestNode {
  return {
    id,
    space_id: spaceId,
    parent_id: `${spaceId}-root`,
    name: `${id}.md`,
    kind: "text",
    path: `/${id}.md`,
    sort_order: 0,
    metadata: {},
    has_children: false,
    created_by: { id: "account-1", kind: "user", display_name: "User" },
    updated_by: { id: "account-1", kind: "user", display_name: "User" },
    created_at: "2026-07-10T02:00:00Z",
    updated_at: "2026-07-10T02:12:00Z"
  };
}

function auditEvent(id: number, op_type: string) {
  return {
    id,
    created_at: "2026-07-10T02:12:00Z",
    actor_account_id: "account-1",
    source: "rest",
    op_type,
    resource_type: "space",
    resource_id: space.id,
    metadata: {}
  };
}

function fileChangeEvent(id: number, op_type: string) {
  return {
    id,
    created_at: "2026-07-10T02:12:00Z",
    space_id: space.id,
    node_id: "node-1",
    actor_account_id: "account-1",
    op_type,
    metadata: {}
  };
}
