import { expect, test, type Locator, type Page, type TestInfo } from "@playwright/test";

import { makeSpace } from "../src/test/fixtures";
import { expectNoAccessibilityViolations } from "./support/accessibility";
import { routeJsonApi } from "./support/api";
import { closeSettingsInspector, openSettings, showSettingsInspector } from "./support/settings";
import { usageResponse } from "./support/usage";

const space = makeSpace();
const meta = { limit: 100, returned: 0, has_more: false, next_cursor: null };
const dates = { created_at: "2026-10-11T13:00:00Z", idle_expires_at: "2026-11-10T13:00:00Z", expires_at: "2026-12-10T13:00:00Z" };
const fixtureToken = "preview-token-not-a-credential";

async function fixture(page: Page, theme: "light" | "dark") {
  await page.clock.setFixedTime(new Date("2026-10-23T00:00:00Z"));
  await page.addInitScript((value) => {
    localStorage.setItem("notegate.theme", value);
    Object.defineProperty(navigator, "clipboard", {
      configurable: true,
      value: { writeText: async () => { throw new Error("Clipboard unavailable in this fixture"); } }
    });
  }, theme);
  const data = {
    sessions: [
      { ...dates, id: "current", current: true, client_environment: "This browser", last_used_at: null },
      { ...dates, id: "remote", current: false, client_environment: "Other browser", last_used_at: null }
    ],
    grants: [
      { ...dates, id: "cli", client_name: "NoteGate CLI", kind: "cli", client_environment: null, last_token_at: null, offline_access: true },
      { ...dates, id: "mcp", client_name: "Remote MCP", kind: "mcp", client_environment: null, last_token_at: null, offline_access: true }
    ],
    keys: [{ id: "key-existing", name: "Existing key", created_at: dates.created_at, expires_at: dates.expires_at }],
    permission: "write" as "read" | "write"
  };
  const connection = () => ({ agent: { id: "agent-preview", kind: "agent", display_name: "Preview agent" }, permission: data.permission, connected_at: dates.created_at });
  await routeJsonApi(page, (url, request) => {
    if (request.method() !== "GET") throw new Error(`Unhandled fixture mutation: ${request.method()} ${url.pathname}`);
    if (url.pathname === "/api/v1/me") return { account: { id: "user-1", kind: "user", display_name: "Preview user" }, user: { email: "preview@example.com" }, capabilities: { can_create_space: true, can_manage_agents: true } };
    if (url.pathname === "/api/v1/me/sessions") return { sessions: data.sessions };
    if (url.pathname === "/api/v1/me/oauth-grants") return { grants: data.grants };
    if (url.pathname === "/api/v1/me/usage") return usageResponse(space);
    if (url.pathname === "/api/v1/spaces") return { spaces: [space], page: meta };
    if (url.pathname === "/api/v1/agents") return { agents: [{ id: "agent-preview", name: "Preview agent", owner_user_id: "user-1" }], page: meta };
    if (url.pathname === "/api/v1/agents/agent-preview/keys") return { keys: data.keys, page: meta };
    if (url.pathname === `/api/v1/spaces/${space.id}/agents`) return { connections: [connection()], page: meta };
    if (url.pathname.endsWith("/children")) return { parent: { id: space.root_node_id, path: "/" }, children: [], page: meta };
    if (url.pathname.endsWith("/nodes")) return { nodes: [], page: meta };
    if (url.pathname.endsWith("/file-change-sync")) return { changes: [], next_after_id: 0, has_more: false, resync_required: false };
    throw new Error(`Unhandled fixture read: ${url.pathname}`);
  });
  return { data, connection };
}

async function capture(page: Page, view: Locator, testInfo: TestInfo, name: string, target: Locator) {
  await target.scrollIntoViewIfNeeded();
  await expect(target).toBeVisible();
  expect(await view.evaluate((element) => element.scrollWidth <= element.clientWidth)).toBe(true);
  // Scope this auth feedback check to Settings, independent of the inactive Files tree.
  await expectNoAccessibilityViolations(page, "[data-settings-view]");
  await testInfo.attach(name, {
    body: await page.screenshot({
      path: `test-results/settings-auth-${name}.png`,
      mask: [view.locator("code").filter({ hasText: fixtureToken })],
      maskColor: "#9ca3af"
    }),
    contentType: "image/png"
  });
}

