import { expect, test, type Page } from "@playwright/test";

import type { Me, Space } from "../src/api/types";
import { expectNoAccessibilityViolations } from "./support/accessibility";

const me: Me = {
  account: { id: "user-1", kind: "user", display_name: "User" },
  user: { email: "user@example.com" },
  capabilities: { can_create_space: true, can_manage_agents: true }
};

const initialSpaces: Space[] = [
  space("daily", "Daily", 1000, true),
  space("research", "Research", 2000, true),
  space("journal", "Private Journal", 3000, false),
  space("archive", "Archive", 4000, false)
];

test("Space Library keeps one accessible ordered grid", async ({ page }) => {
  await page.setViewportSize({ width: 1440, height: 900 });
  await page.emulateMedia({ reducedMotion: "reduce" });
  const api = await mockSpaceLibraryApi(page);
  await page.goto("/");
  await page.getByRole("button", { name: "Open space library" }).click();

  const grid = page.getByRole("list", { name: "All spaces" });
  await expect(grid).toBeVisible();
  await expect(grid.getByRole("listitem")).toHaveCount(4);
  await expect(page.getByRole("heading", { name: "Spaces 4" })).toBeVisible();
  await expect(page.getByRole("complementary", { name: "Space navigation" })).toBeVisible();
  await expect(page.getByRole("complementary", { name: "Space inspector" })).toBeVisible();
  await expect(page.locator("footer").getByText("ready", { exact: true })).toBeVisible();
  await expect(page.locator("footer").getByText("Daily", { exact: true })).toBeVisible();
  await expectNoAccessibilityViolations(page);

  const cardBoxes = await grid.getByRole("listitem").evaluateAll((items) => items.map((item) => {
    const box = item.getBoundingClientRect();
    return { x: box.x, y: box.y, width: box.width };
  }));
  expect(cardBoxes[0].y).toBe(cardBoxes[1].y);
  expect(cardBoxes[1].y).toBe(cardBoxes[2].y);
  expect(cardBoxes[3].y).toBeGreaterThan(cardBoxes[0].y);
  expect(cardBoxes.every((box) => box.width >= 288 && box.width <= 384)).toBe(true);
  expect(
    Math.max(...cardBoxes.map((box) => box.width))
      - Math.min(...cardBoxes.map((box) => box.width))
  ).toBeLessThanOrEqual(1);

  const archiveCard = grid.getByRole("listitem").filter({ hasText: "Archive" });
  await archiveCard.getByTitle("Search default on").click();
  await expect(page.getByRole("button", { name: "Inspect Archive" })).toHaveAttribute("aria-pressed", "true");
  await page.getByRole("button", { name: "Inspect Daily" }).click();

  const inspectorToggle = page.getByRole("button", { name: "Toggle space inspector" });
  await expect(inspectorToggle).toHaveAttribute("aria-pressed", "true");
  await inspectorToggle.click();
  await expect(page.getByText("Space Inspector", { exact: true })).toBeHidden();
  await expect.poll(async () => {
    const boxes = await grid.getByRole("listitem").evaluateAll((items) => items.map((item) => item.getBoundingClientRect().y));
    return new Set(boxes).size;
  }).toBe(1);
  const expandedWidths = await grid.getByRole("listitem").evaluateAll((items) => items.map((item) => item.getBoundingClientRect().width));
  expect(expandedWidths.every((width) => width <= 384)).toBe(true);
  expect(
    Math.max(...expandedWidths) - Math.min(...expandedWidths)
  ).toBeLessThanOrEqual(1);
  await inspectorToggle.click();
  await expect(page.getByText("Space Inspector", { exact: true })).toBeVisible();

  const navigationHelp = page.getByRole("button", { name: "About Navigation" });
  await navigationHelp.focus();
  const helpTooltip = page.getByRole("tooltip");
  await expect(helpTooltip).toContainText("Pinned spaces stay visible");
  await helpTooltip.hover();
  await expect(helpTooltip).toBeVisible();
  await page.keyboard.press("Escape");
  await expect(helpTooltip).toBeHidden();
  await navigationHelp.click();
  await expect(helpTooltip).toBeVisible();
  await navigationHelp.click();
  await expect(helpTooltip).toBeHidden();

  const orderBeforePin = await cardNames(grid);
  await page.getByRole("button", { name: "Pin Private Journal to navigation" }).click();
  await expect(page.getByRole("button", { name: "Unpin Private Journal from navigation" })).toBeVisible();
  expect(await cardNames(grid)).toEqual(orderBeforePin);

  await page.getByRole("button", { name: "Inspect Private Journal" }).click();
  await expect(page.getByRole("button", { name: "Inspect Private Journal" })).toHaveAttribute("aria-pressed", "true");
  await expect(page.getByRole("button", { name: "Inspect Daily" })).toHaveAttribute("aria-pressed", "false");
  await page.getByRole("switch", { name: "User MCP access" }).click();
  await expect(page.getByTitle("User MCP access on")).toHaveCount(3);

  const moveDailyLater = page.getByRole("button", { name: "Move Daily later" });
  await expect(moveDailyLater).toBeEnabled();
  await moveDailyLater.focus();
  await moveDailyLater.press("Enter");
  await expect.poll(() => cardNames(grid)).toEqual(["Research", "Daily", "Private Journal", "Archive"]);
  expect(api.patchCount()).toBeGreaterThan(0);

  const handle = page.getByTestId("drag-handle-archive");
  const target = page.getByTestId("drag-handle-daily");
  const [handleBox, targetBox] = await Promise.all([handle.boundingBox(), target.boundingBox()]);
  expect(handleBox).not.toBeNull();
  expect(targetBox).not.toBeNull();
  await page.mouse.move(handleBox!.x + handleBox!.width / 2, handleBox!.y + handleBox!.height / 2);
  await page.mouse.down();
  await page.mouse.move(targetBox!.x + targetBox!.width / 2, targetBox!.y + targetBox!.height / 2, { steps: 8 });
  await page.mouse.up();
  await expect.poll(() => cardNames(grid)).toEqual(["Research", "Archive", "Daily", "Private Journal"]);

  const undersizedTargets = await page.locator("button").evaluateAll((buttons) => buttons
    .filter((button) => {
      const style = window.getComputedStyle(button);
      return style.visibility !== "hidden" && style.display !== "none";
    })
    .map((button) => {
      const box = button.getBoundingClientRect();
      return { name: button.getAttribute("aria-label") ?? button.textContent?.trim(), width: box.width, height: box.height };
    })
    .filter((target) => target.width > 0 && target.height > 0)
    .filter((target) => target.width < 24 || target.height < 24));
  expect(undersizedTargets).toEqual([]);
});

