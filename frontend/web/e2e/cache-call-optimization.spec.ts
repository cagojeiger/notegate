import { expect, test } from "@playwright/test";

import type { Me, RestNode, Space } from "../src/api/types";

const space: Space = {
  id: "space-1",
  name: "Performance fixture",
  sort_order: 0,
  permission: "write",
  root_node_id: "root-1",
  created_at: "2026-07-24T00:00:00Z",
  updated_at: "2026-07-24T00:00:00Z"
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
  test(`keeps active-space refresh requests constant on ${viewport.name}`, async ({ page }) => {
    const requests = new Map<string, number>();
    await page.clock.install();
    await page.setViewportSize({ width: viewport.width, height: viewport.height });
    await page.route("**/api/v1/**", async (route) => {
      const url = new URL(route.request().url());
      requests.set(url.pathname, (requests.get(url.pathname) ?? 0) + 1);
      await route.fulfill({
        status: 200,
        contentType: "application/json",
        body: JSON.stringify(responseFor(url))
      });
    });

    await page.goto("/");
    if (viewport.opensOverlay) {
      await page.getByRole("button", { name: "Toggle left sidebar" }).click();
    }

    const tree = page.getByRole("tree", { name: "Files" });
    await expect(tree).toBeVisible();
    await expect(tree.getByRole("button", { name: "note.md", exact: true })).toBeVisible();
    await expect.poll(() => count(requests, `/api/v1/spaces/${space.id}/file-change-sync`)).toBe(1);
    await expect.poll(() => count(requests, `/api/v1/spaces/${space.id}/nodes/${space.root_node_id}/children`)).toBe(1);
    await expect.poll(() => count(requests, `/api/v1/spaces/${space.id}/nodes`)).toBe(1);

    await page.clock.fastForward(36_000);

    await expect.poll(() => count(requests, `/api/v1/spaces/${space.id}/file-change-sync`)).toBe(2);
    expect(count(requests, `/api/v1/spaces/${space.id}/nodes/${space.root_node_id}/children`)).toBe(1);
    expect(count(requests, `/api/v1/spaces/${space.id}/nodes`)).toBe(1);
  });
}

function responseFor(url: URL) {
  if (url.pathname === "/api/v1/me") return me;
  if (url.pathname === "/api/v1/spaces") {
    return {
      spaces: [space],
      page: { limit: 100, returned: 1, has_more: false, next_cursor: null }
    };
  }
  if (url.pathname === `/api/v1/spaces/${space.id}/nodes/${space.root_node_id}/children`) {
    return {
      parent: { id: space.root_node_id, path: "/" },
      children: [textNode()],
      page: { limit: 100, returned: 1, has_more: false, next_cursor: null }
    };
  }
  if (url.pathname === `/api/v1/spaces/${space.id}/nodes`) {
    return {
      nodes: [textNode()],
      page: { limit: 50, returned: 1, has_more: false, next_cursor: null }
    };
  }
  if (url.pathname === `/api/v1/spaces/${space.id}/file-change-sync`) {
    return {
      changes: [],
      next_after_id: 10,
      has_more: false,
      resync_required: false
    };
  }
  throw new Error(`Unhandled API request: ${url.pathname}${url.search}`);
}

function textNode(): RestNode {
  return {
    id: "text-1",
    space_id: space.id,
    parent_id: space.root_node_id,
    name: "note.md",
    kind: "text",
    path: "/note.md",
    sort_order: 0,
    metadata: {},
    has_children: false,
    content_sha256: "sha-1",
    created_by: { id: "user-1", kind: "user", display_name: "User" },
    updated_by: { id: "user-1", kind: "user", display_name: "User" },
    created_at: "2026-07-24T00:00:00Z",
    updated_at: "2026-07-24T00:00:00Z"
  };
}

function count(requests: Map<string, number>, path: string): number {
  return requests.get(path) ?? 0;
}
