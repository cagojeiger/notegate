import { expect, test, type Page, type Request } from "@playwright/test";

import { makeSpace } from "../src/test/fixtures";
import { routeJsonApi } from "./support/api";
import { openSettings } from "./support/settings";
import { usageResponse } from "./support/usage";

const space = makeSpace();
const meta = { limit: 50, returned: 0, has_more: false, next_cursor: null };
const key = {
  id: "key-a", name: "User A key", token: "dummy-user-a-one-time-secret",
  created_at: "2026-10-23T00:00:00Z", expires_at: "2026-11-22T00:00:00Z"
};

async function mockOwnerApi(page: Page, pendingMint: boolean) {
  let issued = false;
  let mintCalls = 0;
  let finishMint!: () => void;
  const meOwners: string[] = [];
  const ownerOf = async (request: Request) => {
    const cookie = await request.headerValue("cookie") ?? "";
    return cookie.includes("notegate_browser_session=session-b") ? "b" : "a";
  };
  await routeJsonApi(page, (url) => {
    if (url.pathname === "/api/v1/me/usage") return usageResponse(space);
    if (url.pathname === "/api/v1/spaces") return { spaces: [space], page: meta };
    if (url.pathname === "/api/v1/me/sessions") return { sessions: [] };
    if (url.pathname.endsWith("/agents")) return { connections: [], page: meta };
    if (url.pathname.endsWith("/children")) return { parent: { id: space.root_node_id, path: "/" }, children: [], page: meta };
    if (url.pathname.endsWith("/nodes")) return { nodes: [], page: meta };
    if (url.pathname.endsWith("/file-change-sync")) return { changes: [], next_after_id: 0, has_more: false, resync_required: false };
    throw new Error(`Unhandled API request: ${url.pathname}`);
  });
  await page.route("**/api/v1/me", async (route) => {
    const owner = await ownerOf(route.request());
    meOwners.push(owner);
    await route.fulfill({ status: 200, contentType: "application/json", body: JSON.stringify({
      account: { id: `user-${owner}`, kind: "user", display_name: `User ${owner.toUpperCase()}` },
      user: { email: `user-${owner}@example.invalid` },
      capabilities: { can_create_space: true, can_manage_agents: true }
    }) });
  });
  await page.route("**/api/v1/agents?*", async (route) => {
    const owner = await ownerOf(route.request());
    await route.fulfill({ status: owner === "a" ? 200 : 503, contentType: "application/json",
      body: JSON.stringify(owner === "a"
        ? { agents: [{ id: "agent-a", name: "Agent A", owner_user_id: "user-a" }], page: meta }
        : { message: "Agent list temporarily unavailable", kind: "unavailable" }) });
  });
  await page.route("**/api/v1/agents/agent-a/keys*", async (route) => {
    if (await ownerOf(route.request()) !== "a") {
      return route.fulfill({ status: 403, contentType: "application/json", body: JSON.stringify({ message: "Not your agent", kind: "forbidden" }) });
    }
    if (route.request().method() === "POST") {
      mintCalls += 1;
      if (pendingMint) await new Promise<void>((resolve) => { finishMint = resolve; });
      issued = true;
      return route.fulfill({ status: 201, contentType: "application/json", body: JSON.stringify(key) });
    }
    return route.fulfill({ status: 200, contentType: "application/json", body: JSON.stringify({
      keys: issued ? [{ id: key.id, name: key.name, created_at: key.created_at, expires_at: key.expires_at,
        account_id: "agent-a", scopes: [], revoked_at: null }] : [], page: meta
    }) });
  });
  return { mintCalls: () => mintCalls, finishMint: () => finishMint(), meOwners };
}

async function reconnect(page: Page) {
  // Advance the injected browser clock beyond the query freshness window.
  await page.clock.runFor(6_000);
  const me = page.waitForResponse((response) => new URL(response.url()).pathname === "/api/v1/me");
  await page.evaluate(() => {
    window.dispatchEvent(new Event("offline"));
    window.dispatchEvent(new Event("online"));
  });
  await me;
}

for (const pendingMint of [false, true]) {
  test(`a successful /me owner change discards ${pendingMint ? "a late mint" : "a retained token"} from another browser tab`, async ({ page, baseURL }, testInfo) => {
    const origin = new URL(baseURL!).origin;
    await page.clock.install();
    await page.context().addCookies([{ name: "notegate_browser_session", value: "session-a", url: origin, httpOnly: true, sameSite: "Lax" }]);
    const api = await mockOwnerApi(page, pendingMint);
    await page.goto("/");
    let view = await openSettings(page, "Agents");
    await view.getByRole("button", { name: "Select Agent A", exact: true }).click();
    await view.getByRole("button", { name: "Add API key", exact: true }).click();
    await view.getByRole("textbox", { name: "Name", exact: true }).fill(key.name);
    await view.getByRole("button", { name: "Create", exact: true }).click();
    await expect.poll(api.mintCalls).toBe(1);
    if (!pendingMint) await expect(view.getByText(key.token, { exact: true })).toBeVisible();
    await view.getByRole("button", { name: "Close settings", exact: true }).click();

    if (!pendingMint) {
      await reconnect(page);
      view = await openSettings(page, "Agents");
      await view.getByRole("button", { name: "Select Agent A", exact: true }).click();
      await expect(view.getByText(key.token, { exact: true })).toBeVisible();
      await view.getByRole("button", { name: "Close settings", exact: true }).click();
    }

    // Model the login callback in a second tab with the production cookie flags.
    const loginTab = await page.context().newPage();
    await loginTab.route("**/__test__/login-complete", (route) => route.fulfill({
      status: 200, contentType: "text/html",
      headers: { "set-cookie": "notegate_browser_session=session-b; Path=/; HttpOnly; SameSite=Lax" },
      body: "<p>Mock login callback completed for User B</p>"
    }));
    await loginTab.goto(`${origin}/__test__/login-complete`);
    expect((await page.context().cookies()).find((cookie) => cookie.name === "notegate_browser_session")?.value).toBe("session-b");
    await reconnect(page);
    view = await openSettings(page, "Account");
    await expect(view.getByText("user-b@example.invalid", { exact: true })).toBeVisible();

    if (pendingMint) {
      const response = page.waitForResponse((item) => item.request().method() === "POST" && item.url().endsWith("/agents/agent-a/keys"));
      api.finishMint();
      await response;
    }
    await view.getByRole("tab", { name: "Agents", exact: true }).click();
    const staleAgent = view.getByRole("button", { name: "Select Agent A", exact: true });
    const oldAgentVisible = await staleAgent.count() > 0;
    if (oldAgentVisible) {
      await staleAgent.click();
      await expect(view.getByText("Could not load API keys.", { exact: true })).toBeVisible();
    } else await expect(view.getByText("Could not load agents.", { exact: true })).toBeVisible();
    await page.screenshot({ path: testInfo.outputPath("owner-b-settings.png") });
    expect(api.meOwners).toContain("b");
    await expect(view.getByText(key.token, { exact: true })).toHaveCount(0);
    expect(oldAgentVisible).toBe(false);
  });
}