test("Space Library expands the card grid when the desktop inspector closes", async ({ page }) => {
  await page.setViewportSize({ width: 1920, height: 1080 });
  await mockSpaceLibraryApi(page);
  await page.goto("/");
  await page.getByRole("button", { name: "Open space library" }).click();

  const grid = page.getByRole("list", { name: "All spaces" });
  const widthWithInspector = await grid.evaluate((element) => element.getBoundingClientRect().width);
  await page.getByRole("button", { name: "Toggle space inspector" }).click();

  await expect.poll(
    async () => grid.evaluate((element) => element.getBoundingClientRect().width)
  ).toBeGreaterThan(widthWithInspector + 250);
});

test("Space Library keeps the desktop inspector scrollable at short viewport heights", async ({ page }) => {
  await page.setViewportSize({ width: 1280, height: 480 });
  await mockSpaceLibraryApi(page);
  await page.goto("/");
  await page.getByRole("button", { name: "Open space library" }).click();

  const inspector = page.getByRole("complementary", { name: "Space inspector" });
  const scrollRegion = inspector.getByTestId("space-inspector-scroll-region");
  await expect(inspector).toBeVisible();
  await expect.poll(
    async () => scrollRegion.evaluate((element) => element.scrollHeight > element.clientHeight)
  ).toBe(true);

  await scrollRegion.evaluate((element) => {
    element.scrollTop = 100;
  });
  await expect
    .poll(async () => scrollRegion.evaluate((element) => element.scrollTop))
    .toBeGreaterThan(0);
});

