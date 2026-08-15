import { expect, test } from "@playwright/test";

import type { Me, RestNode, Space } from "../src/api/types";
import { expectNoAccessibilityViolations } from "./support/accessibility";
import { routeJsonApi } from "./support/api";
import { usageResponse } from "./support/usage";

const me: Me = {
  account: { id: "user-1", kind: "user", display_name: "User" },
  user: { email: "user@example.com" },
  capabilities: { can_create_space: true, can_manage_agents: true }
};

const space: Space = {
  id: "space-1",
  name: "Data",
  sort_order: 0,
  navigation_pinned: true,
  user_mcp_enabled: true,
  default_search_enabled: true,
  default_text_encryption_enabled: false,
  features: { text_encryption: true, write_lock: true },
  permission: "write",
  root_node_id: "root-1",
  created_at: "2026-08-02T00:00:00Z",
  updated_at: "2026-08-02T00:00:00Z"
};

const csvContent = largeCsv();
const csvNode: RestNode = {
  id: "large-csv",
  space_id: space.id,
  parent_id: space.root_node_id,
  name: "large-table.csv",
  kind: "text",
  path: "/large-table.csv",
  sort_order: 0,
  metadata: {},
  search_enabled: true,
  write_locked: false,
  write_lock_sources: [],
  has_children: false,
  effective_write_locked: false,
  byte_len: Buffer.byteLength(csvContent),
  line_count: csvContent.split("\n").length,
  content_sha256: "sha-large-csv",
  text_storage_format: "plain",
  text_at_rest_encryption: "none",
  created_by: me.account,
  updated_by: me.account,
  created_at: "2026-08-02T00:00:00Z",
  updated_at: "2026-08-02T00:00:00Z"
};

for (const viewport of [
  { name: "desktop light", width: 1440, height: 900, mobile: false, theme: "light" },
  { name: "tablet dark", width: 900, height: 1024, mobile: false, theme: "dark" },
  { name: "mobile light", width: 390, height: 844, mobile: true, theme: "light" }
] as const) {
  test(`CSV table stays virtualized and scrollable on ${viewport.name}`, async ({ page }) => {
    await page.setViewportSize({ width: viewport.width, height: viewport.height });
    await page.addInitScript((theme) => window.localStorage.setItem("notegate.theme", theme), viewport.theme);
    await mockApi(page);
    await page.goto("/");

    if (viewport.mobile) {
      await page.getByRole("button", { name: "Toggle left sidebar" }).click();
    }
    await page.getByRole("button", { name: csvNode.name }).first().click();

    if (viewport.mobile) {
      const header = page.locator("[data-editor-group-header]");
      const copyPath = page.getByRole("button", { name: "Copy path" });
      const tableView = page.getByRole("button", { name: "Table", exact: true });
      const sourceView = page.getByRole("button", { name: "Source", exact: true });
      await expect.poll(() => header.evaluate((element) => Math.round(element.getBoundingClientRect().height))).toBe(48);
      await expect.poll(() => copyPath.evaluate((element) => Math.floor(element.getBoundingClientRect().width))).toBeGreaterThanOrEqual(24);
      for (const control of [tableView, sourceView]) {
        const box = await control.boundingBox();
        expect(box?.height).toBeGreaterThanOrEqual(44);
        expect(box?.width).toBeGreaterThanOrEqual(44);
      }

      const moreActions = page.getByRole("button", { name: "More actions" });
      await moreActions.click();
      const actionMenu = page.getByRole("menu", { name: "Editor actions" });
      await expect(actionMenu.getByRole("button", { name: "Copy content" })).toBeVisible();
      await page.keyboard.press("Escape");
      await expect(actionMenu).not.toBeVisible();
    }

    const scrollRegion = page.getByRole("region", { name: "CSV table preview" });
    const table = page.getByRole("table", { name: "CSV data" });
    await expect(scrollRegion).toBeVisible();
    await expect(page.getByRole("checkbox", { name: "First row is header" })).toBeChecked();
    await expect(page.getByText("160 records · 32 columns")).toBeVisible();
    await expect.poll(() => scrollMetrics(scrollRegion)).toMatchObject({ horizontal: true, vertical: true });
    await expect.poll(() => table.getByRole("row").count()).toBeLessThan(50);
    await expect.poll(() => table.getByRole("cell").count()).toBeLessThan(600);
    await scrollRegion.evaluate((element) => {
      element.scrollTo({ left: element.scrollWidth, top: element.scrollHeight });
      element.dispatchEvent(new Event("scroll"));
    });

    const finalHeader = table.getByRole("columnheader", { name: "field_32, column 32" });
    const finalRowHeader = table.getByRole("rowheader", { name: "161" });
    await expect(table.getByRole("cell", { name: "LAST-CELL" })).toBeInViewport();
    await expect(finalHeader).toBeInViewport();
    await expect(finalRowHeader).toBeInViewport();
    await expect.poll(() => stickyOffsets(scrollRegion, finalHeader, finalRowHeader)).toMatchObject({ top: 0, left: 0 });
    await expect.poll(() => page.evaluate(() => document.documentElement.scrollWidth <= document.documentElement.clientWidth)).toBe(true);
    await expectNoAccessibilityViolations(page);

    if (viewport.name === "desktop light") {
      await page.getByRole("checkbox", { name: "First row is header" }).click();
      await expect(page.getByText("161 records · 32 columns")).toBeVisible();
      await page.getByRole("button", { name: "Source", exact: true }).click();
      await expect(page.getByRole("region", { name: "CSV source" }).locator("pre")).toHaveText(csvContent);
      await page.getByRole("button", { name: "Table", exact: true }).click();
      await expect(page.getByRole("checkbox", { name: "First row is header" })).not.toBeChecked();
    }
  });
}

