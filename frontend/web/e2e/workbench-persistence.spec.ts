import { expect, test } from "@playwright/test";

import type { Me, RestNode, Space } from "../src/api/types";
import { routeJsonApi } from "./support/api";
import { usageResponse } from "./support/usage";
import { openSettings } from "./support/settings";

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

test("keeps the editor usable while resizing the viewport and restores preferred panel widths", async ({ page }) => {
  await page.setViewportSize({ width: 1440, height: 900 });
  await page.addInitScript(({ node, spaceId }) => {
    window.localStorage.setItem("notegate.lastActiveSpaceId", spaceId);
    window.localStorage.setItem(`notegate.workbench.v1.space.${spaceId}`, JSON.stringify({
      version: 1,
      spaceId,
      updatedAt: Date.now(),
      activeGroupIndex: 0,
      groups: [{ node, mode: "preview", back: [], forward: [] }]
    }));
  }, { node: savedNode, spaceId: savedSpace.id });
  await mockApi(page);
  await page.goto("/");

  const editor = page.locator('[data-editor-group][data-active="true"]');
  await expect(editor.getByRole("heading", { name: "Restored workbench" })).toBeVisible();
  const filesResize = page.getByRole("separator", { name: "Resize Files sidebar" });
  const inspectorResize = page.getByRole("separator", { name: "Resize Inspector" });
  await filesResize.focus();
  await filesResize.press("End");
  await inspectorResize.focus();
  await inspectorResize.press("End");
  await expect(filesResize).toHaveAttribute("aria-valuenow", "520");
  await expect(inspectorResize).toHaveAttribute("aria-valuenow", "520");

  await page.setViewportSize({ width: 900, height: 1024 });
  await expect.poll(async () => (await editor.boundingBox())?.width ?? 0).toBeGreaterThanOrEqual(320);
  await expect.poll(() => page.locator('#primary-sidebar-panel, [data-editor-group], #auxiliary-sidebar-panel').evaluateAll((panels) =>
    panels.every((panel) => {
      const bounds = panel.getBoundingClientRect();
      return bounds.width > 0 && bounds.left >= 0 && bounds.right <= window.innerWidth;
    })
  )).toBe(true);
  await expect(filesResize).toHaveAttribute("aria-valuenow", "256");
  await expect(inspectorResize).toHaveAttribute("aria-valuenow", "280");

  await page.setViewportSize({ width: 800, height: 1024 });
  const inspector = page.locator("#auxiliary-sidebar-panel");
  await expect(inspector).toHaveClass(/fixed/);
  await expect(inspectorResize).toHaveCount(0);
  await expect.poll(async () => (await editor.boundingBox())?.width ?? 0).toBeGreaterThanOrEqual(320);
  await expect.poll(async () => {
    const bounds = await inspector.boundingBox();
    return bounds !== null && bounds.x >= 0 && bounds.x + bounds.width <= 800;
  }).toBe(true);
  await page.getByRole("button", { name: "Close panel" }).click({ position: { x: 10, y: 10 } });
  await expect(inspector).toHaveCount(0);

  await page.setViewportSize({ width: 1440, height: 900 });
  await expect(filesResize).toHaveAttribute("aria-valuenow", "520");
  await expect(page.getByRole("button", { name: "Toggle right sidebar" })).toHaveAttribute("aria-pressed", "false");
  await page.getByRole("button", { name: "Toggle right sidebar" }).click();
  await expect(inspectorResize).toHaveAttribute("aria-valuenow", "520");
  await expect.poll(async () => (await inspector.boundingBox())?.width ?? 0).toBe(520);
  await expect(editor.getByRole("heading", { name: "Restored workbench" })).toBeVisible();
});

for (const width of [1440, 390]) {
  test(`keeps unsaved edits through Library and Settings visits at ${width}px`, async ({ page }) => {
    await page.setViewportSize({ width, height: 900 });
    await page.addInitScript(({ node, spaceId }) => {
      window.localStorage.setItem("notegate.lastActiveSpaceId", spaceId);
      window.localStorage.setItem(`notegate.workbench.v1.space.${spaceId}`, JSON.stringify({
        version: 1, spaceId, updatedAt: Date.now(), activeGroupIndex: 0,
        groups: [{ node, mode: "edit", back: [], forward: [] }]
      }));
    }, { node: savedNode, spaceId: savedSpace.id });
    await mockApi(page);
    await page.route(`**/api/v1/spaces/${spaces[0]!.id}/nodes/${savedNode.id}`, (route) =>
      route.fulfill({ status: 404, contentType: "application/json", body: JSON.stringify({ message: "Node not found", kind: "not_found" }) })
    );
    await page.goto("/");
    const editor = page.getByRole("textbox", { name: "Edit text content", includeHidden: true });
    await expect(editor).toHaveValue("# Restored workbench");
    const draft = "Keep this unsaved draft while browsing spaces and settings";
    await editor.fill(draft);

    const settings = await openSettings(page);
    await settings.getByRole("button", { name: "Close settings", exact: true }).click();
    await expect(editor).toHaveValue(draft);

    await page.getByRole("button", { name: "Open space library", exact: true }).click();
    const library = page.getByRole("tabpanel", { name: "Spaces 2", exact: true });
    await expect(library).toBeVisible();
    await expect(editor).toBeAttached();
    await expect(editor).toBeHidden();
    await editor.evaluate((element) => element.focus());
    await expect(editor).not.toBeFocused();
    expect(await library.evaluate((element) => element.scrollWidth <= element.clientWidth)).toBe(true);

    const librarySettings = await openSettings(page);
    await librarySettings.getByRole("button", { name: "Close settings", exact: true }).click();
    await expect(library).toBeVisible();
    const savedCard = library.getByRole("listitem").filter({
      has: page.getByRole("button", { name: `Inspect ${savedSpace.name}` })
    });
    await savedCard.getByRole("button", { name: "Open", exact: true }).click();
    await expect(editor).toBeVisible();
    await expect(editor).toHaveValue(draft);

    const firstSpace = page.getByRole("button", { name: spaces[0]!.name, exact: true });
    await firstSpace.click();
    await expect(firstSpace).toHaveAttribute("aria-current", "page");
    await expect(page.locator("header").getByText(`/ ${spaces[0]!.name}`, { exact: true })).toBeVisible();
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
  if (matchingSpace && url.pathname === `/api/v1/spaces/${matchingSpace.id}/link-index`) {
    return { status: "idle", availability: { can_trigger: true, reason: null, retry_at: null } };
  }
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
