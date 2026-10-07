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
  test(`compact trash selects one item on ${viewport.name}`, async ({ page }, testInfo) => {
    await page.setViewportSize(viewport);
    const items: TrashItem[] = [
      item,
      { ...item, id: "reports", kind: "folder", name: "reports", path: "/work/reports" },
      { ...item, id: "archive", name: "archive.md", path: "/archive/archive.md", recoverable: false },
      { ...item, id: "old-image", kind: "file", name: "old.png", path: "/images/old.png", recoverable: false, deletion_pending: true }
    ];
    const meta = { limit: 50, returned: 0, has_more: false, next_cursor: null };
    await routeJsonApi(page, (url) => {
      if (url.pathname === "/api/v1/me") return { account: { id: "user-1", kind: "user", display_name: "User" }, user: { email: "user@example.com" }, capabilities: { can_create_space: true, can_manage_agents: true } };
      if (url.pathname === "/api/v1/me/usage") return usageResponse(space);
      if (url.pathname === "/api/v1/spaces") return { spaces: [space], page: meta };
      if (url.pathname === "/api/v1/me/trash") return { items, page: { ...meta, returned: items.length } };
      if (url.pathname.endsWith("/children")) return { parent: { id: space.root_node_id, path: "/" }, children: [], page: meta };
      if (url.pathname.endsWith("/nodes")) return { nodes: [], page: meta };
      if (url.pathname.endsWith("/file-change-sync")) return { changes: [], next_after_id: 0, has_more: false, resync_required: false };
      throw new Error(`Unhandled request: ${url.pathname}`);
    });
    await page.goto("/");
    await page.getByRole("button", { name: "Trash", exact: true }).click();
    const dialog = page.getByRole("dialog", { name: "Trash" });
    await expect(dialog.getByRole("button", { name: "Select meeting.md" })).toHaveAttribute("aria-pressed", "true");
    await expect(dialog.getByRole("button", { name: "Restore reports" })).toHaveCount(0);
    await dialog.getByRole("button", { name: "Select reports" }).click();
    await expect(dialog.getByRole("region", { name: "Details for reports" })).toContainText("Items deleted separately stay in Trash.");
    await expect(dialog.getByRole("button", { name: "Restore reports" })).toBeVisible();
    await expect(dialog.getByRole("button", { name: "Restore meeting.md" })).toHaveCount(0);
    expect(await dialog.evaluate((element) => element.scrollWidth <= element.clientWidth)).toBe(true);
    await testInfo.attach(`trash-${viewport.name}-compact`, { body: await page.screenshot({ path: `test-results/trash-${viewport.name}-compact.png` }), contentType: "image/png" });
    await dialog.getByRole("button", { name: "Select old.png" }).click();
    await expect(dialog.getByRole("button", { name: "Restore old.png" })).toBeDisabled();
    await expect(dialog.getByRole("button", { name: "Permanently delete old.png" })).toBeDisabled();
  });

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
    await expect(page.getByRole("button", { name: "Trash", exact: true })).toBeVisible();
    await page.screenshot({ path: `test-results/trash-${viewport.name}-sidebar.png` });
    await page.getByRole("button", { name: "Trash", exact: true }).click();
    const dialog = page.getByRole("dialog", { name: "Trash" });
    await expect(dialog.getByText("meeting.md", { exact: true })).toBeVisible();
    await expect(dialog.getByRole("region", { name: "Details for meeting.md" })).toContainText("/notes/meeting.md");
    await testInfo.attach(`trash-${viewport.name}`, { body: await page.screenshot({ path: `test-results/trash-${viewport.name}.png` }), contentType: "image/png" });
    const restorePath = "**/api/v1/me/trash/spaces/*/nodes/deleted-node/restore?**";
    const conflictMessage = "a node named 'meeting.md' already exists in this folder";
    await page.route(restorePath, (route) => route.fulfill({
      status: 409,
      contentType: "application/json",
      body: JSON.stringify({ error: "conflict", kind: "conflict", message: conflictMessage })
    }));
    await dialog.getByRole("button", { name: "Restore meeting.md" }).click();
    await expect(dialog.getByRole("alert")).toHaveText(conflictMessage);
    await expect(dialog.getByText("meeting.md", { exact: true })).toBeVisible();
    await expect(dialog.getByRole("button", { name: "Restore meeting.md" })).toBeEnabled();
    await page.screenshot({ path: `test-results/trash-${viewport.name}-restore-conflict.png` });
    await page.unroute(restorePath);
    await dialog.getByRole("button", { name: "Restore meeting.md" }).click();
    await expect(dialog.getByText("Trash is empty.")).toBeVisible();
    current = item;
    await dialog.getByRole("button", { name: "Refresh" }).click();
    await dialog.getByRole("button", { name: "Permanently delete meeting.md" }).click();
    expect(deletionRequests).toBe(0);
    await page.screenshot({ path: `test-results/trash-${viewport.name}-confirm-delete.png` });
    await dialog.getByRole("button", { name: "Cancel" }).click();
    await dialog.getByRole("button", { name: "Permanently delete meeting.md" }).click();
    await dialog.getByRole("button", { name: "Permanently delete", exact: true }).click();
    await expect(dialog.getByText("Deletion queued · recovery unavailable")).toBeVisible();
    await expect(dialog.getByRole("button", { name: "Restore meeting.md" })).toBeDisabled();
    expect(deletionRequests).toBe(1);
  });
}
