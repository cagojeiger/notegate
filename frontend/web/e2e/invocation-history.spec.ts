import { expect, test } from "@playwright/test";

import type { CommandInvocation } from "../src/api/types";
import { makeSpace } from "../src/test/fixtures";
import { routeJsonApi } from "./support/api";
import { usageResponse } from "./support/usage";

const space = makeSpace();
const emptyPage = { limit: 50, returned: 0, has_more: false, next_cursor: null };
const operation = "PUT /api/v2/spaces/{space_id}/text/{node_id}";
const invocations: CommandInvocation[] = Array.from({ length: 51 }, (_, index) => ({
  id: 51 - index,
  invocation_id: `d78b5f11-620e-44a5-9881-${String(51 - index).padStart(12, "0")}`,
  created_at: new Date(Date.UTC(2026, 9, 11, 13, 50) - index * 60_000).toISOString(),
  actor_account_id: "38339a28-3c37-4ba4-b11f-000000000001",
  caller_kind: "agent",
  surface: "api",
  tool: "http",
  op: operation,
  purpose: index === 0 ? "회의록의 결정 사항과 후속 작업 반영" : null,
  space_name: null,
  input: { method: "PUT", route: operation.slice(4), path_ids: {}, body_recorded: false },
  response: { status: 200, body_recorded: false },
  outcome: "success",
  error_code: null,
  duration_ms: 24
}));

