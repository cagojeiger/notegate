import { expect, test } from "@playwright/test";

import type { Me, RestNode, Space } from "../src/api/types";
import { routeJsonApi } from "./support/api";
import { usageResponse } from "./support/usage";

const space: Space = {
  id: "space-1",
  name: "Performance fixture",
  sort_order: 0,
  navigation_pinned: true,
  user_mcp_enabled: true,
  default_search_enabled: true,
  default_text_encryption_enabled: false,
  features: { text_encryption: true, write_lock: true },
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
  test(`backs off idle active-space refreshes on ${viewport.name}`, async ({ page }) => {
    const requests = new Map<string, number>();
    await page.clock.install();
    await page.addInitScript(() => {
      Math.random = () => 0.5;
    });
    await page.setViewportSize({ width: viewport.width, height: viewport.height });
    await routeJsonApi(page, (url) => {
      requests.set(url.pathname, (requests.get(url.pathname) ?? 0) + 1);
      return responseFor(url);
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

    for (const [elapsedMs, expectedRequests] of [
      [30_001, 2],
      [60_001, 3],
      [120_001, 4],
      [300_001, 5],
      [300_001, 6]
    ] as const) {
      await page.clock.fastForward(elapsedMs);
      await expect.poll(() => count(
        requests,
        `/api/v1/spaces/${space.id}/file-change-sync`
      )).toBe(expectedRequests);
    }

    expect(count(requests, `/api/v1/spaces/${space.id}/nodes/${space.root_node_id}/children`)).toBe(1);
    expect(count(requests, `/api/v1/spaces/${space.id}/nodes`)).toBe(1);

    if (viewport.name === "desktop") {
      await page.context().setOffline(true);
      await page.context().setOffline(false);
      expect(count(requests, `/api/v1/spaces/${space.id}/file-change-sync`)).toBe(6);

      await page.clock.fastForward(30_001);
      await expect.poll(() => count(
        requests,
        `/api/v1/spaces/${space.id}/file-change-sync`
      )).toBe(7);
    }
  });
}

test("opens a recent node with one reveal request and no canonical node request", async ({ page }) => {
  const requests = new Map<string, number>();
  await page.setViewportSize({ width: 1440, height: 900 });
  await routeJsonApi(page, (url) => {
    requests.set(url.pathname, (requests.get(url.pathname) ?? 0) + 1);
    return responseFor(url);
  });

  await page.goto("/");
  await page.locator("[data-recent-list]").getByRole("button", { name: "note.md" }).click();

  const editor = page.locator('[data-editor-group][data-active="true"]');
  await expect(editor).toContainText("note.md");
  await expect(editor).toContainText("Request budget");
  await expect.poll(() => count(
    requests,
    `/api/v1/spaces/${space.id}/nodes/text-1/reveal`
  )).toBe(1);
  await expect.poll(() => count(
    requests,
    `/api/v1/spaces/${space.id}/text/text-1`
  )).toBe(1);
  expect(count(requests, `/api/v1/spaces/${space.id}/nodes/text-1`)).toBe(0);
});

test("keeps one usage polling owner while the desktop Space Library is open", async ({ page }) => {
  const requests = new Map<string, number>();
  await page.clock.install();
  await page.setViewportSize({ width: 1440, height: 900 });
  await routeJsonApi(page, (url) => {
    requests.set(url.pathname, (requests.get(url.pathname) ?? 0) + 1);
    return responseFor(url);
  });

  await page.goto("/");
  await expect.poll(() => count(requests, "/api/v1/me/usage")).toBe(1);

  await page.clock.fastForward(10_000);
  await page.getByRole("button", { name: "Open space library" }).click();
  await expect(page.getByRole("heading", { name: /Spaces 1/ })).toBeVisible();

  await page.clock.fastForward(61_000);

  await expect.poll(() => count(requests, "/api/v1/me/usage")).toBe(2);
});

function responseFor(url: URL) {
  if (url.pathname === "/api/v1/me") return me;
  if (url.pathname === "/api/v1/me/usage") return usageResponse(space);
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
  if (url.pathname === `/api/v1/spaces/${space.id}/nodes/text-1/reveal`) {
    return { ancestors: [], target: textNode() };
  }
  if (url.pathname === `/api/v1/spaces/${space.id}/nodes/text-1`) {
    return textNode();
  }
  if (url.pathname === `/api/v1/spaces/${space.id}/text/text-1`) {
    return {
      node: { id: "text-1", path: "/note.md" },
      text: {
        node_id: "text-1",
        storage_format: "plain",
        content: "# Request budget",
        content_sha256: "sha-1",
        byte_len: 16,
        line_count: 1,
        start_line: 1,
        end_line: 1,
        returned_lines: 1,
        truncated: false,
        next_start_line: null,
        updated_by: me.account,
        updated_at: "2026-07-24T00:00:00Z"
      }
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
    search_enabled: true,
    write_locked: false,
    write_lock_sources: [],
    has_children: false,
    effective_write_locked: false,
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
