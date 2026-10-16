import { expect, test } from "@playwright/test";

// Authorize contains a disposable CI key. Never retain its DOM or network headers.
test.use({ trace: "off", screenshot: "off", video: "off" });

test("embedded Swagger UI loads locally and executes the Agent API", async ({ page }) => {
  const session = process.env.NOTEGATE_WEB_E2E_BROWSER_SESSION;
  const apiKey = process.env.NOTEGATE_WEB_E2E_API_KEY;
  if (!session || !apiKey) throw new Error("Missing disposable Swagger UI test identity.");
  const origin = "http://127.0.0.1:9191";
  let externalRequests = 0;
  await page.context().addCookies([{
    name: "notegate_browser_session", value: session, url: origin, httpOnly: true, sameSite: "Lax"
  }]);
  await page.route("**/*", async (route) => {
    const url = new URL(route.request().url());
    if (url.origin === origin || ["data:", "blob:"].includes(url.protocol)) {
      await route.continue();
    } else {
      externalRequests += 1;
      await route.abort("blockedbyclient");
    }
  });

  try {
    // The browser login protects docs but cannot authenticate external resource calls.
    const withoutKey = await page.request.get(`${origin}/api/v2/me`);
    expect(withoutKey.status()).toBe(401);
    await page.goto(`${origin}/swagger-ui/v2`);
    await expect(page).toHaveURL(`${origin}/swagger-ui/v2/`);
    await expect(page.getByRole("heading", { name: "NoteGate REST API", exact: false })).toBeVisible();
    const operation = page.locator("#operations-identity-get_me");
    await expect(operation).toBeVisible();

    await page.locator(".auth-wrapper").getByRole("button", { name: "Authorize", exact: true }).click();
    const authorization = page.locator(".dialog-ux");
    try {
      await authorization.getByRole("textbox", { name: "auth-bearer-value", exact: true }).fill(apiKey);
    } catch {
      throw new Error("Could not enter the disposable Agent API key.");
    }
    await authorization.getByRole("button", { name: "Apply credentials", exact: true }).click();
    await authorization.getByRole("button", { name: "Close", exact: true }).click();
    await operation.locator(".opblock-summary-control").click();
    await operation.getByRole("button", { name: "Try it out", exact: true }).click();
    const responsePromise = page.waitForResponse((response) =>
      new URL(response.url()).pathname === "/api/v2/me" && response.request().method() === "GET");
    await operation.getByRole("button", { name: "Execute", exact: true }).click();
    const response = await responsePromise;
    expect(response.status()).toBe(200);
    const body = await response.json() as { account: { kind: string } };
    expect(body.account.kind).toBe("agent");
    await expect(operation.locator(".live-responses-table")).toContainText("200");
    expect(externalRequests).toBe(0);
  } finally {
    // Swagger's generated curl includes the key; remove it before failure snapshots.
    await page.goto("about:blank");
  }
});