test("Space Library keeps the mobile inspector scrollable", async ({ page }) => {
  await page.setViewportSize({ width: 390, height: 640 });
  await mockSpaceLibraryApi(page);
  await page.goto("/");
  await page.getByRole("button", { name: "Open space library" }).click();
  await page.getByRole("button", { name: "Inspect Daily" }).click();

  const inspector = page.getByRole("dialog", { name: "Space Inspector" });
  const scrollRegion = inspector.getByTestId("space-inspector-scroll-region");
  await expect(inspector).toBeVisible();
  await expect.poll(
    async () => scrollRegion.evaluate((element) => element.scrollHeight > element.clientHeight)
  ).toBe(true);

  await scrollRegion.evaluate((element) => {
    element.scrollTop = element.scrollHeight;
  });
  await expect
    .poll(async () => scrollRegion.evaluate((element) => element.scrollTop))
    .toBeGreaterThan(0);
  await expect(inspector.getByText("Files", { exact: true })).toBeInViewport();
});

for (const viewport of [
  { name: "desktop", width: 1280, height: 900, mobile: false, minimumActionHeight: 24 },
  { name: "mobile", width: 390, height: 844, mobile: true, minimumActionHeight: 44 }
]) {
  test(`Space Inspector keeps maintenance actions secondary on ${viewport.name}`, async ({ page }) => {
    await page.setViewportSize({ width: viewport.width, height: viewport.height });
    await mockSpaceLibraryApi(page);
    await page.goto("/");
    await page.getByRole("button", { name: "Open space library" }).click();
    if (viewport.mobile) await page.getByRole("button", { name: "Inspect Daily" }).click();

    const inspector = viewport.mobile
      ? page.getByRole("dialog", { name: "Space Inspector" })
      : page.getByRole("complementary", { name: "Space inspector" });
    const reindex = inspector.getByRole("button", { name: "Reindex links in Daily" });
    const recalculate = inspector.getByRole("button", { name: "Recalculate Daily usage" });
    const filesUsage = inspector.getByRole("progressbar", { name: "Files usage" });

    await recalculate.scrollIntoViewIfNeeded();
    await expect(reindex).toHaveText("Reindex");
    await expect(recalculate).toHaveText("Recalculate");
    const expectedActionTypography = { fontSize: "13px", lineHeight: "18px", fontWeight: "500" };
    expect(await typography(reindex)).toEqual(expectedActionTypography);
    expect(await typography(recalculate)).toEqual(expectedActionTypography);

    const [filesUsageBox, recalculateBox, reindexBox] = await Promise.all([
      filesUsage.boundingBox(),
      recalculate.boundingBox(),
      reindex.boundingBox()
    ]);
    expect(recalculateBox?.y).toBeGreaterThanOrEqual((filesUsageBox?.y ?? 0) + (filesUsageBox?.height ?? 0) + 8);
    expect(recalculateBox?.height).toBeGreaterThanOrEqual(viewport.minimumActionHeight);
    expect(reindexBox?.height).toBeGreaterThanOrEqual(viewport.minimumActionHeight);
  });
}

test("Space reindex disables immediately and does not submit twice", async ({ page }) => {
  await page.setViewportSize({ width: 1280, height: 900 });
  const api = await mockSpaceLibraryApi(page);
  await page.goto("/");
  await page.getByRole("button", { name: "Open space library" }).click();

  const reindex = page.getByRole("button", { name: "Reindex links in Daily" });
  await reindex.click();
  await expect(reindex).toBeDisabled();
  await expect(reindex).toHaveText("Reindexing…");
  await reindex.evaluate((button) => {
    (button as HTMLButtonElement).click();
    (button as HTMLButtonElement).click();
  });

  expect(api.linkReindexRequests()).toBe(1);
});

