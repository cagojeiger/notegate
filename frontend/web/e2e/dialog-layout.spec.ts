import { expect, test } from "@playwright/test";

import { makeRestNode, makeSpace } from "../src/test/fixtures";
import { routeJsonApi } from "./support/api";
import { usageResponse } from "./support/usage";

for (const viewport of [{ name: "desktop", width: 1440, height: 900 }, { name: "mobile", width: 390, height: 844 }]) {
  test(`small dialogs keep their frame through long paths and errors on ${viewport.name}`, async ({ page }) => {
    await page.setViewportSize(viewport);
    const space = makeSpace();
    const node = makeRestNode();
    const folders = Array.from({ length: 3 }, (_, i) => makeRestNode({
      id: `folder-${i}`, kind: "folder", name: `Destination ${i} ${"long name ".repeat(8)}`.trim(),
      parent_id: i === 0 ? space.root_node_id : `folder-${i - 1}`,
      path: `/destination/${i}`, has_children: i < 2
    }));
    const meta = { limit: 50, returned: 0, has_more: false, next_cursor: null };
    await routeJsonApi(page, (url) => {
      if (url.pathname === "/api/v1/me") return { account: { id: "user-1", kind: "user", display_name: "User" }, user: { email: "user@example.com" }, capabilities: { can_create_space: true, can_manage_agents: true } };
      if (url.pathname === "/api/v1/me/usage") return usageResponse(space);
      if (url.pathname === "/api/v1/spaces") return { spaces: [space], page: meta };
      if (url.pathname === `/api/v1/spaces/${space.id}/nodes/${node.id}`) return node;
      if (url.pathname.endsWith("/children")) {
        const parentId = url.pathname.split("/").at(-2)!;
        return { parent: { id: parentId, path: "/" }, children: [...(parentId === space.root_node_id ? [node] : []), ...folders.filter((folder) => folder.parent_id === parentId)], page: meta };
      }
      if (url.pathname.endsWith("/nodes")) return { nodes: [], page: meta };
      if (url.pathname.endsWith("/file-change-sync")) return { changes: [], next_after_id: 0, has_more: false, resync_required: false };
      throw new Error(`Unhandled request: ${url.pathname}`);
    });
    const message = "The destination is unavailable. Choose another name or location. ".repeat(8);
    let mutations = 0;
    await page.route("**/api/v1/**", async (route) => {
      if (route.request().method() === "GET") return route.fallback();
      mutations++;
      await route.fulfill({ status: 409, contentType: "application/json", body: JSON.stringify({ kind: "conflict", message }) });
    });
    await page.goto("/");
    if (viewport.name === "mobile") await page.getByRole("button", { name: "Toggle left sidebar" }).click();
    for (const action of ["Rename", "Delete", "Move…"]) {
      await page.getByRole("button", { name: node.name, exact: true }).click({ button: "right" });
      await page.getByRole("menu").getByRole("menuitem", { name: action, exact: true }).click();
      const dialog = page.getByRole("dialog");
      await expect(dialog).toBeVisible();
      const bounds = await dialog.boundingBox();
      if (action === "Rename") await dialog.getByRole("textbox").fill("renamed.md");
      if (action === "Move…") {
        for (const folder of folders) {
          await dialog.getByRole("button", { name: folder.name, exact: true }).last().click();
          expect(await dialog.boundingBox()).toEqual(bounds);
        }
        await expect(dialog.getByText("No subfolders here")).toBeVisible();
        expect(await dialog.getByLabel("Destination path").evaluate((element) => element.scrollLeft)).toBeGreaterThan(0);
      }
      const submit = dialog.getByRole("button", { name: action === "Move…" ? "Move here" : action, exact: true });
      const submitBounds = await submit.boundingBox();
      await submit.click();
      await expect(dialog.getByRole("alert")).toHaveText(message);
      expect(await dialog.boundingBox()).toEqual(bounds);
      expect(await submit.boundingBox()).toEqual(submitBounds);
      const errorRegion = dialog.getByRole("alert").locator("..");
      expect(await errorRegion.evaluate((element) => element.scrollHeight > element.clientHeight)).toBe(true);
      await errorRegion.evaluate((element) => { element.scrollTop = element.scrollHeight; });
      await expect(submit).toBeInViewport();
      expect(await dialog.evaluate((element) => element.scrollWidth <= element.clientWidth)).toBe(true);
      await page.screenshot({ path: `test-results/dialog-${viewport.name}-${action === "Move…" ? "move" : action.toLowerCase()}-error.png` });
      await dialog.getByRole("button", { name: "Cancel", exact: true }).click();
    }
    expect(mutations).toBe(3);
  });
}