async function mockApi(page: import("@playwright/test").Page) {
  await routeJsonApi(page, (url) => responseFor(url));
}

function responseFor(url: URL) {
  if (url.pathname === "/api/v1/me") return me;
  if (url.pathname === "/api/v1/me/usage") return usageResponse(space);
  if (url.pathname === "/api/v1/spaces") {
    return { spaces: [space], page: pageInfo(1) };
  }
  if (url.pathname === `/api/v1/spaces/${space.id}/nodes/${space.root_node_id}/children`) {
    return {
      parent: { id: space.root_node_id, path: "/" },
      children: [csvNode],
      page: pageInfo(1)
    };
  }
  if (url.pathname === `/api/v1/spaces/${space.id}/nodes`) {
    return { nodes: [csvNode], page: pageInfo(1) };
  }
  if (url.pathname === `/api/v1/spaces/${space.id}/nodes/${csvNode.id}`) {
    return csvNode;
  }
  if (url.pathname === `/api/v1/spaces/${space.id}/nodes/${csvNode.id}/reveal`) {
    return { ancestors: [], target: csvNode };
  }
  if (url.pathname === `/api/v1/spaces/${space.id}/text/${csvNode.id}`) {
    return {
      node: { id: csvNode.id, path: csvNode.path },
      text: {
        node_id: csvNode.id,
        storage_format: "plain",
        content: csvContent,
        content_sha256: csvNode.content_sha256,
        byte_len: csvNode.byte_len,
        line_count: csvNode.line_count,
        start_line: 1,
        end_line: csvNode.line_count,
        returned_lines: csvNode.line_count,
        truncated: false,
        next_start_line: null,
        updated_by: me.account,
        updated_at: csvNode.updated_at
      }
    };
  }
  if (url.pathname === `/api/v1/spaces/${space.id}/file-change-sync`) {
    return { changes: [], next_after_id: 0, has_more: false, resync_required: false };
  }
  throw new Error(`Unhandled API request: ${url.pathname}${url.search}`);
}

async function scrollMetrics(locator: import("@playwright/test").Locator) {
  return locator.evaluate((element) => ({
    horizontal: element.scrollWidth > element.clientWidth,
    vertical: element.scrollHeight > element.clientHeight
  }));
}

async function stickyOffsets(
  scrollRegion: import("@playwright/test").Locator,
  columnHeader: import("@playwright/test").Locator,
  rowHeader: import("@playwright/test").Locator
) {
  const [scrollBox, columnBox, rowBox] = await Promise.all([
    scrollRegion.boundingBox(),
    columnHeader.boundingBox(),
    rowHeader.boundingBox()
  ]);
  if (!scrollBox || !columnBox || !rowBox) return null;
  return {
    top: Math.round(columnBox.y - scrollBox.y),
    left: Math.round(rowBox.x - scrollBox.x)
  };
}

function largeCsv() {
  const header = Array.from({ length: 32 }, (_, index) => `field_${index + 1}`);
  const records = Array.from({ length: 160 }, (_, rowIndex) => (
    Array.from({ length: 32 }, (_, columnIndex) => (
      rowIndex === 159 && columnIndex === 31 ? "LAST-CELL" : `r${rowIndex + 1}-c${columnIndex + 1}`
    )).join(",")
  ));
  return [header.join(","), ...records].join("\n");
}

function pageInfo(returned: number) {
  return { limit: 100, returned, has_more: false, next_cursor: null };
}
