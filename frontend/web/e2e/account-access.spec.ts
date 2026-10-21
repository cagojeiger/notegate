import AxeBuilder from "@axe-core/playwright";
import { expect, test } from "@playwright/test";

import { makeSpace } from "../src/test/fixtures";
import { routeJsonApi } from "./support/api";
import { openSettings, closeSettingsInspector } from "./support/settings";
import { usageResponse } from "./support/usage";

for (const profile of [
  { name: "desktop dark", width: 1440, theme: "dark" },
  { name: "desktop light", width: 1440, theme: "light" },
  { name: "mobile dark", width: 390, theme: "dark" },
  { name: "small mobile light", width: 320, theme: "light" }
] as const) {
  test.describe(profile.name, () => {
    test.use({ viewport: { width: profile.width, height: 900 }, hasTouch: profile.width < 768 });

    test("keeps growing account connections bounded and revokes only the selected connection", async ({ page }) => {
      const space = makeSpace();
      const meta = { limit: 100, returned: 0, has_more: false, next_cursor: null };
      const dates = { created_at: "2026-10-18T00:00:00Z", idle_expires_at: "2026-11-02T00:00:00Z", expires_at: "2026-11-18T00:00:00Z" };
      const sessions = Array.from({ length: 24 }, (_, i) => ({ ...dates, id: `session-${i}`, current: i === 0, client_environment: i === 0 ? "Chrome on macOS" : `Browser ${i}`, last_used_at: dates.created_at }));
      let grants = (["cli", "mcp"] as const).flatMap((kind) => Array.from({ length: kind === "cli" ? 12 : 8 }, (_, i) => ({
        ...dates, id: `${kind}-${i}`, client_name: `${kind.toUpperCase()} App ${i}`, kind,
        client_environment: "macOS", last_token_at: dates.created_at, offline_access: true
      })));
      const reads = { sessions: 0, grants: 0 };
      const mutations: string[] = [];
      await page.clock.setFixedTime(new Date("2026-10-20T00:00:00Z"));
      await page.addInitScript((theme) => localStorage.setItem("notegate.theme", theme), profile.theme);
      await routeJsonApi(page, (url, request) => {
        if (request.method() === "DELETE") {
          mutations.push(url.pathname);
          grants = grants.filter((grant) => url.pathname !== `/api/v1/me/oauth-grants/${grant.id}`);
          return {};
        }
        if (url.pathname === "/api/v1/me") return { account: { id: "user-1", kind: "user", display_name: "User" }, user: { email: "user@example.com" }, capabilities: { can_create_space: true, can_manage_agents: true } };
        if (url.pathname === "/api/v1/me/usage") return usageResponse(space);
        if (url.pathname === "/api/v1/spaces") return { spaces: [space], page: meta };
        if (url.pathname === "/api/v1/me/sessions") { reads.sessions++; return { sessions }; }
        if (url.pathname === "/api/v1/me/oauth-grants") { reads.grants++; return { grants }; }
        if (url.pathname.endsWith("/children")) return { parent: { id: space.root_node_id, path: "/" }, children: [], page: meta };
        if (url.pathname.endsWith("/nodes")) return { nodes: [], page: meta };
        if (url.pathname.endsWith("/file-change-sync")) return { changes: [], next_after_id: 0, has_more: false, resync_required: false };
        throw new Error(`Unhandled request: ${request.method()} ${url.pathname}`);
      });
      await page.goto("/");
      const view = await openSettings(page, "Account");
      const list = view.getByRole("list", { name: "Browser sessions" });
      await expect(list.getByRole("button")).toHaveCount(10);
      await expect.poll(() => reads).toEqual({ sessions: 1, grants: 0 });
      const header = view.getByRole("tablist", { name: "Settings sections" });
      const bounds = await header.boundingBox();
      await view.getByRole("button", { name: "Show more (10)", exact: true }).click();
      await expect(list.getByRole("button")).toHaveCount(20);
      expect(reads).toEqual({ sessions: 1, grants: 0 });
      await list.getByRole("button").last().scrollIntoViewIfNeeded();
      expect(await list.evaluate((element) => element.scrollTop)).toBeGreaterThan(0);
      const scrollTop = await list.evaluate((element) => element.scrollTop);
      await list.getByRole("button", { name: "Select Browser 19", exact: true }).click();
      await closeSettingsInspector(page);
      expect(await header.boundingBox()).toEqual(bounds);
      await expect(view.getByRole("button", { name: "Copy user MCP server URL" })).toHaveCount(0);
      await expect(view.getByRole("button", { name: "Show more (4)" })).toBeInViewport();
      expect(await view.evaluate((element) => element.scrollWidth <= element.clientWidth)).toBe(true);

      const browserAccessibility = await new AxeBuilder({ page }).include("[data-settings-view]").withTags(["wcag2a", "wcag2aa", "wcag21aa", "wcag22aa"]).analyze();
      expect(browserAccessibility.violations).toEqual([]);
      await view.getByRole("tab", { name: "Connections", exact: true }).click();
      await expect(view.getByRole("button", { name: "Copy user MCP server URL" })).toBeInViewport();
      expect(reads).toEqual({ sessions: 1, grants: 0 });
      await expect(page.getByRole("button", { name: "Toggle settings inspector" })).toBeDisabled();
      await view.getByRole("tab", { name: "MCP", exact: true }).click();
      await expect(view.getByRole("tablist", { name: "Connection views" })).toBeInViewport();
      await expect(view.getByRole("list", { name: "MCP connections" }).getByRole("button")).toHaveCount(8);
      await view.getByRole("button", { name: "Select MCP App 1", exact: true }).click();
      await expect(view.getByRole("region", { name: "Selected connection" }).getByText("MCP App 1 · MCP", { exact: true })).toBeVisible();
      await expect(view.getByRole("button", { name: "Revoke all CLI connections", exact: true })).toHaveCount(0);
      const revoke = view.getByRole("button", { name: "Revoke MCP App 1", exact: true });
      await revoke.click();
      expect(mutations).toEqual([]);
      const confirmation = view.getByRole("group", { name: "Confirm revocation" });
      await expect(confirmation).toBeFocused();
      await expect(confirmation.getByRole("button", { name: "Confirm", exact: true })).toBeInViewport();
      const accessibility = await new AxeBuilder({ page }).include("[data-settings-view]").withTags(["wcag2a", "wcag2aa", "wcag21aa", "wcag22aa"]).analyze();
      expect(accessibility.violations).toEqual([]);
      await confirmation.getByRole("button", { name: "Cancel", exact: true }).click();
      await expect(revoke).toBeFocused();
      await revoke.click();
      await confirmation.getByRole("button", { name: "Confirm", exact: true }).click();
      await expect.poll(() => mutations).toEqual(["/api/v1/me/oauth-grants/mcp-1"]);
      await expect(view.getByRole("button", { name: "Select MCP App 1", exact: true })).toHaveCount(0);
      await expect(view.getByText("Access revoked.", { exact: true })).toBeFocused();
      await closeSettingsInspector(page);
      await view.getByRole("tab", { name: "CLI", exact: true }).click();
      await expect(view.getByRole("list", { name: "CLI connections" }).getByRole("button")).toHaveCount(10);
      await closeSettingsInspector(page);
      await view.getByRole("tab", { name: "Account", exact: true }).click();
      await expect(list.getByRole("button")).toHaveCount(20);
      await expect(list.getByRole("button", { name: "Select Browser 19", exact: true })).toHaveAttribute("aria-pressed", "true");
      expect(await list.evaluate((element) => element.scrollTop)).toBe(scrollTop);
      await closeSettingsInspector(page);
      await view.getByRole("tab", { name: "General", exact: true }).click();
      await view.getByRole("tab", { name: "Account", exact: true }).click();
      await expect(list.getByRole("button")).toHaveCount(20);
      await expect(list.getByRole("button", { name: "Select Browser 19", exact: true })).toHaveAttribute("aria-pressed", "true");
      expect(await list.evaluate((element) => element.scrollTop)).toBe(scrollTop);
      expect(await header.boundingBox()).toEqual(bounds);
      await closeSettingsInspector(page);
      if (profile.width >= 768) await view.getByRole("button", { name: "Close settings inspector", exact: true }).click();
      await view.getByRole("button", { name: "Open session actions", exact: true }).click();
      await expect(view.getByRole("region", { name: "Selected connection" })).toBeVisible();
    });
  });
}
