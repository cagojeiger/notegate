import { expect, test } from "@playwright/test";

import type { Me, RestNode, Space } from "../src/api/types";
import { routeJsonApi } from "./support/api";
import { usageResponse } from "./support/usage";

const me: Me = {
  account: { id: "user-1", kind: "user", display_name: "User" },
  user: { email: "user@example.com" },
  capabilities: { can_create_space: true, can_manage_agents: true }
};

const spaces: Space[] = [
  space("space-1", "First Space", "root-1", 0),
  space("space-2", "Saved Space", "root-2", 1)
];
const savedSpace = spaces[1]!;
const savedNode: RestNode = {
  id: "saved-note",
  space_id: savedSpace.id,
  parent_id: savedSpace.root_node_id,
  name: "persisted-note.md",
  kind: "text",
  path: "/persisted-note.md",
  sort_order: 0,
  metadata: { source: "saved-workbench" },
  external_access_enabled: true,
  write_locked: false,
  write_lock_sources: [],
  has_children: false,
  effective_write_locked: false,
  byte_len: 25,
  line_count: 1,
  content_sha256: "sha-persisted-note",
  text_storage_format: "plain",
  text_at_rest_encryption: "none",
  created_by: me.account,
  updated_by: me.account,
  created_at: "2026-07-28T00:00:00Z",
  updated_at: "2026-07-28T00:00:00Z"
};

test("restores the saved workbench before showing the app and persists panel toggles", async ({ page }) => {
  await page.setViewportSize({ width: 1440, height: 900 });
  await page.addInitScript(({ node, spaceId }) => {
    window.localStorage.setItem("notegate.theme", "dark");
    window.localStorage.setItem("notegate.lastActiveSpaceId", spaceId);
    window.localStorage.setItem("notegate.workbenchPanels.v1", JSON.stringify({
      version: 1,
      primarySidebarOpen: false,
      auxiliaryOpen: false
    }));
    window.localStorage.setItem(`notegate.workbench.v1.space.${spaceId}`, JSON.stringify({
      version: 1,
      spaceId,
      updatedAt: Date.now(),
      activeGroupIndex: 0,
      groups: [{
        node,
        mode: "preview",
        back: [],
        forward: []
      }]
    }));
  }, { node: savedNode, spaceId: savedSpace.id });
  await mockApi(page);

  await page.goto("/");

  await expect(page.locator("html")).toHaveAttribute("data-theme", "dark");
  await expect(
    page
      .getByRole("complementary", { name: "Space navigation" })
      .getByRole("button", { name: savedSpace.name, exact: true })
  ).toHaveAttribute("aria-current", "page");

  const primaryToggle = page.getByRole("button", { name: "Toggle left sidebar" });
  const auxiliaryToggle = page.getByRole("button", { name: "Toggle right sidebar" });
  await expect(primaryToggle).toHaveAttribute("aria-pressed", "false");
  await expect(auxiliaryToggle).toHaveAttribute("aria-pressed", "false");
  await expect(page.locator("#primary-sidebar-panel")).toHaveCount(0);
  await expect(page.getByText("Inspector", { exact: true })).toHaveCount(0);

  const activeEditor = page.locator('[data-editor-group][data-active="true"]');
  await expect(activeEditor).toContainText(savedNode.name);
  await expect(activeEditor.getByRole("heading", { name: "Restored workbench" })).toBeVisible();

  await primaryToggle.click();
  await expect.poll(() => storedPanelState(page)).toEqual({
    version: 1,
    primarySidebarOpen: true,
    auxiliaryOpen: false
  });

  await auxiliaryToggle.click();
  await expect.poll(() => storedPanelState(page)).toEqual({
    version: 1,
    primarySidebarOpen: true,
    auxiliaryOpen: true
  });
});

