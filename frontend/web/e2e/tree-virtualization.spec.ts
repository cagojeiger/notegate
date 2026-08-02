import { expect, test } from "@playwright/test";

import type { Me, RestNode, Space } from "../src/api/types";
import { routeWorkbenchJsonApi } from "./support/linkIndex";
import { usageResponse } from "./support/usage";

const space: Space = {
  id: "space-1",
  name: "Large tree",
  sort_order: 0,
  navigation_pinned: true,
  user_mcp_enabled: true,
  default_search_enabled: true,
  default_text_encryption_enabled: false,
  features: { text_encryption: true, write_lock: true },
  permission: "write",
  root_node_id: "root-1",
  created_at: "2026-07-01T00:00:00Z",
  updated_at: "2026-07-01T00:00:00Z"
};

const me: Me = {
  account: { id: "user-1", kind: "user", display_name: "User" },
  user: { email: "user@example.com" },
  capabilities: { can_create_space: true, can_manage_agents: true }
};

for (const viewport of [
  { name: "desktop", width: 1440, height: 900, opensOverlay: false },
  { name: "tablet", width: 900, height: 1024, opensOverlay: false },
  { name: "mobile", width: 390, height: 844, opensOverlay: true }
]) {
  test(`large folders stay virtualized on ${viewport.name}`, async ({ page }) => {
    const children = Array.from({ length: 1_000 }, (_, index) => fileNode(index));
    let childRequestCount = 0;
    await page.setViewportSize({ width: viewport.width, height: viewport.height });
    await routeWorkbenchJsonApi(page, (url) => {
      if (url.pathname === `/api/v1/spaces/${space.id}/nodes/${space.root_node_id}/children`) {
        childRequestCount += 1;
      }
      return responseFor(url, children);
    });

    await page.goto("/");
    if (viewport.opensOverlay) {
      await page.getByRole("button", { name: "Toggle left sidebar" }).click();
    }
    const tree = page.getByRole("tree", { name: "Files" });
    await expect(tree).toBeVisible();
    await expect(page.getByRole("button", { name: "file-0000.bin" })).toBeVisible();
    await expect.poll(() => childRequestCount).toBe(1);
    await expect.poll(() => page.locator("[data-node-row]").count()).toBeLessThan(60);

    const filesSeparator = page.getByRole("separator", { name: "Resize Files section" });
    const sidebarSeparator = page.getByRole("separator", { name: "Resize Files sidebar" });
    await expect(filesSeparator).toHaveAttribute("aria-valuenow", "67");
    await filesSeparator.focus();
    await filesSeparator.press("ArrowDown");
    await expect(filesSeparator).toHaveAttribute("aria-valuenow", "72");
    const filesSeparatorBox = await filesSeparator.boundingBox();
    expect(filesSeparatorBox?.height).toBeGreaterThanOrEqual(24);
    if (!viewport.opensOverlay) {
      const sidebarBox = await sidebarSeparator.boundingBox();
      expect(sidebarBox?.width).toBeGreaterThanOrEqual(24);
    }

    await page.getByRole("button", { name: "file-0000.bin" }).focus();
    for (let index = 1; index <= 20; index += 1) {
      await page.keyboard.press("ArrowDown");
      await expect(page.getByRole("button", { name: fileName(index) })).toBeFocused();
    }
    await tree.evaluate((element) => {
      element.scrollTop = element.scrollHeight;
      element.dispatchEvent(new Event("scroll"));
    });
    await expect(page.getByRole("button", { name: "file-0020.bin" })).toBeFocused();
    await page.getByRole("button", { name: "Toggle theme" }).focus();

    for (let attempt = 0; attempt < 10 && childRequestCount < 10; attempt += 1) {
      const previousRequestCount = childRequestCount;
      await scrollToTreeBottom(tree);
      await expect.poll(() => childRequestCount).toBeGreaterThan(previousRequestCount);
      await expect.poll(() => page.locator("[data-node-row]").count()).toBeLessThan(60);
    }

    expect(childRequestCount).toBe(10);
    await scrollToTreeBottom(tree);
    await expect(page.getByRole("button", { name: "file-0999.bin" })).toBeVisible();
    await expect.poll(() => page.locator("[data-node-row]").count()).toBeLessThan(60);

    await tree.evaluate((element) => {
      element.scrollTop = 0;
      element.dispatchEvent(new Event("scroll"));
    });
    await expect(page.getByRole("button", { name: "file-0000.bin" })).toBeVisible();
    const recentButton = page.locator("[data-recent-list] [data-node-open]").first();
    await recentButton.focus();
    await page.keyboard.press("ArrowUp");
    await expect(page.getByRole("button", { name: "file-0999.bin" })).toBeFocused();
    await page.keyboard.press("ArrowDown");
    await expect(recentButton).toBeFocused();
  });
}

function responseFor(url: URL, children: RestNode[]) {
  if (url.pathname === "/api/v1/me") return me;
  if (url.pathname === "/api/v1/me/usage") return usageResponse(space, children.length);
  if (url.pathname === "/api/v1/spaces") {
    return { spaces: [space], page: { limit: 100, returned: 1, has_more: false, next_cursor: null } };
  }
  if (url.pathname === `/api/v1/spaces/${space.id}/nodes/${space.root_node_id}/children`) {
    const start = Number(url.searchParams.get("cursor") ?? 0);
    const pageSize = 100;
    const pageChildren = children.slice(start, start + pageSize);
    const next = start + pageChildren.length;
    return {
      parent: { id: space.root_node_id, path: "/" },
      children: pageChildren,
      page: {
        limit: pageSize,
        returned: pageChildren.length,
        has_more: next < children.length,
        next_cursor: next < children.length ? String(next) : null
      }
    };
  }
  if (url.pathname === `/api/v1/spaces/${space.id}/nodes`) {
    return {
      nodes: [recentNode()],
      page: { limit: 50, returned: 1, has_more: false, next_cursor: null }
    };
  }
  if (url.pathname === `/api/v1/spaces/${space.id}/file-change-sync`) {
    return {
      changes: [],
      next_after_id: 0,
      has_more: false,
      resync_required: false
    };
  }
  throw new Error(`Unhandled API request: ${url.pathname}${url.search}`);
}

async function scrollToTreeBottom(tree: import("@playwright/test").Locator) {
  await tree.evaluate((element) => {
    element.scrollTop = element.scrollHeight;
    element.dispatchEvent(new Event("scroll"));
  });
}

function fileNode(index: number): RestNode {
  const suffix = index.toString().padStart(4, "0");
  const name = fileName(index);
  return {
    id: `file-${suffix}`,
    space_id: space.id,
    parent_id: space.root_node_id,
    name,
    kind: "file",
    path: `/${name}`,
    sort_order: index,
    metadata: {},
    search_enabled: true,
    write_locked: false,
    write_lock_sources: [],
    has_children: false,
    effective_write_locked: false,
    byte_len: index,
    media_type: "application/octet-stream",
    created_by: { id: "user-1", kind: "user", display_name: "User" },
    updated_by: { id: "user-1", kind: "user", display_name: "User" },
    created_at: "2026-07-01T00:00:00Z",
    updated_at: "2026-07-01T00:00:00Z"
  };
}

function fileName(index: number): string {
  return `file-${index.toString().padStart(4, "0")}.bin`;
}

function recentNode(): RestNode {
  return {
    ...fileNode(1_000),
    id: "recent-1",
    name: "recent-entry.md",
    kind: "text",
    path: "/recent-entry.md"
  };
}