test("opening an unpinned Space does not add it to navigation", async ({ page }) => {
  await page.setViewportSize({ width: 1440, height: 900 });
  await mockSpaceLibraryApi(page);
  await page.goto("/");
  await page.getByRole("button", { name: "Open space library" }).click();

  const privateCard = page
    .getByRole("list", { name: "All spaces" })
    .getByRole("listitem")
    .filter({ hasText: "Private Journal" });
  await privateCard.getByRole("button", { name: "Open" }).click();

  await expect(page.getByText("/ Private Journal", { exact: true })).toBeVisible();
  await expect(page.getByRole("button", { name: "Private Journal" })).toHaveCount(0);
});

for (const viewport of [
  { name: "compact desktop", width: 1180, height: 900, columns: 2, mobile: false },
  { name: "tablet", width: 900, height: 1024, columns: 1, mobile: false },
  { name: "mobile", width: 390, height: 844, columns: 1, mobile: true },
  { name: "narrow mobile", width: 320, height: 800, columns: 1, mobile: true }
]) {
  test(`Space Library reflows to ${viewport.columns} column on ${viewport.name}`, async ({ page }) => {
    await page.setViewportSize({ width: viewport.width, height: viewport.height });
    await mockSpaceLibraryApi(page);
    await page.goto("/");
    await page.getByRole("button", { name: "Open space library" }).click();

    const items = page.getByRole("list", { name: "All spaces" }).getByRole("listitem");
    await expect(items).toHaveCount(4);
    const boxes = await items.evaluateAll((elements) => elements.map((element) => {
      const box = element.getBoundingClientRect();
      return { x: box.x, y: box.y };
    }));

    if (viewport.columns === 2) {
      expect(boxes[0].y).toBe(boxes[1].y);
      expect(boxes[2].y).toBeGreaterThan(boxes[0].y);
    } else {
      expect(boxes[1].y).toBeGreaterThan(boxes[0].y);
      if (viewport.mobile) {
        await expect(page.getByRole("navigation", { name: "Spaces" })).toBeVisible();
        await expect(page.getByRole("complementary", { name: "Space navigation" })).toBeHidden();
        await expect(page.getByText("Space Inspector", { exact: true })).toBeHidden();
        await page.getByRole("button", { name: "Inspect Daily" }).click();
        await expect(page.getByRole("dialog", { name: "Space Inspector" })).toBeVisible();
      } else {
        await expect(page.getByText("Space Inspector", { exact: true })).toBeVisible();
      }
    }
    expect(await page.evaluate(() => document.documentElement.scrollWidth)).toBeLessThanOrEqual(viewport.width);
    await expectNoAccessibilityViolations(page);
  });
}