for (const indexedSpaces of [2, 20]) {
  test(`profiles editor persistence with ${indexedSpaces} indexed spaces`, async ({ page }) => {
    test.skip(!process.env.NOTEGATE_PROFILE_WORKBENCH, "Run against the production bundle in CI");
    await page.setViewportSize({ width: 1440, height: 900 });
    await page.addInitScript(({ node, spaceId, indexedSpaces: count }) => {
      window.localStorage.setItem("notegate.lastActiveSpaceId", spaceId);
      window.localStorage.setItem(`notegate.workbench.v1.space.${spaceId}`, JSON.stringify({
        version: 1,
        spaceId,
        updatedAt: Date.now(),
        activeGroupIndex: 0,
        groups: [{ node, mode: "preview", back: [], forward: [] }]
      }));
      window.localStorage.setItem("notegate.workbench.v1.index", JSON.stringify({
        version: 1,
        spaces: [
          { spaceId: "space-1", updatedAt: 2 },
          { spaceId, updatedAt: 1 },
          ...Array.from({ length: count - 2 }, (_, index) => ({
            spaceId: `older-space-${index}`,
            updatedAt: -index
          }))
        ]
      }));

      const storageCalls: { operation: string; durationMs: number; bytes: number }[] = [];
      Reflect.set(window, "__workbenchStorageCalls", storageCalls);
      Reflect.set(window, "__measureWorkbenchStorage", true);
      const originalGet = Storage.prototype.getItem;
      const originalSet = Storage.prototype.setItem;
      const originalRemove = Storage.prototype.removeItem;
      Storage.prototype.getItem = function (key) {
        const start = performance.now();
        try { return originalGet.call(this, key); } finally {
          if (key.startsWith("notegate.workbench.v1.")) storageCalls.push({ operation: "read", durationMs: performance.now() - start, bytes: 0 });
        }
      };
      Storage.prototype.setItem = function (key, value) {
        const start = performance.now();
        try { return originalSet.call(this, key, value); } finally {
          if (key.startsWith("notegate.workbench.v1.")) storageCalls.push({ operation: "write", durationMs: performance.now() - start, bytes: value.length });
        }
      };
      Storage.prototype.removeItem = function (key) {
        const start = performance.now();
        try { return originalRemove.call(this, key); } finally {
          if (key.startsWith("notegate.workbench.v1.")) storageCalls.push({ operation: "remove", durationMs: performance.now() - start, bytes: 0 });
        }
      };
    }, { node: savedNode, spaceId: savedSpace.id, indexedSpaces });
    await mockApi(page);
    await page.goto("/");
    const editor = page.locator('[data-editor-group][data-active="true"]');
    await expect(editor.getByRole("heading", { name: "Restored workbench" })).toBeVisible();
    await page.evaluate(() => {
      performance.clearMeasures("notegate-workbench-persist");
      (Reflect.get(window, "__workbenchStorageCalls") as unknown[]).length = 0;
    });

    for (let iteration = 0; iteration < 10; iteration += 1) {
      await editor.getByRole("button", { name: "Edit", exact: true }).click();
      await expect(editor.getByRole("button", { name: "Cancel edit" })).toBeVisible();
      await editor.getByRole("button", { name: "Cancel edit" }).click();
      await expect(editor.getByRole("button", { name: "Edit", exact: true })).toBeVisible();
    }

    await expect.poll(() => page.evaluate(() =>
      performance.getEntriesByName("notegate-workbench-persist", "measure").length
    )).toBeGreaterThanOrEqual(20);

    const metrics = await page.evaluate(() => {
      const durations = performance.getEntriesByName("notegate-workbench-persist", "measure")
        .map((entry) => entry.duration)
        .sort((a, b) => a - b);
      const calls = Reflect.get(window, "__workbenchStorageCalls") as {
        operation: string; durationMs: number; bytes: number
      }[];
      const writes = calls.filter((call) => call.operation === "write");
      const percentile = (values: number[], fraction: number) => values[Math.ceil(values.length * fraction) - 1] ?? 0;
      return {
        saves: durations.length,
        saveMedianMs: percentile(durations, 0.5),
        saveP95Ms: percentile(durations, 0.95),
        saveMaxMs: durations.at(-1) ?? 0,
        storageReads: calls.filter((call) => call.operation === "read").length,
        storageWrites: writes.length,
        storageRemoves: calls.filter((call) => call.operation === "remove").length,
        storageWriteMs: writes.reduce((sum, call) => sum + call.durationMs, 0),
        storageWrittenChars: writes.reduce((sum, call) => sum + call.bytes, 0)
      };
    });
    expect(metrics.saves).toBeGreaterThanOrEqual(20);
    console.log(`WORKBENCH_STORAGE_METRIC ${JSON.stringify({ indexedSpaces, editCycles: 10, ...metrics })}`);
  });
}