for (const profile of [
  { name: "desktop-light", width: 1440, theme: "light" },
  { name: "mobile-dark", width: 390, theme: "dark" }
] as const) {
  test.describe(profile.name, () => {
    test.use({ viewport: { width: profile.width, height: 900 }, hasTouch: profile.width < 768 });

    test("connection feedback preserves browser and CLI access through an explicit retry", async ({ page }, testInfo) => {
      const { data } = await fixture(page, profile.theme);
      let release: () => void = () => undefined;
      const response = new Promise<void>((resolve) => { release = resolve; });
      let requests = 0;
      await page.route("**/api/v1/me/oauth-grants/mcp", async (route) => {
        if (route.request().method() !== "DELETE") return route.fallback();
        requests += 1;
        if (requests === 1) {
          await response;
          return route.fulfill({ status: 503, contentType: "application/json", body: JSON.stringify({ message: "Unavailable", kind: "unavailable" }) });
        }
        data.grants = data.grants.filter((grant) => grant.id !== "mcp");
        return route.fulfill({ status: 204 });
      });
      await page.goto("/");
      const view = await openSettings(page, "Account");
      await expect(view.getByRole("list", { name: "Browser sessions" }).getByRole("button")).toHaveCount(2);
      await closeSettingsInspector(page);
      await view.getByRole("tab", { name: "Connections", exact: true }).click();
      await view.getByRole("tab", { name: "MCP", exact: true }).click();
      await view.getByRole("button", { name: "Select Remote MCP", exact: true }).click();
      await showSettingsInspector(page);
      await view.getByRole("button", { name: "Revoke Remote MCP", exact: true }).click();
      const confirmation = view.getByRole("group", { name: "Confirm revocation", exact: true });
      const confirm = confirmation.getByRole("button", { name: "Confirm", exact: true });
      try {
        await confirm.click();
        await expect(confirm).toBeDisabled();
        await expect(confirm).toHaveAttribute("aria-busy", "true");
        await expect(confirmation.getByRole("button", { name: "Cancel", exact: true })).toBeDisabled();
        await expect(view.getByRole("button", { name: "Select Remote MCP", exact: true })).toBeDisabled();
        expect(requests).toBe(1);
        await capture(page, view, testInfo, `revoke-pending-${profile.name}`, confirmation);
      } finally { release(); }
      const failure = view.getByRole("alert");
      await expect(failure).toHaveText("Could not revoke access. Try again.");
      await expect(view.getByText("Remote MCP · MCP", { exact: true })).toBeVisible();
      await capture(page, view, testInfo, `revoke-error-${profile.name}`, failure);
      await confirm.click();
      const success = view.getByRole("status").filter({ hasText: "Access revoked." });
      await expect(success).toBeFocused();
      await expect(view.getByRole("alert")).toHaveCount(0);
      await expect(confirmation).toHaveCount(0);
      await capture(page, view, testInfo, `revoke-success-${profile.name}`, success);
      await closeSettingsInspector(page);
      await view.getByRole("tab", { name: "CLI", exact: true }).click();
      await expect(view.getByRole("button", { name: "Select NoteGate CLI", exact: true })).toBeVisible();
      await closeSettingsInspector(page);
      await view.getByRole("tab", { name: "Account", exact: true }).click();
      await expect(view.getByRole("list", { name: "Browser sessions" }).getByRole("button")).toHaveCount(2);
      expect(requests).toBe(2);
    });

    test("key and permission feedback keeps the token outside announcements", async ({ page }, testInfo) => {
      const { data, connection } = await fixture(page, profile.theme);
      let permissionRequests = 0;
      await page.route(`**/api/v1/spaces/${space.id}/agents/agent-preview`, async (route) => {
        if (route.request().method() !== "PUT") return route.fallback();
        permissionRequests += 1;
        if (permissionRequests === 1) return route.fulfill({ status: 503, contentType: "application/json", body: JSON.stringify({ message: "Unavailable", kind: "unavailable" }) });
        data.permission = "read";
        return route.fulfill({ status: 200, contentType: "application/json", body: JSON.stringify(connection()) });
      });
      let release: () => void = () => undefined;
      const response = new Promise<void>((resolve) => { release = resolve; });
      let keyRequests = 0;
      await page.route("**/api/v1/agents/agent-preview/keys", async (route) => {
        if (route.request().method() !== "POST") return route.fallback();
        keyRequests += 1;
        if (keyRequests === 1) {
          await response;
          return route.fulfill({ status: 503, contentType: "application/json", body: JSON.stringify({ message: "Key service unavailable", kind: "unavailable" }) });
        }
        const key = { id: "key-preview", name: "Preview key", created_at: dates.created_at, expires_at: dates.expires_at };
        data.keys.push(key);
        return route.fulfill({ status: 201, contentType: "application/json", body: JSON.stringify({ ...key, token: fixtureToken }) });
      });
      await page.goto("/");
      const view = await openSettings(page, "Agents");
      await view.getByRole("button", { name: "Select Preview agent", exact: true }).click();
      await closeSettingsInspector(page);
      const permission = view.getByRole("combobox", { name: "Daily permission", exact: true });
      await permission.selectOption("read");
      const permissionError = view.getByRole("alert");
      await expect(permissionError).toHaveText("Could not update access. Check the current permission and try again.");
      await expect(permission).toHaveValue("write");
      await capture(page, view, testInfo, `permission-error-${profile.name}`, permissionError);
      await permission.selectOption("read");
      await expect(view.getByText("Space access updated.", { exact: true })).toHaveAttribute("role", "status");
      await expect(permission).toHaveValue("read");
      await view.getByRole("button", { name: "Add API key", exact: true }).click();
      await view.getByLabel("Name", { exact: true }).fill("Preview key");
      const create = view.getByRole("button", { name: "Create", exact: true });
      try {
        await create.click();
        await expect(create).toBeDisabled();
        await expect(create).toHaveAttribute("aria-busy", "true");
        await capture(page, view, testInfo, `key-pending-${profile.name}`, view.getByText("Creating API key…", { exact: true }));
      } finally { release(); }
      const keyError = view.getByRole("alert");
      await expect(keyError).toHaveText("Key service unavailable");
      await expect(view.getByRole("button", { name: "Copy token", exact: true })).toHaveCount(0);
      await expect(view.getByLabel("Name", { exact: true })).toHaveValue("Preview key");
      await capture(page, view, testInfo, `key-error-${profile.name}`, keyError);
      await create.click();
      const token = view.locator("code").filter({ hasText: fixtureToken });
      await expect(token).toBeVisible();
      expect(await token.evaluate((element) => element.closest('[role="status"], [role="alert"], [aria-live]') === null)).toBe(true);
      await capture(page, view, testInfo, `key-created-${profile.name}`, view.getByRole("status").filter({ hasText: "Key “Preview key” created" }));
      await view.getByRole("button", { name: "Copy token", exact: true }).click();
      const copyError = view.getByRole("alert");
      await expect(copyError).toContainText("Could not copy token.");
      await expect(token).toBeVisible();
      expect((await view.locator('[role="status"], [role="alert"], [aria-live]').allTextContents()).join(" ")).not.toContain(fixtureToken);
      await view.getByRole("button", { name: "Done", exact: true }).scrollIntoViewIfNeeded();
      await capture(page, view, testInfo, `key-copy-error-${profile.name}`, copyError);
      await view.getByRole("button", { name: "Done", exact: true }).click();
      await expect(token).toHaveCount(0);
      await expect(view.getByRole("button", { name: "Revoke Existing key", exact: true })).toBeVisible();
      await expect(view.getByRole("button", { name: "Revoke Preview key", exact: true })).toBeVisible();
      expect(permissionRequests).toBe(2);
      expect(keyRequests).toBe(2);
    });
  });
}
