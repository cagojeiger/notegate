import { expect, test, type Page } from "@playwright/test";

import { makeRestNode, makeSpace } from "../src/test/fixtures";
import { routeJsonApi } from "./support/api";
import { usageResponse } from "./support/usage";

async function mockWorkbench(page: Page, nodeCount = 1) {
  let space = makeSpace();
  const node = makeRestNode();
  const nodes = [node, ...Array.from({ length: nodeCount - 1 }, (_, index) => makeRestNode({ id: `node-extra-${index}`, name: `note-${index}.md`, path: `/note-${index}.md` }))];
  const meta = { limit: 50, returned: 0, has_more: false, next_cursor: null };
  await routeJsonApi(page, (url, request) => {
    if (url.pathname === "/api/v1/me") return { account: { id: "user-1", kind: "user", display_name: "User" }, user: { email: "user@example.com" }, capabilities: { can_create_space: true, can_manage_agents: true } };
    if (url.pathname === "/api/v1/me/usage") return usageResponse(space);
    if (url.pathname === "/api/v1/spaces") return { spaces: [space], page: meta };
    if (url.pathname === `/api/v1/spaces/${space.id}` && request.method() === "PATCH") {
      space = { ...space, ...request.postDataJSON() };
      return space;
    }
    if (url.pathname === "/api/v1/agents") return { agents: [], page: meta };
    if (url.pathname === `/api/v1/spaces/${space.id}/nodes/${node.id}`) return node;
    if (url.pathname.endsWith("/link-index")) return { status: "idle", availability: { can_trigger: true, reason: null, retry_at: null } };
    if (url.pathname.endsWith("/children")) return { parent: { id: space.root_node_id, path: "/" }, children: nodes, page: meta };
    if (url.pathname.endsWith("/nodes")) return { nodes: [], page: meta };
    if (url.pathname.endsWith("/file-change-sync")) return { changes: [], next_after_id: 0, has_more: false, resync_required: false };
    throw new Error(`Unhandled API request: ${request.method()} ${url.pathname}`);
  });
  return node;
}

for (const profile of [
  { name: "desktop", width: 1440, hasTouch: false },
  { name: "mobile", width: 390, hasTouch: true },
  { name: "touch tablet", width: 900, hasTouch: true }
]) {
  test.describe(profile.name, () => {
    test.use({ viewport: { width: profile.width, height: 900 }, hasTouch: profile.hasTouch });

    test("keeps controls usable at the intended pointer density", async ({ page }) => {
      await mockWorkbench(page);
      await page.goto("/");
      await page.getByRole("button", { name: "Open space library" }).click();
      if (profile.width < 768) await page.getByRole("button", { name: "Inspect Daily" }).click();

      const toggle = page.getByRole("switch", { name: "Pin to navigation" });
      const toggleBox = await toggle.boundingBox();
      expect(toggleBox?.height).toBe(profile.hasTouch ? 44 : 24);
      expect(toggleBox?.width).toBe(profile.hasTouch ? 44 : 40);
      await expect(toggle).toHaveAttribute("aria-checked", "true");
      await toggle.click({ position: { x: 2, y: 2 } });
      await expect(toggle).toHaveAttribute("aria-checked", "false");
      if (profile.width < 768) await page.getByRole("dialog", { name: "Space Inspector" }).getByRole("button", { name: "Close", exact: true }).click();

      await page.getByRole("button", { name: "Settings", exact: true }).click();
      const settings = page.getByRole("dialog", { name: "Settings" });
      const tab = settings.getByRole("tab", { name: "Agents" });
      expect((await tab.boundingBox())?.height).toBe(profile.hasTouch ? 44 : 32);
      await tab.click();
      const field = settings.getByRole("textbox", { name: "New agent name" });
      expect((await field.boundingBox())?.height).toBe(profile.hasTouch ? 44 : 38);
      await field.focus();
      await page.keyboard.press("Tab");
      await page.keyboard.press("Shift+Tab");
      await expect(field).toBeFocused();
      expect(await field.evaluate((element) => ({ visible: element.matches(":focus-visible"), style: getComputedStyle(element).outlineStyle, width: getComputedStyle(element).outlineWidth }))).toEqual({ visible: true, style: "solid", width: "2px" });
    });

    if (profile.name === "touch tablet") {
      test("keeps virtual tree rows separate and rail controls within the 44px rail", async ({ page }) => {
        await mockWorkbench(page, 12);
        await page.goto("/");
        const rows = page.locator("[data-tree-index] [data-node-row]");
        await expect(rows).toHaveCount(12);
        const bounds = await rows.evaluateAll((elements) => elements.map((element) => {
          const rect = element.getBoundingClientRect();
          return { top: rect.top, bottom: rect.bottom, height: rect.height };
        }));
        for (const [index, row] of bounds.entries()) {
          expect(row.height).toBe(36);
          if (index > 0) {
            expect(row.top).toBeGreaterThanOrEqual(bounds[index - 1].bottom);
            expect(row.top - bounds[index - 1].top).toBe(38);
          }
        }

        const rail = page.getByRole("complementary", { name: "Space navigation" });
        const railBounds = await rail.boundingBox();
        expect(railBounds?.width).toBe(44);
        const buttons = await rail.getByRole("button").evaluateAll((elements) => elements.map((element) => {
          const rect = element.getBoundingClientRect();
          return { left: rect.left, right: rect.right, width: rect.width };
        }));
        expect(buttons.length).toBeGreaterThan(4);
        for (const button of buttons) {
          expect(button.width).toBe(44);
          expect(button.left).toBeGreaterThanOrEqual(railBounds!.x);
          expect(button.right).toBeLessThanOrEqual(railBounds!.x + railBounds!.width);
        }
      });
    }
  });
}

test("file and space context menus transfer focus and return it on dismissal", async ({ page }) => {
  await page.setViewportSize({ width: 1440, height: 900 });
  const node = await mockWorkbench(page);
  await page.goto("/");
  const row = page.getByRole("button", { name: node.name, exact: true });
  await row.focus();
  await row.click({ button: "right" });
  const files = page.getByRole("menu", { name: "File actions" });
  await expect(files.getByRole("menuitem", { name: "Open", exact: true })).toBeFocused();
  await page.keyboard.press("End");
  await expect(files.getByRole("menuitem", { name: "Delete", exact: true })).toBeFocused();
  await page.keyboard.press("Escape");
  await expect(files).toBeHidden();
  await expect(row).toBeFocused();

  const space = page.getByRole("button", { name: "Daily", exact: true });
  await space.focus();
  await space.click({ button: "right" });
  const spaces = page.getByRole("menu", { name: "Space actions" });
  await expect(spaces.getByRole("menuitem", { name: "Select", exact: true })).toBeFocused();
  await page.keyboard.press("Escape");
  await expect(space).toBeFocused();
});