for (const viewport of [
  { name: "desktop", width: 1440, height: 900 },
  { name: "mobile", width: 390, height: 844 },
  { name: "short-desktop", width: 1024, height: 320 }
]) {
  for (const theme of ["light", "dark"] as const) {
    test(`history and settings keep stable frames on ${viewport.name} in ${theme} mode`, async ({ page }, testInfo) => {
      await page.setViewportSize(viewport);
      // Cache freshness must not depend on CI runner speed.
      await page.clock.setFixedTime(new Date("2026-10-11T14:00:00Z"));
      await page.addInitScript((value) => window.localStorage.setItem("notegate.theme", value), theme);
      const historyRequests: URL[] = [];
      await routeJsonApi(page, (url) => {
        if (url.pathname === "/api/v1/me") return { account: { id: "user-1", kind: "user", display_name: "User" }, user: { email: "user@example.com" }, capabilities: { can_create_space: true, can_manage_agents: true } };
        if (url.pathname === "/api/v1/me/sessions") return { sessions: [{
          id: "current-session", current: true, client_environment: "Chrome on Linux",
          created_at: "2026-10-11T13:00:00Z", last_used_at: "2026-10-11T14:00:00Z",
          idle_expires_at: "2026-10-25T14:00:00Z", expires_at: "2026-11-10T13:00:00Z"
        }] };
        if (url.pathname === "/api/v1/me/oauth-grants") return { grants: [
          { id: "cli-grant", client_name: "NoteGate CLI", kind: "cli", client_environment: "NoteGate CLI on macOS", created_at: "2026-10-11T13:00:00Z", last_token_at: "2026-10-11T14:00:00Z", idle_expires_at: "2026-10-18T14:00:00Z", expires_at: "2026-11-10T13:00:00Z", offline_access: true },
          { id: "mcp-grant", client_name: "Remote MCP", kind: "mcp", client_environment: null, created_at: "2026-10-11T13:00:00Z", last_token_at: null, idle_expires_at: "2026-10-18T14:00:00Z", expires_at: "2026-11-10T13:00:00Z", offline_access: true }
        ] };
        if (url.pathname === "/api/v1/me/usage") return usageResponse(space);
        if (url.pathname === "/api/v1/spaces") return { spaces: [space], page: emptyPage };
        if (url.pathname.endsWith("/children")) return { parent: { id: space.root_node_id, path: "/" }, children: [], page: emptyPage };
        if (url.pathname.endsWith("/nodes")) return { nodes: [], page: emptyPage };
        if (url.pathname.endsWith("/file-change-sync")) return { changes: [], next_after_id: 0, has_more: false, resync_required: false };
        if (url.pathname.endsWith("/file-change-events")) return { events: [], page: emptyPage };
        if (url.pathname === "/api/v1/me/audit-events") return { events: [], page: emptyPage };
        if (url.pathname === "/api/v1/me/jobs") return { jobs: [], page: emptyPage };
        if (url.pathname === "/api/v1/agents") return { agents: Array.from({ length: 30 }, (_, i) => ({ id: `agent-${i}`, name: `Agent ${i}`, owner_user_id: "user-1" })), page: emptyPage };
        if (url.pathname === "/api/v1/me/command-invocations") {
          historyRequests.push(url);
          if (url.searchParams.get("surface") !== "api") return { command_invocations: [], page: emptyPage };
          const nextPage = url.searchParams.get("cursor") === "api-page-2";
          return {
            command_invocations: nextPage ? invocations.slice(50) : invocations.slice(0, 50),
            page: { ...emptyPage, returned: nextPage ? 1 : 50, has_more: !nextPage, next_cursor: nextPage ? null : "api-page-2" }
          };
        }
        throw new Error(`Unhandled request: ${url.pathname}`);
      });

      await page.goto("/");
      await page.getByRole("button", { name: "History", exact: true }).click();
      const dialog = page.getByRole("dialog", { name: "History" });
      await expect(dialog.getByText("No changes yet.")).toBeVisible();
      const bounds = await dialog.boundingBox();
      if (!bounds) throw new Error("History dialog missing");
      expect(bounds.y).toBeGreaterThanOrEqual(16);
      expect(bounds.y + bounds.height).toBeLessThanOrEqual(viewport.height - 16);
      const tabs = dialog.getByRole("tablist");
      const tabBounds = await tabs.boundingBox();
      expect(historyRequests).toHaveLength(0);
      let release!: () => void;
      const loading = new Promise<void>((resolve) => { release = resolve; });
      await page.route("**/api/v1/me/command-invocations?surface=api&limit=50", async (route) => {
        await loading;
        await route.fallback();
      });
      try {
        await dialog.getByRole("tab", { name: "API", exact: true }).click();
        await expect(dialog.getByText("Loading…", { exact: true })).toBeVisible();
        expect(await dialog.boundingBox()).toEqual(bounds);
      } finally {
        release();
      }
      const rows = dialog.locator("ol > li");
      await expect(rows).toHaveCount(50);
      expect(await dialog.boundingBox()).toEqual(bounds);
      expect(historyRequests.map((url) => url.search)).toEqual(["?surface=api&limit=50"]);
      const panel = dialog.locator("section");
      expect(await panel.evaluate((element) => element.scrollWidth <= element.clientWidth)).toBe(true);

      await rows.first().getByText("Response", { exact: true }).click();
      await expect(rows.first().locator("pre")).toContainText('"status": 200');
      expect(await dialog.boundingBox()).toEqual(bounds);
      expect(historyRequests).toHaveLength(1);
      await testInfo.attach(`invocation-history-${viewport.name}-${theme}`, {
        body: await page.screenshot({ path: `test-results/invocation-history-${viewport.name}-${theme}.png` }),
        contentType: "image/png"
      });

      await dialog.getByRole("tab", { name: "CLI", exact: true }).click();
      await expect(dialog.getByText("No CLI calls.")).toBeVisible();
      expect(await dialog.boundingBox()).toEqual(bounds);
      await page.screenshot({ path: `test-results/invocation-history-empty-${viewport.name}-${theme}.png` });
      await dialog.getByRole("tab", { name: "API", exact: true }).click();
      await expect(rows).toHaveCount(50);
      expect(historyRequests.map((url) => url.searchParams.get("surface"))).toEqual(["api", "cli"]);

      await dialog.getByRole("button", { name: "Load more", exact: true }).click();
      await expect(rows).toHaveCount(51);
      expect(historyRequests.at(-1)?.search).toBe("?surface=api&limit=50&cursor=api-page-2");
      await expect(dialog.getByRole("button", { name: "Load more", exact: true })).toHaveCount(0);
      expect(await dialog.getByRole("tabpanel").evaluate((element) => element.scrollTop)).toBeGreaterThan(0);
      expect(await tabs.boundingBox()).toEqual(tabBounds);
      expect(await dialog.boundingBox()).toEqual(bounds);
      expect(await dialog.evaluate((element) => element.scrollHeight <= element.clientHeight)).toBe(true);
      for (const [tab, empty] of [["MCP", "No MCP calls."], ["Audit", "No audit events."], ["Jobs", "No background jobs yet."], ["Changes", "No changes yet."]]) {
        await dialog.getByRole("tab", { name: tab, exact: true }).click();
        await expect(dialog.getByText(empty, { exact: true })).toBeVisible();
        expect(await dialog.boundingBox()).toEqual(bounds);
        expect(await dialog.getByRole("tabpanel").evaluate((element) => element.scrollTop)).toBe(0);
      }

      await dialog.getByRole("button", { name: "Close", exact: true }).click();
      await page.getByRole("button", { name: "Settings", exact: true }).click();
      const settings = page.getByRole("dialog", { name: "Settings" });
      const settingsBounds = await settings.boundingBox();
      for (const tab of ["Account", "Agents", "General"]) {
        await settings.getByRole("tab", { name: tab, exact: true }).click();
        if (tab === "Account") {
          await expect(settings.getByText("Chrome on Linux · This session", { exact: true })).toBeAttached();
          await expect(settings.getByText("NoteGate CLI · CLI", { exact: true })).toBeAttached();
          await expect(settings.getByText("Remote MCP · MCP", { exact: true })).toBeAttached();
        }
        if (tab === "Agents") {
          await expect(settings.getByRole("button", { name: "Toggle Agent 29 details", exact: true })).toBeAttached();
          await settings.getByRole("button", { name: "Toggle Agent 29 details", exact: true }).scrollIntoViewIfNeeded();
          expect(await settings.getByRole("tabpanel").evaluate((element) => element.scrollTop)).toBeGreaterThan(0);
        } else {
          expect(await settings.getByRole("tabpanel").evaluate((element) => element.scrollTop)).toBe(0);
        }
        expect(await settings.boundingBox()).toEqual(settingsBounds);
        expect(await settings.evaluate((element) => element.scrollHeight <= element.clientHeight)).toBe(true);
        await expect(settings.getByRole("tablist")).toBeInViewport();
        await expect(settings.getByRole("button", { name: "Close", exact: true })).toBeInViewport();
        if (tab === "Account") {
          for (const [kind, client] of [["CLI", "NoteGate CLI"], ["MCP", "Remote MCP"]]) {
            const connections = settings.getByRole("region", { name: `${kind} connections` });
            await connections.getByRole("button", { name: `Revoke ${client}`, exact: true }).click();
            const confirmation = connections.getByRole("group", { name: "Confirm revocation" });
            await expect(confirmation).toBeFocused();
            await expect(confirmation.getByRole("button", { name: "Confirm", exact: true })).toBeInViewport();
            await page.screenshot({ path: `test-results/dialog-auth-${kind.toLowerCase()}-${viewport.name}-${theme}.png` });
            await confirmation.getByRole("button", { name: "Cancel", exact: true }).click();
          }
        }
      }
    });
  }
}
