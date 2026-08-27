import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";

import { ApiProvider } from "../../api/ApiProvider";
import { makeSpace } from "../../test/fixtures";
import { EventHistoryModal } from "./EventHistoryModal";

const page = { limit: 50, returned: 0, has_more: false, next_cursor: null };

const space = makeSpace({
  updated_at: "2026-07-10T00:00:00Z"
});

const secondSpace = makeSpace({
  ...space,
  id: "space-2",
  name: "Research",
  root_node_id: "root-2"
});

function jsonResponse(body: unknown) {
  return Promise.resolve(new Response(JSON.stringify(body), { status: 200, headers: { "content-type": "application/json" } }));
}

describe("EventHistoryModal", () => {
  it("does not call the user-only audit endpoint when audit is unavailable", async () => {
    const fetchMock = vi.spyOn(globalThis, "fetch").mockResolvedValue(await jsonResponse({ events: [], page }));

    render(
      <ApiProvider authCacheKey="browser-session:0">
        <EventHistoryModal spaces={[space]} initialSpaceId={space.id} canViewAuditEvents={false} onClose={vi.fn()} />
      </ApiProvider>
    );

    expect(screen.queryByRole("tab", { name: "Audit" })).not.toBeInTheDocument();
    expect(screen.queryByRole("tab", { name: "MCP" })).not.toBeInTheDocument();
    expect(screen.queryByRole("tab", { name: "CLI" })).not.toBeInTheDocument();
    expect(screen.getByRole("tab", { name: "Changes" })).toBeInTheDocument();

    await screen.findByText("No changes yet.");
    await waitFor(() => expect(fetchMock).toHaveBeenCalled());
    expect(fetchMock.mock.calls.some(([input]) => String(input).includes("/api/v1/me/audit-events"))).toBe(false);
    expect(fetchMock.mock.calls.some(([input]) => String(input).includes("/api/v1/me/command-invocations"))).toBe(false);
  });

  it("does not call the audit endpoint when the account loses audit access", async () => {
    const user = userEvent.setup();
    const fetchMock = vi.spyOn(globalThis, "fetch").mockImplementation(() => jsonResponse({ events: [], page }));
    const { rerender } = render(
      <ApiProvider authCacheKey="browser-session:0">
        <EventHistoryModal spaces={[space]} initialSpaceId={space.id} canViewAuditEvents onClose={vi.fn()} />
      </ApiProvider>
    );

    await user.click(screen.getByRole("tab", { name: "Audit" }));
    await screen.findByText("No audit events.");
    fetchMock.mockClear();

    rerender(
      <ApiProvider authCacheKey="browser-session:1">
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
      <ApiProvider authCacheKey="browser-session:0">
        <EventHistoryModal spaces={[space]} initialSpaceId={space.id} canViewAuditEvents onClose={vi.fn()} />
      </ApiProvider>
    );

    await user.click(screen.getByRole("tab", { name: "Audit" }));
    await screen.findByText("Updated a space");
    await user.click(screen.getByRole("button", { name: "Load more" }));

    expect(await screen.findByText("Deleted a space")).toBeInTheDocument();
    expect(fetchMock.mock.calls.map(([input]) => String(input))).toContain("/api/v1/me/audit-events?limit=50&cursor=audit-cursor-1");
  });

  it("keeps MCP and CLI histories on independent queries and cursors", async () => {
    const user = userEvent.setup();
    const fetchMock = vi.spyOn(globalThis, "fetch").mockImplementation((input) => {
      const path = String(input);
      if (path.includes("surface=mcp") && path.includes("cursor=mcp-cursor-1")) {
        return jsonResponse({
          command_invocations: [commandInvocation(2, "read", "read", "Review an older MCP note", "success", "mcp")],
          page: { limit: 50, returned: 1, has_more: false, next_cursor: null }
        });
      }
      if (path.includes("surface=mcp")) {
        return jsonResponse({
          command_invocations: [commandInvocation(3, "read", "changes", "Review recent changes", "success", "mcp", "Research")],
          page: { limit: 50, returned: 1, has_more: true, next_cursor: "mcp-cursor-1" }
        });
      }
      if (path.includes("surface=cli") && path.includes("cursor=cli-cursor-1")) {
        return jsonResponse({
          command_invocations: [commandInvocation(4, "search", "find", "Find an older CLI note", "success", "cli")],
          page: { limit: 50, returned: 1, has_more: false, next_cursor: null }
        });
      }
      if (path.includes("surface=cli")) {
        return jsonResponse({
          command_invocations: [commandInvocation(5, "read", "read", "Review the selected note", "error", "cli")],
          page: { limit: 50, returned: 1, has_more: true, next_cursor: "cli-cursor-1" }
        });
      }
      return jsonResponse({ events: [], page });
    });

    render(
      <ApiProvider authCacheKey="browser-session:0">
        <EventHistoryModal spaces={[space]} initialSpaceId={space.id} canViewAuditEvents onClose={vi.fn()} />
      </ApiProvider>
    );

    expect(screen.getAllByRole("tab").map((tab) => tab.textContent)).toEqual([
      "Changes",
      "Audit",
      "MCP",
      "CLI",
      "Jobs"
    ]);

    await user.click(screen.getByRole("tab", { name: "MCP" }));
    expect(await screen.findByText("Review recent changes")).toBeInTheDocument();
    expect(screen.getByTitle("MCP transport")).toHaveTextContent("MCP");
    expect(screen.getByText("read · changes")).toBeInTheDocument();
    expect(screen.getByText("Space Research")).toBeInTheDocument();
    expect(screen.getByText("12 ms")).toBeInTheDocument();
    expect(screen.getByText("Success")).toBeInTheDocument();
    expect(screen.queryByText(/"target": "Research:\/"/)).not.toBeInTheDocument();
    expect(screen.queryByText(/"kind": "complete"/)).not.toBeInTheDocument();
    await user.click(screen.getByText("Input"));
    expect(screen.getByText(/"target": "Research:\/"/)).toBeInTheDocument();
    await user.click(screen.getByText("Response"));
    expect(screen.getByText(/"kind": "complete"/)).toBeInTheDocument();
    expect(screen.getByText(/"ok": true/)).toBeInTheDocument();

    await user.click(screen.getByRole("button", { name: "Load more" }));

    expect(await screen.findByText("Review an older MCP note")).toBeInTheDocument();
    expect(fetchMock.mock.calls.map(([input]) => String(input))).toContain(
      "/api/v1/me/command-invocations?surface=mcp&limit=50&cursor=mcp-cursor-1"
    );

    await user.click(screen.getByRole("tab", { name: "CLI" }));
    expect(await screen.findByText("Review the selected note")).toBeInTheDocument();
    expect(screen.getByTitle("CLI transport")).toHaveTextContent("CLI");
    expect(screen.getByText("Error · tool_error")).toBeInTheDocument();
    expect(screen.queryByText("Review recent changes")).not.toBeInTheDocument();

    await user.click(screen.getByRole("button", { name: "Load more" }));

    expect(await screen.findByText("Find an older CLI note")).toBeInTheDocument();
    expect(fetchMock.mock.calls.map(([input]) => String(input))).toContain(
      "/api/v1/me/command-invocations?surface=cli&limit=50&cursor=cli-cursor-1"
    );
  });

  it("distinguishes legacy calls without a recorded response", async () => {
    const user = userEvent.setup();
    vi.spyOn(globalThis, "fetch").mockImplementation((input) => {
      if (String(input).includes("/api/v1/me/command-invocations")) {
        return jsonResponse({
          command_invocations: [commandInvocation(1, "read", "spaces", "List spaces", "success", "mcp", null, null)],
          page
        });
      }
      return jsonResponse({ events: [], page });
    });

    render(
      <ApiProvider authCacheKey="browser-session:0">
        <EventHistoryModal spaces={[space]} initialSpaceId={space.id} canViewAuditEvents onClose={vi.fn()} />
      </ApiProvider>
    );

    await user.click(screen.getByRole("tab", { name: "MCP" }));
    await screen.findByText("List spaces");
    await user.click(screen.getByText("Response"));

    expect(screen.getByText("Not recorded. This call predates response logging.")).toBeInTheDocument();
  });

  it("distinguishes caller identity from a missing purpose", async () => {
    const user = userEvent.setup();
    vi.spyOn(globalThis, "fetch").mockImplementation((input) => {
      if (String(input).includes("/api/v1/me/command-invocations")) {
        return jsonResponse({
          command_invocations: [
            commandInvocation(2, "me", null, null, "success", "mcp"),
            commandInvocation(1, "read", "read", null, "error", "mcp")
          ],
          page: { ...page, returned: 2 }
        });
      }
      return jsonResponse({ events: [], page });
    });

    render(
      <ApiProvider authCacheKey="browser-session:0">
        <EventHistoryModal spaces={[space]} initialSpaceId={space.id} canViewAuditEvents onClose={vi.fn()} />
      </ApiProvider>
    );

    await user.click(screen.getByRole("tab", { name: "MCP" }));

    expect(await screen.findByText("Checked caller identity")).toBeInTheDocument();
    expect(screen.getByText("Purpose not recorded")).toBeInTheDocument();
  });

  it("shows queue jobs and loads attempt history only when expanded", async () => {
    const user = userEvent.setup();
    const job = backgroundJob("job-1", "running");
    const linkJob = {
      ...backgroundJob("job-2", "succeeded"),
      kind: "link_graph_project_nodes"
    };
    const finishedJob = {
      ...backgroundJob("job-1", "succeeded"),
      completed_at: "2026-07-10T02:12:00.039Z"
    };
    const fetchMock = vi.spyOn(globalThis, "fetch").mockImplementation((input) => {
      const path = String(input);
      if (path.endsWith("/api/v1/me/jobs/job-1")) {
        return jsonResponse({
          job: finishedJob,
          attempts: [{
            attempt_number: 1,
            started_at: "2026-07-10T02:12:00.028Z",
            finished_at: "2026-07-10T02:12:00.039Z",
            outcome: "succeeded",
            error_code: null
          }]
        });
      }
      if (path.includes("/api/v1/me/jobs?")) {
        return jsonResponse({ jobs: [job, linkJob], page: { ...page, returned: 2 } });
      }
      return jsonResponse({ events: [], page });
    });

    render(
      <ApiProvider authCacheKey="browser-session:0">
        <EventHistoryModal spaces={[space]} initialSpaceId={space.id} canViewAuditEvents onClose={vi.fn()} />
      </ApiProvider>
    );

    expect(fetchMock.mock.calls.some(([input]) => String(input).includes("/api/v1/me/jobs"))).toBe(false);
    await user.click(screen.getByRole("tab", { name: "Jobs" }));
    const jobTitle = await screen.findByText("Usage recalculation");
    expect(jobTitle).toHaveClass("text-workbench");
    expect(jobTitle).not.toHaveClass("truncate");
    expect(jobTitle).toHaveClass("sm:truncate");
    expect(jobTitle.closest("li")).toHaveClass("py-2");
    expect(screen.getByText("Link indexing")).toBeInTheDocument();
    expect(screen.getByText("Running…")).toBeInTheDocument();
    expect(screen.getAllByText("Space Research")).toHaveLength(2);
    expect(screen.getAllByLabelText(/^Queued /)).toHaveLength(2);
    expect(fetchMock.mock.calls.some(([input]) => String(input).endsWith("/api/v1/me/jobs/job-1"))).toBe(false);

    await user.click(screen.getByRole("button", { name: "Show attempts for Usage recalculation" }));

    expect(await screen.findByText("Attempt 1")).toBeInTheDocument();
    expect(screen.getByText("Succeeded")).toBeInTheDocument();
    expect(screen.getByText("39 ms total")).toBeInTheDocument();
    expect(screen.getByText("Queue 28 ms")).toBeInTheDocument();
    expect(screen.getByText("Run 11 ms")).toBeInTheDocument();
    expect(screen.getByLabelText(/^Started /)).toBeInTheDocument();
    expect(screen.getAllByLabelText(/^Finished /)).toHaveLength(3);
    expect(fetchMock.mock.calls.some(([input]) => String(input).endsWith("/api/v1/me/jobs/job-1"))).toBe(true);
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
      <ApiProvider authCacheKey="browser-session:0">
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
      <ApiProvider authCacheKey="browser-session:0">
        <EventHistoryModal spaces={[space]} initialSpaceId={space.id} canViewAuditEvents onClose={vi.fn()} />
      </ApiProvider>
    );

    await screen.findByText("No changes yet.");
    expect(screen.queryByRole("button", { name: "Node" })).not.toBeInTheDocument();
    expect(fetchMock.mock.calls.map(([input]) => String(input))).toContain(
      "/api/v1/spaces/space-1/file-change-events?limit=50"
    );
  });

  it("reveals structured file-change details on demand", async () => {
    const user = userEvent.setup();
    vi.spyOn(globalThis, "fetch").mockResolvedValue(await jsonResponse({
      events: [{
        ...fileChangeEvent(1, "file.create"),
        node_id: "12345678-1234-1234-1234-123456789012",
        metadata: {
          item_kind: "file",
          item_name: "archive.zip",
          parent_node_id: "87654321-4321-4321-4321-210987654321",
          byte_len_after: 1536
        }
      }],
      page: { ...page, returned: 1 }
    }));

    render(
      <ApiProvider authCacheKey="browser-session:0">
        <EventHistoryModal spaces={[space]} initialSpaceId={space.id} canViewAuditEvents onClose={vi.fn()} />
      </ApiProvider>
    );

    const toggle = await screen.findByRole("button", { name: "Show change details for File · archive.zip" });
    expect(screen.queryByText("1.5 KB")).not.toBeInTheDocument();

    await user.click(toggle);

    expect(screen.getByText("1.5 KB")).toBeInTheDocument();
    expect(screen.getByText("87654321…4321")).toBeInTheDocument();
    expect(screen.getByText("12345678…9012")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Hide change details for File · archive.zip" })).toHaveAttribute("aria-expanded", "true");
  });

  it("switches the activity query without changing the workbench space", async () => {
    const user = userEvent.setup();
    const fetchMock = vi.spyOn(globalThis, "fetch").mockResolvedValue(await jsonResponse({ events: [], page }));

    render(
      <ApiProvider authCacheKey="browser-session:0">
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

function commandInvocation(
  id: number,
  tool: string,
  op: string | null,
  purpose: string | null,
  outcome: "success" | "error",
  surface: "mcp" | "cli" = "mcp",
  spaceName: string | null = null,
  response: Record<string, unknown> | null = {
    kind: "complete",
    is_error: false,
    result: { ok: true }
  }
) {
  return {
    id,
    created_at: "2026-07-10T02:12:00Z",
    actor_account_id: "account-1",
    actor: { id: "account-1", kind: "user", display_name: "REST Test Owner" },
    caller_kind: "user",
    surface,
    tool,
    op,
    purpose,
    space_name: spaceName,
    input: {
      purpose,
      op,
      ...(spaceName ? { target: `${spaceName}:/` } : {})
    },
    response,
    outcome,
    error_code: outcome === "error" ? "tool_error" : null,
    duration_ms: 12
  };
}

function backgroundJob(id: string, status: "queued" | "running" | "succeeded" | "dead") {
  return {
    id,
    kind: "space_usage_reconcile",
    status,
    context_kind: "space",
    context_id: secondSpace.id,
    context_label: secondSpace.name,
    attempt_count: status === "queued" ? 0 : 1,
    failure_count: 0,
    max_attempts: 8,
    last_error_code: null,
    created_at: "2026-07-10T02:12:00Z",
    updated_at: "2026-07-10T02:12:01Z",
    completed_at: status === "succeeded" || status === "dead" ? "2026-07-10T02:12:02Z" : null
  };
}