async function mockSpaceLibraryApi(page: Page) {
  let spaces = initialSpaces.map((item) => ({ ...item }));
  let patchCount = 0;
  let linkReindexRequests = 0;
  const pendingLinkIndexes = new Set<string>();

  await page.route("**/api/v1/**", async (route) => {
    const request = route.request();
    const url = new URL(request.url());

    if (url.pathname === "/api/v1/me") return respond(route, me);
    const linkIndexMatch = url.pathname.match(/^\/api\/v1\/spaces\/([^/]+)\/link-index$/);
    if (linkIndexMatch) {
      const pending = pendingLinkIndexes.has(linkIndexMatch[1]);
      return respond(route, {
        status: pending ? "pending" : "idle",
        availability: {
          can_trigger: !pending,
          reason: pending ? "pending" : null,
          retry_at: null
        }
      });
    }
    const linkReindexMatch = url.pathname.match(/^\/api\/v1\/spaces\/([^/]+)\/actions\/reindex-links$/);
    if (linkReindexMatch && request.method() === "POST") {
      linkReindexRequests += 1;
      pendingLinkIndexes.add(linkReindexMatch[1]);
      return respond(route, {
        result: "accepted",
        availability: { can_trigger: false, reason: "pending", retry_at: null }
      }, 202);
    }
    if (url.pathname === "/api/v1/me/usage") {
      return respond(route, {
        tier: "free",
        spaces: spaces.map((item, index) => ({
          id: item.id,
          name: item.name,
          items: { used: index + 1, limit: 100 },
          text_bytes: { used: 1024 * (index + 1), limit: 1024 * 100 },
          file_bytes: { used: 2048 * (index + 1), limit: 2048 * 100 },
          reconciliation: {
            status: "idle",
            availability: { can_trigger: true, reason: null, retry_at: null }
          }
        }))
      });
    }
    if (url.pathname === "/api/v1/spaces" && request.method() === "GET") {
      return respond(route, {
        spaces: [...spaces].sort((left, right) => left.sort_order - right.sort_order),
        page: { limit: 100, returned: spaces.length, has_more: false, next_cursor: null }
      });
    }
    if (url.pathname === "/api/v1/spaces:reorder" && request.method() === "POST") {
      const input = request.postDataJSON() as { updates: Array<{ space_id: string; sort_order: number }> };
      patchCount += 1;
      spaces = spaces.map((item) => {
        const update = input.updates.find((candidate) => candidate.space_id === item.id);
        return update ? { ...item, sort_order: update.sort_order } : item;
      });
      return route.fulfill({ status: 204 });
    }
    const spaceMatch = url.pathname.match(/^\/api\/v1\/spaces\/([^/]+)$/);
    if (spaceMatch && request.method() === "PATCH") {
      patchCount += 1;
      const input = request.postDataJSON() as Partial<Space>;
      const index = spaces.findIndex((item) => item.id === spaceMatch[1]);
      spaces[index] = { ...spaces[index], ...input, updated_at: "2026-07-25T01:00:00Z" };
      return respond(route, spaces[index]);
    }
    const childrenMatch = url.pathname.match(/^\/api\/v1\/spaces\/([^/]+)\/nodes\/([^/]+)\/children$/);
    if (childrenMatch) {
      return respond(route, {
        parent: { id: childrenMatch[2], path: "/" },
        children: [],
        page: { limit: 100, returned: 0, has_more: false, next_cursor: null }
      });
    }
    if (/^\/api\/v1\/spaces\/[^/]+\/nodes$/.test(url.pathname)) {
      return respond(route, {
        nodes: [],
        page: { limit: 50, returned: 0, has_more: false, next_cursor: null }
      });
    }
    if (/^\/api\/v1\/spaces\/[^/]+\/file-change-sync$/.test(url.pathname)) {
      return respond(route, { changes: [], next_after_id: 0, has_more: false, resync_required: false });
    }
    throw new Error(`Unhandled API request: ${request.method()} ${url.pathname}${url.search}`);
  });

  return {
    patchCount: () => patchCount,
    linkReindexRequests: () => linkReindexRequests
  };
}

async function respond(
  route: Parameters<Parameters<Page["route"]>[1]>[0],
  body: unknown,
  status = 200
) {
  await route.fulfill({ status, contentType: "application/json", body: JSON.stringify(body) });
}

async function cardNames(grid: ReturnType<Page["getByRole"]>) {
  return grid.getByRole("listitem").evaluateAll((items) => items.map((item) => {
    const inspect = item.querySelector<HTMLButtonElement>('button[aria-label^="Inspect "]');
    return inspect?.getAttribute("aria-label")?.replace(/^Inspect /, "") ?? "";
  }));
}

async function typography(locator: ReturnType<Page["getByRole"]>) {
  return locator.evaluate((element) => {
    const style = getComputedStyle(element);
    return {
      fontSize: style.fontSize,
      lineHeight: style.lineHeight,
      fontWeight: style.fontWeight
    };
  });
}

function space(id: string, name: string, sortOrder: number, navigationPinned: boolean): Space {
  return {
    id,
    name,
    sort_order: sortOrder,
    navigation_pinned: navigationPinned,
    user_mcp_enabled: navigationPinned,
    default_search_enabled: true,
    default_text_encryption_enabled: false,
    features: { text_encryption: true, write_lock: true },
    permission: "write",
    root_node_id: `${id}-root`,
    created_at: "2026-07-01T00:00:00Z",
    updated_at: "2026-07-25T00:00:00Z"
  };
}
