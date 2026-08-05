import { expect, test } from "@playwright/test";

import type { Me, RestNode, Space } from "../src/api/types";
import { routeWorkbenchJsonApi } from "./support/linkIndex";
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
  search_enabled: true,
  write_locked: false,
  write_lock_sources: [],
  has_children: false,
  revision: 1,
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

async function mockApi(page: import("@playwright/test").Page) {
  await routeWorkbenchJsonApi(page, (url) => responseFor(url));
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
    default_search_enabled: true,
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
