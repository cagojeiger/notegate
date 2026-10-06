import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";

import { ApiProvider } from "../../api/ApiProvider";
import type { TrashItem } from "../../api/trash";
import { TrashModal } from "./TrashModal";

const item: TrashItem = {
  id: "node-1", space_id: "space-1", space_name: "Daily", kind: "text",
  name: "note.md", path: "/notes/note.md", deleted_at: "2026-10-01T00:00:00Z",
  purge_after: "2026-10-31T00:00:00Z", recoverable: true, deletion_pending: false
};
const page = { limit: 50, returned: 1, has_more: false, next_cursor: null };
const response = (items: TrashItem[]) => new Response(JSON.stringify({ items, page }), { status: 200, headers: { "content-type": "application/json" } });
function show() {
  render(<ApiProvider authCacheKey="trash-test"><TrashModal onClose={vi.fn()} /></ApiProvider>);
}

describe("TrashModal", () => {
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
    await user.click(screen.getByRole("button", { name: "Permanently delete", exact: true }));
    await screen.findByText("Deletion queued · recovery unavailable");
    expect(screen.getByRole("button", { name: "Restore note.md" })).toBeDisabled();
    expect(fetchMock.mock.calls.filter(([, options]) => options?.method === "DELETE")).toHaveLength(1);
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
    expect(fetchMock.mock.calls.some(([url]) => String(url).endsWith("/nodes/node-1/restore"))).toBe(true);
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
});
