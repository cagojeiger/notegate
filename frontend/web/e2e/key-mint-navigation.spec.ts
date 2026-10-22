import { expect, test, type Page } from "@playwright/test";

import { makeSpace } from "../src/test/fixtures";
import { routeJsonApi } from "./support/api";
import { openSettings } from "./support/settings";
import { usageResponse } from "./support/usage";

const space = makeSpace();
const meta = { limit: 50, returned: 0, has_more: false, next_cursor: null };
const mintedKey = {
  id: "key-review", name: "Review key", token: "dummy-one-time-key",
  created_at: "2026-10-22T00:00:00Z", expires_at: "2026-11-21T00:00:00Z"
};

async function mockKeyIssuance(page: Page) {
  let issued = false;
  let calls = 0;
  let finish!: () => void;
  await routeJsonApi(page, (url) => {
    if (url.pathname === "/api/v1/me") return {
      account: { id: "user-1", kind: "user", display_name: "Review user" },
      user: { email: "review@example.invalid" },
      capabilities: { can_create_space: true, can_manage_agents: true }
    };
    if (url.pathname === "/api/v1/me/usage") return usageResponse(space);
    if (url.pathname === "/api/v1/spaces") return { spaces: [space], page: meta };
    if (url.pathname === "/api/v1/me/sessions") return { sessions: [] };
    if (url.pathname === "/api/v1/agents") return {
      agents: [{ id: "agent-review", name: "Review agent", owner_user_id: "user-1" }], page: meta
    };
    if (url.pathname.endsWith("/keys")) return {
      keys: issued ? [{ id: mintedKey.id, name: mintedKey.name, created_at: mintedKey.created_at,
        expires_at: mintedKey.expires_at, account_id: "agent-review", scopes: [], revoked_at: null }] : [], page: meta
    };
    if (url.pathname.endsWith("/agents")) return { connections: [], page: meta };
    if (url.pathname.endsWith("/children")) return { parent: { id: space.root_node_id, path: "/" }, children: [], page: meta };
    if (url.pathname.endsWith("/nodes")) return { nodes: [], page: meta };
    if (url.pathname.endsWith("/file-change-sync")) return { changes: [], next_after_id: 0, has_more: false, resync_required: false };
    throw new Error(`Unhandled request: ${url.pathname}`);
  });
  await page.route("**/api/v1/agents/agent-review/keys", async (route) => {
    calls += 1;
    await new Promise<void>((resolve) => { finish = resolve; });
    issued = true;
    await route.fulfill({ status: 201, contentType: "application/json", body: JSON.stringify(mintedKey) });
  });
  return { calls: () => calls, finish: () => finish() };
}

for (const width of [1440, 390]) {
  test.describe(`one-time key issuance at ${width}px`, () => {
    test.use({ viewport: { width, height: 900 }, hasTouch: width < 768 });

    for (const navigation of ["Back to agents", "Account tab", "Close settings"]) {
      test(`retains the pending mint and token through ${navigation} until Done`, async ({ page }) => {
        const issuance = await mockKeyIssuance(page);
        await page.goto("/");
        const view = await openSettings(page, "Agents");
        await view.getByRole("button", { name: "Select Review agent", exact: true }).click();
        await view.getByRole("button", { name: "Add API key", exact: true }).click();
        await view.getByRole("textbox", { name: "Name", exact: true }).fill(mintedKey.name);
        await view.getByRole("button", { name: "Create", exact: true }).click();
        await expect.poll(issuance.calls).toBe(1);

        const leave = async () => {
          if (navigation === "Account tab") await view.getByRole("tab", { name: "Account", exact: true }).click();
          else await view.getByRole("button", { name: navigation, exact: true }).click();
        };
        const returnToAgent = async () => {
          if (navigation === "Account tab") {
            await page.keyboard.press("ArrowRight");
            await expect(view.getByRole("tab", { name: "Connections", exact: true })).toBeFocused();
            await page.keyboard.press("ArrowRight");
            await expect(view.getByRole("tab", { name: "Agents", exact: true })).toBeFocused();
          }
          else {
            if (navigation === "Close settings") await openSettings(page, "Agents");
            await view.getByRole("button", { name: "Select Review agent", exact: true }).click();
          }
        };

        await leave();
        await returnToAgent();
        await expect(view.getByRole("status")).toHaveText("Creating API key…");
        await expect(view.getByRole("button", { name: "Add API key", exact: true })).toBeDisabled();

        await leave();
        const response = page.waitForResponse((response) => response.request().method() === "POST"
          && response.url().endsWith("/api/v1/agents/agent-review/keys"));
        issuance.finish();
        await response;
        await returnToAgent();
        await expect(view.getByRole("button", { name: "Copy token", exact: true })).toBeVisible();
        await expect(view.getByText(mintedKey.token, { exact: true })).toBeVisible();
        await expect(view.getByRole("button", { name: "Add API key", exact: true })).toBeDisabled();
        if (navigation === "Account tab") await expect(view.getByRole("tab", { name: "Agents", exact: true })).toBeFocused();

        await leave();
        await returnToAgent();
        await expect(view.getByText(mintedKey.token, { exact: true })).toBeVisible();
        await view.getByRole("button", { name: "Done", exact: true }).click();
        await expect(view.getByText(mintedKey.token, { exact: true })).toHaveCount(0);
        await expect(view.getByRole("button", { name: "Add API key", exact: true })).toBeEnabled();
        await leave();
        await returnToAgent();
        await expect(view.getByRole("button", { name: "Copy token", exact: true })).toHaveCount(0);
        await expect(view.getByRole("button", { name: "Revoke Review key", exact: true })).toBeVisible();
        expect(issuance.calls()).toBe(1);
      });
    }
  });
}
