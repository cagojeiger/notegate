import { expect, test, type Page } from "@playwright/test";

import { makeSpace } from "../src/test/fixtures";
import { routeJsonApi } from "./support/api";
import { openSettings, showSettingsInspector } from "./support/settings";
import { usageResponse } from "./support/usage";

const space = makeSpace();
const meta = { limit: 50, returned: 0, has_more: false, next_cursor: null };
const dates = { created_at: "2026-10-18T00:00:00Z", idle_expires_at: "2026-11-02T00:00:00Z", expires_at: "2026-11-18T00:00:00Z" };

async function mockSettings(page: Page) {
  const mutations: string[] = [];
  const trash = ["meeting.md", "other.md"].map((name, index) => ({
    id: `deleted-${index}`, space_id: space.id, space_name: space.name, kind: "text", name,
    path: `/notes/${name}`, deleted_at: dates.created_at, purge_after: dates.expires_at,
    recoverable: true, deletion_pending: false
  }));
  await routeJsonApi(page, (url, request) => {
    if (request.method() !== "GET") mutations.push(`${request.method()} ${url.pathname}`);
    if (url.pathname === "/api/v1/me") return { account: { id: "user-1", kind: "user", display_name: "User" }, user: { email: "user@example.com" }, capabilities: { can_create_space: true, can_manage_agents: true } };
    if (url.pathname === "/api/v1/me/usage") return usageResponse(space);
    if (url.pathname === "/api/v1/spaces") return { spaces: [space], page: meta };
    if (url.pathname === "/api/v1/me/trash") return { items: trash, page: meta };
    if (url.pathname === "/api/v1/me/sessions") return { sessions: [{ ...dates, id: "session-1", current: false, client_environment: "Other browser", last_used_at: dates.created_at }] };
    if (url.pathname === "/api/v1/me/oauth-grants") return { grants: [{ ...dates, id: "grant-1", kind: "mcp", client_name: "MCP App", client_environment: "macOS", last_token_at: dates.created_at, offline_access: true }] };
    if (url.pathname === "/api/v1/agents") return { agents: [{ id: "agent-1", name: "Review agent", owner_user_id: "user-1" }], page: meta };
    if (url.pathname.endsWith("/keys")) return { keys: [] };
    if (url.pathname.endsWith("/agents")) return { connections: [], page: meta };
    if (url.pathname.endsWith("/children")) return { parent: { id: space.root_node_id, path: "/" }, children: [], page: meta };
    if (url.pathname.endsWith("/nodes")) return { nodes: [], page: meta };
    if (url.pathname.endsWith("/file-change-sync")) return { changes: [], next_after_id: 0, has_more: false, resync_required: false };
    throw new Error(`Unhandled request: ${request.method()} ${url.pathname}`);
  });
  return { mutations, trash };
}

type Dismiss = "close" | "toggle" | "escape" | "backdrop";
async function dismissInspector(page: Page, mobile: boolean, event: Dismiss) {
  if (event === "escape") await page.keyboard.press("Escape");
  else if (event === "backdrop") await page.locator('[data-settings-view] button[aria-hidden="true"]').click({ position: { x: 5, y: 5 } });
  else if (event === "toggle") await page.getByRole("button", { name: "Toggle settings inspector", exact: true }).click();
  else await page.getByRole("button", { name: mobile ? "Close" : "Close settings inspector", exact: true }).click();
  await expect(page.getByRole("button", { name: "Toggle settings inspector", exact: true })).toHaveAttribute("aria-pressed", "false");
}

