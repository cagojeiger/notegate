import { expect, test } from "@playwright/test";

import type { Me, RestNode, Space } from "../src/api/types";
import { routeJsonApi } from "./support/api";
import { usageResponse } from "./support/usage";

const me: Me = {
  account: { id: "user-1", kind: "user", display_name: "User" },
  user: { email: "user@example.com" },
  capabilities: { can_create_space: true, can_manage_agents: true }
};

const space: Space = {
  id: "space-1",
  name: "Daily",
  sort_order: 0,
  navigation_pinned: true,
  user_mcp_enabled: true,
  default_search_enabled: true,
  default_text_encryption_enabled: false,
  features: { text_encryption: true, write_lock: true },
  permission: "write",
  root_node_id: "root-1",
  created_at: "2026-08-01T00:00:00Z",
  updated_at: "2026-08-01T00:00:00Z"
};

const markdownNode: RestNode = {
  id: "outline-note",
  space_id: space.id,
  parent_id: space.root_node_id,
  name: "outline-note.md",
  kind: "text",
  path: "/outline-note.md",
  sort_order: 0,
  metadata: {},
  search_enabled: true,
  write_locked: false,
  write_lock_sources: [],
  has_children: false,
  effective_write_locked: false,
  byte_len: 12_000,
  line_count: 180,
  content_sha256: "sha-outline-note",
  text_storage_format: "plain",
  text_at_rest_encryption: "none",
  created_by: me.account,
  updated_by: me.account,
  created_at: "2026-08-01T00:00:00Z",
  updated_at: "2026-08-01T00:00:00Z"
};

test("marks the final outline row current after navigating to the document bottom", async ({ page }) => {
  await page.setViewportSize({ width: 1440, height: 900 });
  await mockApi(page);
  await page.goto("/");

  await page.getByRole("button", { name: markdownNode.name }).first().click();
  await expect(page.getByRole("heading", { name: "Opening" })).toBeVisible();
  const readingTypography = await page.locator(".markdown").evaluate((element) => {
    const style = getComputedStyle(element);
    return { fontFamily: style.fontFamily, fontSize: style.fontSize, lineHeight: style.lineHeight };
  });
  expect(readingTypography).toMatchObject({ fontSize: "16px", lineHeight: "27.2px" });
  expect(readingTypography.fontFamily).toContain("-apple-system");
  expect(readingTypography.fontFamily).not.toContain("LINE Seed Sans KR");

  await page.getByRole("tab", { name: "Outline" }).click();
  const outline = page.getByRole("navigation", { name: "Document outline" });
  const finalHeading = outline.getByRole("button", { name: "Final heading" });
  await finalHeading.click();

  const scrollRegion = page.getByTestId("markdown-preview-scroll-region");
  await expect.poll(() => scrollMetrics(scrollRegion)).toMatchObject({ atBottom: true });
  await expect(finalHeading).toHaveAttribute("aria-current", "location");
});

test("keeps the outline frame fixed while its headings scroll", async ({ page }) => {
  await page.setViewportSize({ width: 1440, height: 520 });
  await mockApi(page);
  await page.goto("/");

  await page.getByRole("button", { name: markdownNode.name }).first().click();
  await page.getByRole("tab", { name: "Outline" }).click();

  const panel = page.getByRole("tabpanel", { name: "Outline" });
  const outline = page.getByRole("navigation", { name: "Document outline" });
  await expect(outline).toBeVisible();
  await outline.focus();
  await expect(outline).toBeFocused();

  const initialGeometry = await outlineGeometry(panel, outline);
  expect(initialGeometry.topInset).toBeGreaterThanOrEqual(11);
  expect(initialGeometry.bottomInset).toBeGreaterThanOrEqual(11);

  await outline.evaluate((element) => {
    element.scrollTop = element.scrollHeight;
  });
  await expect.poll(() => outline.evaluate((element) => element.scrollTop)).toBeGreaterThan(0);

  expect(await outlineGeometry(panel, outline)).toEqual(initialGeometry);
});

test("closes the mobile Inspector after an outline navigation", async ({ page }) => {
  await page.setViewportSize({ width: 390, height: 844 });
  await mockApi(page);
  await page.goto("/");

  await page.getByRole("button", { name: "Toggle left sidebar" }).click();
  await page.getByRole("button", { name: markdownNode.name }).first().click();
  await page.getByRole("button", { name: "Toggle right sidebar" }).click();
  const inspector = page.getByRole("complementary", { name: "Inspector" });
  await expect(inspector).toBeVisible();
  await inspector.getByRole("tab", { name: "Outline" }).click();

  await page.getByRole("navigation", { name: "Document outline" })
    .getByRole("button", { name: "Final heading" })
    .click();

  await expect(inspector).toHaveCount(0);
});

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
      children: [markdownNode],
      page: pageInfo(1)
    };
  }
  if (url.pathname === `/api/v1/spaces/${space.id}/nodes`) {
    return { nodes: [markdownNode], page: pageInfo(1) };
  }
  if (url.pathname === `/api/v1/spaces/${space.id}/nodes/${markdownNode.id}`) {
    return markdownNode;
  }
  if (url.pathname === `/api/v1/spaces/${space.id}/nodes/${markdownNode.id}/reveal`) {
    return { ancestors: [], target: markdownNode };
  }
  if (url.pathname === `/api/v1/spaces/${space.id}/text/${markdownNode.id}`) {
    return {
      node: { id: markdownNode.id, path: markdownNode.path },
      text: {
        node_id: markdownNode.id,
        storage_format: "plain",
        content: longMarkdown(),
        content_sha256: markdownNode.content_sha256,
        byte_len: markdownNode.byte_len,
        line_count: markdownNode.line_count,
        start_line: 1,
        end_line: markdownNode.line_count,
        returned_lines: markdownNode.line_count,
        truncated: false,
        next_start_line: null,
        updated_by: me.account,
        updated_at: markdownNode.updated_at
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
    atBottom: element.scrollTop + element.clientHeight >= element.scrollHeight - 2
  }));
}

async function outlineGeometry(
  panel: import("@playwright/test").Locator,
  outline: import("@playwright/test").Locator
) {
  const panelBox = await panel.boundingBox();
  const outlineBox = await outline.boundingBox();
  if (!panelBox || !outlineBox) throw new Error("Outline geometry is unavailable");
  return {
    x: outlineBox.x,
    y: outlineBox.y,
    width: outlineBox.width,
    height: outlineBox.height,
    topInset: outlineBox.y - panelBox.y,
    bottomInset: panelBox.y + panelBox.height - outlineBox.y - outlineBox.height
  };
}

function longMarkdown() {
  const sections = Array.from({ length: 18 }, (_, index) => [
    `## Section ${index + 1}`,
    "",
    "This paragraph adds enough height for outline navigation to require a real scroll.",
    "",
    "Another line keeps the preview layout tall and stable.",
    ""
  ].join("\n"));

  return [
    "# Opening",
    "",
    "The first heading starts near the top of the preview.",
    "",
    ...sections,
    "## Final heading",
    "",
    "The last heading sits at the end so navigating to it clamps the preview at the document bottom."
  ].join("\n");
}

function pageInfo(returned: number) {
  return { limit: 100, returned, has_more: false, next_cursor: null };
}
