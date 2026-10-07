import { useQueryClient, type QueryClient } from "@tanstack/react-query";
import { queryKeys } from "../../api/queryKeys";

import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";

import { ApiProvider } from "../../api/ApiProvider";
import type { TrashItem } from "../../api/trash";
import { TrashModal } from "./TrashModal";

const item: TrashItem = {
  id: "node-1", space_id: "space-1", space_name: "Daily", kind: "text",
  name: "note.md", path: "/notes/note.md", deleted_at: "2026-10-01T00:00:00Z",
  purge_after: "2026-10-31T00:00:00Z", deletion_operation_id: "operation-1", recoverable: true, deletion_pending: false
};
const page = { limit: 50, returned: 1, has_more: false, next_cursor: null };
const response = (items: TrashItem[]) => new Response(JSON.stringify({ items, page }), { status: 200, headers: { "content-type": "application/json" } });
function CacheProbe({ onReady }: { onReady?: (client: QueryClient) => void }) {
  const client = useQueryClient();
  onReady?.(client);
  return null;
}
function show(onReady?: (client: QueryClient) => void) {
  render(<ApiProvider authCacheKey="trash-test"><CacheProbe onReady={onReady} /><TrashModal onClose={vi.fn()} /></ApiProvider>);
}

describe("TrashModal", () => {
  it("shows actions only for the selected item and restores that item's deletion", async () => {
    const user = userEvent.setup();
    const second = { ...item, id: "node-2", name: "second.md", path: "/notes/second.md", deletion_operation_id: "operation-2" };
    let restored = false;
    const fetchMock = vi.spyOn(globalThis, "fetch").mockImplementation(async (_url, options) => {
      if (options?.method === "POST") {
        restored = true;
        return new Response(null, { status: 204 });
      }
      return response(restored ? [item] : [item, second]);
    });
    show();
    expect(await screen.findByRole("button", { name: "Select note.md" })).toHaveAttribute("aria-pressed", "true");
    expect(screen.queryByRole("button", { name: "Restore second.md" })).not.toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Select second.md" }));
    expect(screen.getByRole("button", { name: "Select second.md" })).toHaveAttribute("aria-pressed", "true");
    expect(screen.queryByRole("button", { name: "Restore note.md" })).not.toBeInTheDocument();
    expect(screen.getByRole("region", { name: "Details for second.md" })).toHaveTextContent("/notes/second.md");
    await user.click(screen.getByRole("button", { name: "Restore second.md" }));
    await waitFor(() => expect(screen.queryByRole("button", { name: "Select second.md" })).not.toBeInTheDocument());
    expect(screen.getByRole("button", { name: "Restore note.md" })).toBeEnabled();
    const requests = fetchMock.mock.calls.filter(([, options]) => options?.method === "POST");
    expect(requests).toHaveLength(1);
    const url = new URL(String(requests[0]?.[0]), "http://localhost");
    expect(url.pathname).toContain("/nodes/node-2/restore");
    expect(url.searchParams.get("deletion_operation_id")).toBe(second.deletion_operation_id);
  });

  it("requires explicit confirmation and displays queued deletion without claiming completion", async () => {
    const user = userEvent.setup();
    let queued = false;
    const fetchMock = vi.spyOn(globalThis, "fetch").mockImplementation(async (_url, options) => {
      if (options?.method === "DELETE") {
        queued = true;
        return new Response(JSON.stringify({ status: "deletion_requested" }), { status: 202 });
      }
      return response([{ ...item, recoverable: !queued, deletion_pending: queued }]);
    });
    show();
    await screen.findByText("note.md");
    await user.click(screen.getByRole("button", { name: "Permanently delete note.md" }));
    expect(fetchMock.mock.calls.some(([, options]) => options?.method === "DELETE")).toBe(false);
    await user.click(screen.getByRole("button", { name: "Cancel" }));
    await user.click(screen.getByRole("button", { name: "Permanently delete note.md" }));
    await user.click(screen.getByRole("button", { name: /^Permanently delete$/ }));
    await screen.findByText("Deletion queued · recovery unavailable");
    expect(screen.getByRole("button", { name: "Restore note.md" })).toBeDisabled();
    const requests = fetchMock.mock.calls.filter(([, options]) => options?.method === "DELETE");
    expect(requests).toHaveLength(1);
    const params = new URL(String(requests[0]?.[0]), "http://localhost").searchParams;
    expect(params.get("deleted_at")).toBe(item.deleted_at);
    expect(params.get("deletion_operation_id")).toBe(item.deletion_operation_id);
  });

  it("restores and refreshes the list without removing an item before server success", async () => {
    const user = userEvent.setup();
    let restored = false;
    const fetchMock = vi.spyOn(globalThis, "fetch").mockImplementation(async (_url, options) => {
      if (options?.method === "POST") {
        restored = true;
        return new Response(null, { status: 204 });
      }
      return response(restored ? [] : [item]);
    });
    show();
    await user.click(await screen.findByRole("button", { name: "Restore note.md" }));
    await screen.findByText("Trash is empty.");
    expect(fetchMock.mock.calls.some(([url]) => new URL(String(url), "http://localhost").pathname.endsWith("/nodes/node-1/restore"))).toBe(true);
  });

  it("keeps recoverable content visible when a restore conflicts", async () => {
    const user = userEvent.setup();
    vi.spyOn(globalThis, "fetch").mockImplementation(async (_url, options) => options?.method === "POST"
      ? new Response(JSON.stringify({ message: "Name conflict", kind: "conflict" }), { status: 409 })
      : response([item]));
    show();
    await user.click(await screen.findByRole("button", { name: "Restore note.md" }));
    await screen.findByRole("alert");
    expect(screen.getByText("note.md")).toBeInTheDocument();
    await waitFor(() => expect(screen.getByRole("button", { name: "Restore note.md" })).toBeEnabled());
  });

  it("disables legacy recovery and loads subsequent pages with the server cursor", async () => {
    const user = userEvent.setup();
    const fetchMock = vi.spyOn(globalThis, "fetch").mockImplementation(async (url) => {
      if (String(url).includes("cursor=next-page")) return response([{ ...item, id: "node-2", name: "second.md" }]);
      return new Response(JSON.stringify({ items: [{ ...item, recoverable: false }], page: { ...page, has_more: true, next_cursor: "next-page" } }), { status: 200 });
    });
    show();
    expect(await screen.findByRole("button", { name: "Restore note.md" })).toBeDisabled();
    await user.click(screen.getByRole("button", { name: "Load more" }));
    await screen.findByText("second.md");
    expect(fetchMock.mock.calls.some(([url]) => String(url).includes("cursor=next-page"))).toBe(true);
  });
  it("refreshes the Space resources and agent connections after restoration", async () => {
    const user = userEvent.setup();
    let restored = false;
    vi.spyOn(globalThis, "fetch").mockImplementation(async (_url, options) => {
      if (options?.method === "POST") { restored = true; return new Response(null, { status: 204 }); }
      return response(restored ? [] : [{ ...item, kind: "space" }]);
    });
    let cache: QueryClient | undefined;
    show((client) => { cache = client; });
    await screen.findByText("note.md");
    if (!cache) throw new Error("query client missing");
    cache.setQueryData(queryKeys.connections(item.space_id), { connections: ["old-agent"] });
    cache.setQueryData(queryKeys.node(item.space_id, "child"), { id: "child" });
    cache.setQueryData(queryKeys.connections("other-space"), { connections: ["other-agent"] });
    await user.click(screen.getByRole("button", { name: "Restore note.md" }));
    await screen.findByText("Trash is empty.");
    expect(cache.getQueryState(queryKeys.connections(item.space_id))?.isInvalidated).toBe(true);
    expect(cache.getQueryState(queryKeys.node(item.space_id, "child"))?.isInvalidated).toBe(true);
    expect(cache.getQueryState(queryKeys.connections("other-space"))?.isInvalidated).toBe(false);
  });

  it("refreshes a stale deletion after conflict without retrying permanent deletion", async () => {
    const user = userEvent.setup();
    let changed = false;
    const fetchMock = vi.spyOn(globalThis, "fetch").mockImplementation(async (_url, options) => {
      if (options?.method === "DELETE") {
        changed = true;
        return new Response(JSON.stringify({ message: "Trash entry has changed", kind: "conflict" }), { status: 409 });
      }
      return response([{ ...item, name: changed ? "new-note.md" : item.name, deletion_operation_id: changed ? "operation-2" : item.deletion_operation_id }]);
    });
    show();
    await user.click(await screen.findByRole("button", { name: "Permanently delete note.md" }));
    await user.click(screen.getByRole("button", { name: /^Permanently delete$/ }));
    await screen.findByText("new-note.md");
    expect(screen.getByRole("alert")).toHaveTextContent("Trash entry has changed");
    expect(fetchMock.mock.calls.filter(([, options]) => options?.method === "DELETE")).toHaveLength(1);
  });

  it.each(["restore", "purge"] as const)("refreshes a missing item after %s without retrying the action", async (action) => {
    const user = userEvent.setup();
    const method = action === "restore" ? "POST" : "DELETE";
    let missing = false;
    const fetchMock = vi.spyOn(globalThis, "fetch").mockImplementation(async (_url, options) => {
      if (options?.method === method) {
        missing = true;
        return new Response(JSON.stringify({ message: "Trash item not found", kind: "not_found" }), { status: 404 });
      }
      return response(missing ? [] : [item]);
    });
    show();
    if (action === "restore") {
      await user.click(await screen.findByRole("button", { name: "Restore note.md" }));
    } else {
      await user.click(await screen.findByRole("button", { name: "Permanently delete note.md" }));
      await user.click(screen.getByRole("button", { name: /^Permanently delete$/ }));
    }
    await screen.findByText("Trash is empty.");
    expect(screen.getByRole("alert")).toHaveTextContent("This item is no longer available in this trash view.");
    expect(screen.queryByRole("button", { name: /^Permanently delete$/ })).not.toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Refresh" })).toBeEnabled();
    expect(fetchMock.mock.calls.filter(([, options]) => options?.method === method)).toHaveLength(1);
  });

});
