import { expect, test } from "@playwright/test";
import AxeBuilder from "@axe-core/playwright";

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
  for (const theme of ["light", "dark"] as const) {
    test(`compact trash selects one item on ${viewport.name} in ${theme} mode`, async ({ page }, testInfo) => {
      await page.setViewportSize(viewport);
      await page.addInitScript((value) => window.localStorage.setItem("notegate.theme", value), theme);
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
      const listBounds = await dialog.getByRole("list", { name: "Deleted items" }).boundingBox();
      const detailBounds = await dialog.getByRole("region", { name: "Details for reports" }).boundingBox();
      if (!listBounds || !detailBounds) throw new Error("Trash panes missing");
      if (viewport.name === "desktop") expect(detailBounds.x).toBeGreaterThan(listBounds.x + listBounds.width);
      else expect(detailBounds.y).toBeGreaterThan(listBounds.y + listBounds.height);
      expect(await dialog.evaluate((element) => element.scrollWidth <= element.clientWidth)).toBe(true);
      await expect(page.locator("html")).toHaveAttribute("data-theme", theme);
      const accessibility = await new AxeBuilder({ page }).include('[role="dialog"]').withTags(["wcag2a", "wcag2aa", "wcag21a", "wcag21aa", "wcag22aa"]).analyze();
      expect(accessibility.violations, JSON.stringify(accessibility.violations, null, 2)).toEqual([]);
      await testInfo.attach(`trash-${viewport.name}-${theme}`, { body: await page.screenshot({ path: `test-results/trash-${viewport.name}-${theme}.png` }), contentType: "image/png" });
      await dialog.getByRole("button", { name: "Select old.png" }).click();
      await expect(dialog.getByRole("button", { name: "Restore old.png" })).toBeDisabled();
      await expect(dialog.getByRole("button", { name: "Permanently delete old.png" })).toBeDisabled();
    });
  }

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
    await expect(dialog.getByRole("button", { name: "Select meeting.md" })).toBeVisible();
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
    await expect(dialog.getByRole("button", { name: "Select meeting.md" })).toBeVisible();
    await expect(dialog.getByRole("button", { name: "Restore meeting.md" })).toBeEnabled();
    await page.screenshot({ path: `test-results/trash-${viewport.name}-restore-conflict.png` });
    await page.unroute(restorePath);
    await dialog.getByRole("button", { name: "Restore meeting.md" }).click();
    await expect(dialog.getByText("Trash is empty.")).toBeVisible();
    current = item;
    await dialog.getByRole("button", { name: "Refresh" }).click();
    await dialog.getByRole("button", { name: "Select meeting.md" }).click();
    await dialog.getByRole("button", { name: "Permanently delete meeting.md" }).click();
    expect(deletionRequests).toBe(0);
    await expect(dialog.getByRole("button", { name: "Cancel" })).toBeFocused();
    await expect(dialog.getByRole("region", { name: "Confirm permanent deletion" })).toContainText("/notes/meeting.md");
    await page.screenshot({ path: `test-results/trash-${viewport.name}-confirm-delete.png` });
    await dialog.getByRole("button", { name: "Cancel" }).click();
    await expect(dialog.getByRole("button", { name: "Permanently delete meeting.md" })).toBeFocused();
    await dialog.getByRole("button", { name: "Permanently delete meeting.md" }).click();
    await dialog.getByRole("button", { name: "Permanently delete", exact: true }).click();
    await expect(dialog.getByText("Deletion queued · recovery unavailable")).toBeVisible();
    await expect(dialog.getByRole("button", { name: "Restore meeting.md" })).toBeDisabled();
    expect(deletionRequests).toBe(1);
  });

  test(`trash preserves loaded pages and scroll after restoration on ${viewport.name}`, async ({ page }, testInfo) => {
    await page.setViewportSize(viewport);
    let items = Array.from({ length: 60 }, (_, index) => ({ ...item, id: `node-${index}`, name: `note-${index}.md`, path: `/notes/note-${index}.md` }));
    let nextPageRequests = 0;
    const meta = { limit: 50, returned: 0, has_more: false, next_cursor: null };
    await routeJsonApi(page, (url) => {
      if (url.pathname === "/api/v1/me") return { account: { id: "user-1", kind: "user", display_name: "User" }, user: { email: "user@example.com" }, capabilities: { can_create_space: true, can_manage_agents: true } };
      if (url.pathname === "/api/v1/me/usage") return usageResponse(space);
      if (url.pathname === "/api/v1/spaces") return { spaces: [space], page: meta };
      if (url.pathname === "/api/v1/me/trash") {
        const later = url.searchParams.get("cursor") === "next-page";
        if (later) nextPageRequests += 1;
        const entries = later ? items.slice(50) : items.slice(0, 50);
        return { items: entries, page: { ...meta, returned: entries.length, has_more: !later, next_cursor: later ? null : "next-page" } };
      }
      if (url.pathname.endsWith("/nodes/node-55/restore")) { items = items.filter((entry) => entry.id !== "node-55"); return {}; }
      if (url.pathname.endsWith("/children")) return { parent: { id: space.root_node_id, path: "/" }, children: [], page: meta };
      if (url.pathname.endsWith("/nodes")) return { nodes: [], page: meta };
      if (url.pathname.endsWith("/file-change-sync")) return { changes: [], next_after_id: 0, has_more: false, resync_required: false };
      throw new Error(`Unhandled request: ${url.pathname}`);
    });
    await page.goto("/");
    await page.getByRole("button", { name: "Trash", exact: true }).click();
    const dialog = page.getByRole("dialog", { name: "Trash" });
    const list = dialog.getByRole("list", { name: "Deleted items" });
    await expect(list.getByRole("button")).toHaveCount(50);
    const initialScrollHeight = await dialog.evaluate((element) => element.scrollHeight);
    if (viewport.name === "desktop") {
      const geometry = await dialog.evaluate((element) => ({
        height: element.clientHeight,
        scrollHeight: element.scrollHeight,
        scrollTop: element.scrollTop
      }));
      await testInfo.attach("trash-long-list-geometry", { body: JSON.stringify(geometry), contentType: "application/json" });
      await page.screenshot({ path: "test-results/trash-desktop-long-list.png" });
      expect.soft(geometry.scrollHeight, "Only the list should scroll when the desktop panes fit").toBeLessThanOrEqual(geometry.height + 1);
    }
    await dialog.getByRole("button", { name: "Load more" }).click();
    await expect(list.getByRole("button")).toHaveCount(60);
    expect(await dialog.evaluate((element) => element.scrollHeight), "Loading another page must not create blank space outside the list").toBeLessThanOrEqual(initialScrollHeight + 1);
    await list.evaluate((element) => { element.scrollTop = element.scrollHeight; });
    const outerScrollTop = await dialog.evaluate((element) => element.scrollTop);
    await list.hover();
    await page.mouse.wheel(0, 600);
    await page.evaluate(() => new Promise<void>((resolve) => requestAnimationFrame(() => requestAnimationFrame(() => resolve()))));
    expect(await dialog.evaluate((element) => element.scrollTop), "Wheeling at the list boundary must not move the outer dialog").toBe(outerScrollTop);
    await page.screenshot({ path: `test-results/trash-${viewport.name}-list-end.png` });
    await dialog.getByRole("button", { name: "Select note-55.md" }).click();
    const scrollTop = await list.evaluate((element) => element.scrollTop);
    expect(scrollTop).toBeGreaterThan(0);
    await dialog.getByRole("button", { name: "Permanently delete note-55.md" }).click();
    await dialog.getByRole("button", { name: "Cancel" }).click();
    expect(await list.evaluate((element) => element.scrollTop)).toBe(scrollTop);
    if (viewport.name === "desktop") {
      expect.soft(await dialog.evaluate((element) => element.scrollTop), "Selecting or confirming an item must not scroll the modal heading away").toBe(0);
      await expect.soft(dialog.getByRole("heading", { name: "Trash", exact: true })).toBeInViewport();
    }
    await dialog.getByRole("button", { name: "Restore note-55.md" }).click();
    await expect(list.getByRole("button", { name: "Select note-55.md" })).toHaveCount(0);
    await expect(list.getByRole("button", { name: "Select note-59.md" })).toHaveCount(1);
    await expect(dialog.getByRole("status")).toContainText("Restored “note-55.md”");
    await expect(dialog.getByRole("button", { name: "Restore note-0.md" })).toHaveCount(0);
    expect(await list.evaluate((element) => element.scrollTop)).toBeGreaterThan(0);
    expect(nextPageRequests).toBe(2);
  });
}
