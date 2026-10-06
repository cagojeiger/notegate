import { expect, test } from "@playwright/test";

import { makeSpace } from "../src/test/fixtures";
import type { TrashItem } from "../src/api/trash";
import { routeJsonApi } from "./support/api";
import { usageResponse } from "./support/usage";

const space = makeSpace();
const item: TrashItem = {
  id: "deleted-node", space_id: space.id, space_name: space.name, kind: "text",
  name: "meeting.md", path: "/notes/meeting.md", deleted_at: "2026-10-01T00:00:00Z",
  purge_after: "2026-10-31T00:00:00Z", recoverable: true, deletion_pending: false
};

for (const viewport of [{ name: "desktop", width: 1440, height: 900 }, { name: "mobile", width: 390, height: 844 }]) {
  test(`trash confirms deletion and restores content on ${viewport.name}`, async ({ page }, testInfo) => {
    await page.setViewportSize(viewport);
    let current: TrashItem | null = item;
    let deletionRequests = 0;
    const meta = { limit: 50, returned: 0, has_more: false, next_cursor: null };
    await routeJsonApi(page, (url, request) => {
      if (url.pathname === "/api/v1/me") return { account: { id: "user-1", kind: "user", display_name: "User" }, user: { email: "user@example.com" }, capabilities: { can_create_space: true, can_manage_agents: true } };
      if (url.pathname === "/api/v1/me/usage") return usageResponse(space);
      if (url.pathname === "/api/v1/spaces") return { spaces: [space], page: meta };
      if (url.pathname === "/api/v1/me/trash") return { items: current ? [current] : [], page: meta };
      if (url.pathname.endsWith("/nodes/deleted-node/restore")) { current = null; return {}; }
      if (request.method() === "DELETE" && url.pathname.endsWith("/nodes/deleted-node")) {
        deletionRequests += 1;
        current = { ...item, recoverable: false, deletion_pending: true };
        return { status: "deletion_requested" };
      }
      if (url.pathname.endsWith("/children")) return { parent: { id: space.root_node_id, path: "/" }, children: [], page: meta };
      if (url.pathname.endsWith("/nodes")) return { nodes: [], page: meta };
      if (url.pathname.endsWith("/file-change-sync")) return { changes: [], next_after_id: 0, has_more: false, resync_required: false };
      throw new Error(`Unhandled request: ${url.pathname}`);
    });
    await page.goto("/");
    await page.getByRole("button", { name: "Trash", exact: true }).click();
    const dialog = page.getByRole("dialog", { name: "Trash" });
    await expect(dialog.getByText("meeting.md", { exact: true })).toBeVisible();
    await expect(dialog.getByText("/notes/meeting.md", { exact: false })).toBeVisible();
    await testInfo.attach(`trash-${viewport.name}`, { body: await page.screenshot(), contentType: "image/png" });
    await dialog.getByRole("button", { name: "Restore meeting.md" }).click();
    await expect(dialog.getByText("Trash is empty.")).toBeVisible();
    current = item;
    await dialog.getByRole("button", { name: "Refresh" }).click();
    await dialog.getByRole("button", { name: "Permanently delete meeting.md" }).click();
    expect(deletionRequests).toBe(0);
    await dialog.getByRole("button", { name: "Cancel" }).click();
    await dialog.getByRole("button", { name: "Permanently delete meeting.md" }).click();
    await dialog.getByRole("button", { name: "Permanently delete", exact: true }).click();
    await expect(dialog.getByText("Deletion queued · recovery unavailable")).toBeVisible();
    await expect(dialog.getByRole("button", { name: "Restore meeting.md" })).toBeDisabled();
    expect(deletionRequests).toBe(1);
  });
}