async function mockApi(page: import("@playwright/test").Page) {
  await routeJsonApi(page, (url) => responseFor(url));
}

function responseFor(url: URL) {
  if (url.pathname === "/api/v1/me") return me;
  if (url.pathname === "/api/v1/me/usage") return usageResponse(savedSpace);
  if (url.pathname === "/api/v1/spaces") {
    return { spaces, page: pageInfo(spaces.length) };
  }
  if (url.pathname === `/api/v1/spaces/${savedSpace.id}/nodes/${savedNode.id}`) {
    return savedNode;
  }
  if (url.pathname === `/api/v1/spaces/${savedSpace.id}/text/${savedNode.id}`) {
    return {
      node: { id: savedNode.id, path: savedNode.path },
      text: {
        node_id: savedNode.id,
        storage_format: "plain",
        content: "# Restored workbench",
        content_sha256: savedNode.content_sha256,
        byte_len: savedNode.byte_len,
        line_count: savedNode.line_count,
        start_line: 1,
        end_line: 1,
        returned_lines: 1,
        truncated: false,
        next_start_line: null,
        updated_by: me.account,
        updated_at: savedNode.updated_at
      }
    };
  }

  const matchingSpace = spaces.find((candidate) => url.pathname.startsWith(`/api/v1/spaces/${candidate.id}/`));
  if (matchingSpace && url.pathname === `/api/v1/spaces/${matchingSpace.id}/nodes/${matchingSpace.root_node_id}/children`) {
    return {
      parent: { id: matchingSpace.root_node_id, path: "/" },
      children: matchingSpace.id === savedSpace.id ? [savedNode] : [],
      page: pageInfo(matchingSpace.id === savedSpace.id ? 1 : 0)
    };
  }
  if (matchingSpace && url.pathname === `/api/v1/spaces/${matchingSpace.id}/nodes`) {
    return {
      nodes: matchingSpace.id === savedSpace.id ? [savedNode] : [],
      page: pageInfo(matchingSpace.id === savedSpace.id ? 1 : 0)
    };
  }
  if (matchingSpace && url.pathname === `/api/v1/spaces/${matchingSpace.id}/file-change-sync`) {
    return { changes: [], next_after_id: 0, has_more: false, resync_required: false };
  }
  throw new Error(`Unhandled API request: ${url.pathname}${url.search}`);
}

async function storedPanelState(page: import("@playwright/test").Page) {
  return page.evaluate(() => JSON.parse(
    window.localStorage.getItem("notegate.workbenchPanels.v1") ?? "null"
  ) as unknown);
}

function space(id: string, name: string, rootNodeId: string, sortOrder: number): Space {
  return {
    id,
    name,
    sort_order: sortOrder,
    navigation_pinned: true,
    user_mcp_enabled: true,
    default_external_access_enabled: true,
    default_text_encryption_enabled: false,
    features: { text_encryption: true, write_lock: true },
    permission: "write",
    root_node_id: rootNodeId,
    created_at: "2026-07-28T00:00:00Z",
    updated_at: "2026-07-28T00:00:00Z"
  };
}

function pageInfo(returned: number) {
  return { limit: 100, returned, has_more: false, next_cursor: null };
}