for (const width of [1440, 390]) {
  const mobile = width < 768;
  test.describe(`settings FSM at ${width}px`, () => {
    test.use({ viewport: { width, height: 900 }, hasTouch: mobile });

    test("dismissing an unsubmitted confirmation returns each feature to browsing", async ({ page }) => {
      const { mutations } = await mockSettings(page);
      await page.goto("/");
      const view = await openSettings(page, "Trash");
      const targets = [
        { tab: "Trash", trigger: "Permanently delete meeting.md", confirmation: "Confirm permanent deletion", role: "region", row: "Select meeting.md" },
        { tab: "Account", trigger: "Sign out Other browser", confirmation: "Confirm revocation", role: "group", row: "Select Other browser" },
        { tab: "Connections", trigger: "Revoke MCP App", confirmation: "Confirm revocation", role: "group", row: "Select MCP App" },
        { tab: "Agents", trigger: "Delete Review agent", confirmation: "Delete this agent and revoke its API keys?", role: "text", row: null }
      ] as const;
      for (const target of targets) {
        await view.getByRole("tab", { name: target.tab, exact: true }).click();
        if (target.tab === "Agents") await view.getByRole("button", { name: "Select Review agent", exact: true }).click();
        if (target.tab === "Connections") await view.getByRole("tab", { name: "MCP", exact: true }).click();
        for (const event of mobile ? ["close", "escape", "backdrop"] as const : ["close", "toggle"] as const) {
          await showSettingsInspector(page);
          await view.getByRole("button", { name: target.trigger, exact: true }).click();
          const confirmation = target.role === "text" ? view.getByText(target.confirmation, { exact: true }) : view.getByRole(target.role, { name: target.confirmation, exact: true });
          await expect(confirmation).toBeVisible();
          await dismissInspector(page, mobile, event);
          if (target.row) await expect(view.getByRole("button", { name: target.row })).toBeEnabled();
          if (target.tab === "Trash") await expect(view.getByRole("button", { name: "Refresh", exact: true })).toBeEnabled();
          await showSettingsInspector(page);
          await expect(confirmation).toHaveCount(0);
          await expect(view.getByRole("button", { name: target.trigger, exact: true })).toBeVisible();
          expect(mutations).toEqual([]);
          await dismissInspector(page, mobile, "close");
        }
        await showSettingsInspector(page);
        await view.getByRole("button", { name: target.trigger, exact: true }).click();
        const confirmation = target.role === "text" ? view.getByText(target.confirmation, { exact: true }) : view.getByRole(target.role, { name: target.confirmation, exact: true });
        await expect(confirmation).toBeVisible();
        await page.setViewportSize({ width: mobile ? 1440 : 390, height: 900 });
        if (mobile) await expect(confirmation).toBeVisible();
        else await expect(page.getByRole("button", { name: "Toggle settings inspector", exact: true })).toHaveAttribute("aria-pressed", "false");
        await page.setViewportSize({ width, height: 900 });
        if (mobile) {
          await expect(confirmation).toBeVisible();
          await dismissInspector(page, mobile, "close");
          await showSettingsInspector(page);
        }
        await expect(confirmation).toHaveCount(0);
        if (!mobile) {
          await view.getByRole("button", { name: target.trigger, exact: true }).click();
          await view.getByRole("tab", { name: "General", exact: true }).click();
          await view.getByRole("tab", { name: target.tab, exact: true }).click();
          await expect(confirmation).toHaveCount(0);
        }
        expect(mutations).toEqual([]);
        await dismissInspector(page, mobile, "close");
      }
    });

    for (const outcome of ["success", "error"] as const) {
      test(`closing a pending deletion preserves its ${outcome} and never resubmits`, async ({ page }) => {
        const { trash } = await mockSettings(page);
        let requests = 0;
        let release!: () => void;
        const response = new Promise<void>((resolve) => { release = resolve; });
        await page.route("**/api/v1/me/trash/spaces/*/nodes/deleted-0?**", async (route) => {
          requests++;
          await response;
          if (outcome === "success") {
            trash[0] = { ...trash[0], recoverable: false, deletion_pending: true };
            await route.fulfill({ status: 202, contentType: "application/json", body: JSON.stringify({ status: "deletion_requested" }) });
          } else await route.fulfill({ status: 503, contentType: "application/json", body: JSON.stringify({ message: "Storage unavailable", kind: "unavailable" }) });
        });
        await page.goto("/");
        const view = await openSettings(page, "Trash");
        await showSettingsInspector(page);
        await view.getByRole("button", { name: "Permanently delete meeting.md", exact: true }).click();
        await view.getByRole("button", { name: "Permanently delete", exact: true }).click();
        try {
          await expect.poll(() => requests).toBe(1);
          await dismissInspector(page, mobile, "close");
          await expect(view.getByRole("button", { name: "Select other.md" })).toBeDisabled();
          await showSettingsInspector(page);
          await expect(view.getByRole("region", { name: "Confirm permanent deletion", exact: true })).toBeVisible();
          await expect(view.getByRole("button", { name: "Requesting…", exact: true })).toBeDisabled();
          expect(requests).toBe(1);
          await dismissInspector(page, mobile, "close");
        } finally { release(); }
        await expect(view.getByRole("button", { name: "Select other.md" })).toBeEnabled();
        await showSettingsInspector(page);
        await expect(view.getByRole("region", { name: "Confirm permanent deletion", exact: true })).toHaveCount(0);
        if (outcome === "success") {
          await expect(view.getByRole("status")).toContainText("Permanent deletion requested");
          await expect(view.getByRole("button", { name: "Restore meeting.md", exact: true })).toBeDisabled();
        } else {
          await expect(view.getByRole("alert")).toHaveText("Storage unavailable");
          await expect(view.getByRole("button", { name: "Restore meeting.md", exact: true })).toBeEnabled();
        }
        expect(requests).toBe(1);
      });
    }
  });
}
