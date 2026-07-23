import { expect, test } from "@playwright/test";

import type { Me, RestNode, Space } from "../src/api/types";
import { expectNoAccessibilityViolations } from "./support/accessibility";

const space: Space = {
  id: "space-1",
  name: "Daily",
  sort_order: 0,
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

const imageNode: RestNode = {
  id: "image-1",
  space_id: space.id,
  parent_id: space.root_node_id,
  name: "tall-preview.png",
  kind: "file",
  path: "/tall-preview.png",
  sort_order: 0,
  metadata: {},
  has_children: false,
  byte_len: 1024,
  media_type: "image/png",
  detected_media_type: "image/png",
  preview_available: true,
  encryption_mode: "none",
  created_by: me.account,
  updated_by: me.account,
  created_at: "2026-07-01T00:00:00Z",
  updated_at: "2026-07-01T00:00:00Z"
};

for (const viewport of [
  { name: "desktop", width: 1440, height: 900, mobile: false },
  { name: "tablet", width: 900, height: 1024, mobile: false },
  { name: "mobile", width: 390, height: 844, mobile: true }
]) {
  test(`tall file previews stay inside the editor on ${viewport.name}`, async ({ page }) => {
    await page.setViewportSize({ width: viewport.width, height: viewport.height });
    await mockFilePreviewApi(page);
    await page.goto("/");

    if (viewport.mobile) {
      await page.getByRole("button", { name: "Toggle left sidebar" }).click();
    }
    await page.getByRole("button", { name: imageNode.name }).first().click();
    await expect(page.getByRole("img", { name: imageNode.name })).toBeVisible();

    const scroller = page.locator("[data-file-detail-scroll]");
    await expect(scroller).toBeVisible();
    await expect.poll(() => scroller.evaluate((element) => ({
      overflowY: getComputedStyle(element).overflowY,
      scrollable: element.scrollHeight > element.clientHeight
    }))).toEqual({ overflowY: "auto", scrollable: true });

    const download = page.getByRole("button", { name: "Download" });
    await download.scrollIntoViewIfNeeded();
    await expectNoAccessibilityViolations(page);
    const [scrollerBox, downloadBox] = await Promise.all([scroller.boundingBox(), download.boundingBox()]);
    expect(scrollerBox).not.toBeNull();
    expect(downloadBox).not.toBeNull();
    expect(downloadBox!.y + downloadBox!.height).toBeLessThanOrEqual(scrollerBox!.y + scrollerBox!.height + 1);

    if (viewport.mobile) {
      await page.getByRole("button", { name: "Toggle left sidebar" }).click();
      const spaces = page.getByRole("navigation", { name: "Spaces" });
      const spacesBox = await spaces.boundingBox();
      expect(spacesBox).not.toBeNull();
      expect(await page.evaluate(({ x, y }) => {
        const target = document.elementFromPoint(x, y);
        return Boolean(target?.closest('nav[aria-label="Spaces"]'));
      }, {
        x: spacesBox!.x + spacesBox!.width / 2,
        y: spacesBox!.y + 8
      })).toBe(true);
    }
  });
}

async function mockFilePreviewApi(page: import("@playwright/test").Page) {
  const previewSvg = Buffer.from(
    '<svg xmlns="http://www.w3.org/2000/svg" width="1200" height="4000"><rect width="1200" height="4000" fill="#ffffff"/><path d="M80 120h1040v3760H80z" fill="none" stroke="#185fc4" stroke-width="16"/></svg>'
  ).toString("base64");

  await page.route("**/api/v1/**", async (route) => {
    const url = new URL(route.request().url());
    const response = responseFor(url, previewSvg);
    await route.fulfill({ status: 200, contentType: "application/json", body: JSON.stringify(response) });
  });
}

function responseFor(url: URL, previewSvg: string) {
  if (url.pathname === "/api/v1/me") return me;
  if (url.pathname === "/api/v1/spaces") {
    return { spaces: [space], page: pageInfo(1) };
  }
  if (url.pathname === `/api/v1/spaces/${space.id}/nodes/${space.root_node_id}/children`) {
    return {
      parent: { id: space.root_node_id, path: "/" },
      children: [imageNode],
      page: pageInfo(1)
    };
  }
  if (url.pathname === `/api/v1/spaces/${space.id}/nodes`) {
    return { nodes: [imageNode], page: pageInfo(1) };
  }
  if (url.pathname === `/api/v1/spaces/${space.id}/nodes/${imageNode.id}`) {
    return imageNode;
  }
  if (url.pathname === `/api/v1/spaces/${space.id}/nodes/${imageNode.id}/reveal`) {
    return { ancestors: [], target: imageNode };
  }
  if (url.pathname === `/api/v1/spaces/${space.id}/files/${imageNode.id}/preview-url`) {
    return {
      url: `data:image/svg+xml;base64,${previewSvg}`,
      media_type: "image/png",
      expires_at: "2026-07-24T12:00:00Z"
    };
  }
  throw new Error(`Unhandled API request: ${url.pathname}${url.search}`);
}

function pageInfo(returned: number) {
  return { limit: 100, returned, has_more: false, next_cursor: null };
}
