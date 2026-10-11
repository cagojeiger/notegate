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
  { name: "mobile", width: 390, height: 844 }
]) {
  for (const theme of ["light", "dark"] as const) {
    test(`API history stays within the panel and fetches on demand on ${viewport.name} in ${theme} mode`, async ({ page }, testInfo) => {
      await page.setViewportSize(viewport);
      // Cache freshness must not depend on CI runner speed.
      await page.clock.setFixedTime(new Date("2026-10-11T14:00:00Z"));
      await page.addInitScript((value) => window.localStorage.setItem("notegate.theme", value), theme);
      const historyRequests: URL[] = [];
      await routeJsonApi(page, (url) => {
        if (url.pathname === "/api/v1/me") return { account: { id: "user-1", kind: "user", display_name: "User" }, user: { email: "user@example.com" }, capabilities: { can_create_space: true, can_manage_agents: true } };
        if (url.pathname === "/api/v1/me/usage") return usageResponse(space);
        if (url.pathname === "/api/v1/spaces") return { spaces: [space], page: emptyPage };
        if (url.pathname.endsWith("/children")) return { parent: { id: space.root_node_id, path: "/" }, children: [], page: emptyPage };
        if (url.pathname.endsWith("/nodes")) return { nodes: [], page: emptyPage };
        if (url.pathname.endsWith("/file-change-sync")) return { changes: [], next_after_id: 0, has_more: false, resync_required: false };
        if (url.pathname.endsWith("/file-change-events")) return { events: [], page: emptyPage };
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
      expect(historyRequests).toHaveLength(0);
      await dialog.getByRole("tab", { name: "API", exact: true }).click();
      const rows = dialog.locator("ol > li");
      await expect(rows).toHaveCount(50);
      expect(historyRequests.map((url) => url.search)).toEqual(["?surface=api&limit=50"]);
      const panel = dialog.locator("section");
      expect(await panel.evaluate((element) => element.scrollWidth <= element.clientWidth)).toBe(true);

      await rows.first().getByText("Response", { exact: true }).click();
      await expect(rows.first().locator("pre")).toContainText('"status": 200');
      expect(historyRequests).toHaveLength(1);
      await testInfo.attach(`invocation-history-${viewport.name}-${theme}`, {
        body: await page.screenshot({ path: `test-results/invocation-history-${viewport.name}-${theme}.png` }),
        contentType: "image/png"
      });

      await dialog.getByRole("tab", { name: "CLI", exact: true }).click();
      await expect(dialog.getByText("No CLI calls.")).toBeVisible();
      await dialog.getByRole("tab", { name: "API", exact: true }).click();
      await expect(rows).toHaveCount(50);
      expect(historyRequests.map((url) => url.searchParams.get("surface"))).toEqual(["api", "cli"]);

      await dialog.getByRole("button", { name: "Load more", exact: true }).click();
      await expect(rows).toHaveCount(51);
      expect(historyRequests.at(-1)?.search).toBe("?surface=api&limit=50&cursor=api-page-2");
      await expect(dialog.getByRole("button", { name: "Load more", exact: true })).toHaveCount(0);
    });
  }
}
