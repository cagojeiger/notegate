import { expect, test } from "@playwright/test";

test("standalone CLI approval and connection revocation", async ({ browser, playwright, baseURL }) => {
  const session = process.env.NOTEGATE_WEB_E2E_BROWSER_SESSION;
  expect(Boolean(session)).toBe(true);
  const request = await playwright.request.newContext({ baseURL });
  const context = await browser.newContext({ baseURL });
  try {
    await context.addCookies([{ name: "notegate_browser_session", value: session!, url: baseURL!, httpOnly: true, sameSite: "Lax" }]);
    const metadata = await (await request.get("/.well-known/oauth-authorization-server")).json();
    expect(metadata.issuer).toBe(baseURL);
    expect(metadata.client_id_metadata_document_supported).toBe(true);
    const deviceResponse = await request.post("/oauth/device/authorize", {
      form: { client_id: metadata.cli_client_id, scope: "cli offline_access" }
    });
    expect(deviceResponse.status()).toBe(200);
    const device = await deviceResponse.json();
    const page = await context.newPage();
    await page.goto(device.verification_uri_complete);
    await expect(page.getByRole("heading", { name: "Connect NoteGate CLI" })).toBeVisible();
    const approvalRequest = page.waitForRequest((request) => request.method() === "POST" && new URL(request.url()).pathname === "/oauth/device");
    await page.getByRole("button", { name: "Connect CLI", exact: true }).click();
    expect((await (await approvalRequest).allHeaders()).origin).toBe(new URL(baseURL!).origin);
    await expect(page.getByRole("heading", { name: "Request completed" })).toBeVisible();
    const tokenResponse = await request.post("/oauth/token", {
      form: {
        grant_type: "urn:ietf:params:oauth:grant-type:device_code",
        client_id: metadata.cli_client_id,
        device_code: device.device_code
      }
    });
    expect(tokenResponse.status()).toBe(200);
    expect(tokenResponse.headers()["cache-control"]).toBe("no-store");
    const tokens = await tokenResponse.json();
    expect(typeof tokens.access_token).toBe("string");
    await page.goto("/oauth/connections");
    await expect(page.getByRole("heading", { name: "Connected applications" })).toBeVisible();
    await expect(page.getByText("NoteGate CLI", { exact: true })).toBeVisible();
    await page.screenshot({ path: "test-results/browser-performance/oauth-connections.png" });
    await page.getByRole("button", { name: "Revoke connection" }).click();
    await expect(page.getByRole("button", { name: "Revoke connection" })).toHaveCount(0);
    const revoked = await request.post("/oauth/token", {
      form: { grant_type: "refresh_token", client_id: metadata.cli_client_id, refresh_token: tokens.refresh_token }
    });
    expect(revoked.status()).toBe(400);
    expect((await revoked.json()).error).toBe("invalid_grant");
  } finally {
    await context.close();
    await request.dispose();
  }
});
