import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";

import { ApiProvider } from "../../api/ApiProvider";
import type { Space } from "../../api/types";
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

const secondSpace: Space = {
  ...space,
  id: "space-2",
  name: "Research",
  root_node_id: "root-2"
};

function jsonResponse(body: unknown) {
  return Promise.resolve(new Response(JSON.stringify(body), { status: 200, headers: { "content-type": "application/json" } }));
}

describe("EventHistoryModal", () => {
  it("does not call the user-only audit endpoint when audit is unavailable", async () => {
    const fetchMock = vi.spyOn(globalThis, "fetch").mockResolvedValue(await jsonResponse({ events: [], page }));

    render(
      <ApiProvider apiKey="agent-key" authCacheKey="agent-key:0">
        <EventHistoryModal spaces={[space]} initialSpaceId={space.id} canViewAuditEvents={false} onClose={vi.fn()} />
      </ApiProvider>
    );

    expect(screen.queryByRole("tab", { name: "Audit log" })).not.toBeInTheDocument();
    expect(screen.getByRole("tab", { name: "Changes" })).toBeInTheDocument();

    await screen.findByText("No changes yet.");
    await waitFor(() => expect(fetchMock).toHaveBeenCalled());
    expect(fetchMock.mock.calls.some(([input]) => String(input).includes("/api/v1/me/audit-events"))).toBe(false);
  });

  it("does not call the audit endpoint when the account loses audit access", async () => {
    const user = userEvent.setup();
    const fetchMock = vi.spyOn(globalThis, "fetch").mockImplementation(() => jsonResponse({ events: [], page }));
    const { rerender } = render(
      <ApiProvider apiKey="user-key" authCacheKey="user-key:0">
        <EventHistoryModal spaces={[space]} initialSpaceId={space.id} canViewAuditEvents onClose={vi.fn()} />
      </ApiProvider>
    );

    await user.click(screen.getByRole("tab", { name: "Audit log" }));
    await screen.findByText("No audit events.");
    fetchMock.mockClear();

    rerender(
      <ApiProvider apiKey="agent-key" authCacheKey="agent-key:1">
        <EventHistoryModal spaces={[space]} initialSpaceId={space.id} canViewAuditEvents={false} onClose={vi.fn()} />
      </ApiProvider>
    );

    await screen.findByText("No changes yet.");
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
        <EventHistoryModal spaces={[space]} initialSpaceId={space.id} canViewAuditEvents onClose={vi.fn()} />
      </ApiProvider>
    );

    await user.click(screen.getByRole("tab", { name: "Audit log" }));
    await screen.findByText("Updated a space");
    await user.click(screen.getByRole("button", { name: "Load more" }));

    expect(await screen.findByText("Deleted a space")).toBeInTheDocument();
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
        <EventHistoryModal spaces={[space]} initialSpaceId={space.id} canViewAuditEvents onClose={vi.fn()} />
      </ApiProvider>
    );

    await screen.findByText("Edited");
    await user.click(screen.getByRole("button", { name: "Load more" }));

    expect(await screen.findByText("Moved")).toBeInTheDocument();
    expect(fetchMock.mock.calls.map(([input]) => String(input))).toContain("/api/v1/spaces/space-1/file-change-events?limit=50&cursor=file-cursor-1");
  });

  it("shows one space-wide timeline without a node scope control", async () => {
    const fetchMock = vi.spyOn(globalThis, "fetch").mockResolvedValue(await jsonResponse({ events: [], page }));

    render(
      <ApiProvider apiKey="user-key" authCacheKey="user-key:0">
        <EventHistoryModal spaces={[space]} initialSpaceId={space.id} canViewAuditEvents onClose={vi.fn()} />
      </ApiProvider>
    );

    await screen.findByText("No changes yet.");
    expect(screen.queryByRole("button", { name: "Node" })).not.toBeInTheDocument();
    expect(fetchMock.mock.calls.map(([input]) => String(input))).toContain(
      "/api/v1/spaces/space-1/file-change-events?limit=50"
    );
  });

  it("switches the activity query without changing the workbench space", async () => {
    const user = userEvent.setup();
    const fetchMock = vi.spyOn(globalThis, "fetch").mockResolvedValue(await jsonResponse({ events: [], page }));

    render(
      <ApiProvider apiKey="user-key" authCacheKey="user-key:0">
        <EventHistoryModal spaces={[space, secondSpace]} initialSpaceId={space.id} canViewAuditEvents onClose={vi.fn()} />
      </ApiProvider>
    );

    await screen.findByText("No changes yet.");
    await user.selectOptions(screen.getByRole("combobox", { name: "Space" }), secondSpace.id);

    await waitFor(() => expect(fetchMock.mock.calls.map(([input]) => String(input))).toContain(
      "/api/v1/spaces/space-2/file-change-events?limit=50"
    ));
    expect(screen.getByRole("combobox", { name: "Space" })).toHaveValue(secondSpace.id);
  });
});

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
