import { createServer } from "node:http";

import { expect, test, type Page } from "@playwright/test";

import { contrastRatio } from "./support/accessibility";

async function captureAuthViews(page: Page, name: string) {
  const links = page.getByRole("navigation", { name: "Legal and support" });
  for (const [label, path] of [["Terms", "/terms"], ["Privacy", "/privacy"], ["Support", "/support"]]) {
    await expect(links.getByRole("link", { name: label, exact: true })).toHaveAttribute("href", path);
  }
  for (const theme of ["light", "dark"] as const) {
    if (await page.locator("html").getAttribute("data-theme") !== theme) {
      await page.getByRole("button", { name: `Use ${theme} theme`, exact: true }).click();
    }
    for (const [device, width, height] of [["desktop", 1440, 900], ["mobile", 390, 844]] as const) {
      await page.setViewportSize({ width, height });
      await expect(page.locator("html")).toHaveAttribute("data-theme", theme);
      await expect(page.getByRole("heading", { level: 1 })).toBeVisible();
      expect(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth)).toBe(true);
      const approveButton = page.locator('button[value="approve"]');
      if (device === "desktop" && (await approveButton.count()) > 0) {
        await page.locator("#auth-theme").hover();
        const foreground = await approveButton.evaluate((element) => getComputedStyle(element).color);
        await approveButton.hover();
        await expect.poll(() => approveButton.evaluate((element) => getComputedStyle(element).color)).toBe(foreground);
        await page.screenshot({ path: `test-results/browser-functional/${name}-${theme}-approve-hover.png`, fullPage: true });
        await page.locator("#auth-theme").hover();
        await approveButton.focus();
        await page.keyboard.press("Shift+Tab");
        await page.keyboard.press("Tab");
        await expect(approveButton).toBeFocused();
        const style = await approveButton.evaluate((button) => {
          const computed = getComputedStyle(button);
          return {
            focusVisible: button.matches(":focus-visible"),
            outlineStyle: computed.outlineStyle,
            outlineWidth: computed.outlineWidth,
            outlineColor: computed.outlineColor,
            backgroundColor: computed.backgroundColor
          };
        });
        expect(style.focusVisible).toBe(true);
        expect(style.outlineStyle).toBe("solid");
        expect(Number.parseFloat(style.outlineWidth)).toBeGreaterThanOrEqual(2);
        const outlineContrast = contrastRatio(style.outlineColor, style.backgroundColor);
        expect(outlineContrast).toBeGreaterThanOrEqual(3);
        console.log(`FOCUS_INDICATOR_METRIC ${JSON.stringify({ name, theme, outlineContrast })}`);
        await approveButton.evaluate((button) => button.blur());
      }
      if (device === "mobile") {
        const heights = await page.locator("button:visible, input:visible, summary:visible").evaluateAll(
          (elements) => elements.map((element) => element.getBoundingClientRect().height)
        );
        expect(heights.every((value) => value >= 44)).toBe(true);
      }
      await page.screenshot({ path: `test-results/browser-functional/${name}-${theme}-${device}.png`, fullPage: true });
    }
  }
  await page.setViewportSize({ width: 320, height: 844 });
  for (const summary of await page.locator("summary").all()) await summary.click();
  expect(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth)).toBe(true);
}

test("standalone CLI approval and connection revocation", async ({ browser, playwright, baseURL }) => {
  const session = process.env.NOTEGATE_WEB_E2E_OAUTH_BROWSER_SESSION;
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
    const approvalPage = await page.goto(device.verification_uri_complete);
    expect(approvalPage?.headers()["content-security-policy"]).toBe("default-src 'none'; style-src 'self'; script-src 'self'; form-action 'self'; frame-ancestors 'none'; base-uri 'none'");
    await expect(page.getByRole("heading", { name: "Connect NoteGate CLI" })).toBeVisible();
    await expect(page.getByRole("heading", { level: 2, name: "Check the code", exact: true })).toBeVisible();
    await captureAuthViews(page, "oauth-device");
    const approvalRequest = page.waitForRequest((request) => request.method() === "POST" && new URL(request.url()).pathname === "/oauth/device");
    await page.getByRole("button", { name: "Connect CLI", exact: true }).click();
    expect((await (await approvalRequest).allHeaders()).origin).toBe(new URL(baseURL!).origin);
    await expect(page.getByRole("heading", { name: "Request completed" })).toBeVisible();
    await expect(page.locator(".auth-note--success")).toBeVisible();
    await captureAuthViews(page, "oauth-device-complete");
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
    await expect(page.getByRole("heading", { level: 2, name: "Revoking access", exact: true })).toBeVisible();
    await expect(page.getByText("NoteGate CLI", { exact: true })).toBeVisible();
    await captureAuthViews(page, "oauth-connections");
    await page.screenshot({ path: "test-results/browser-functional/oauth-connections.png" });
    await page.getByRole("button", { name: "Revoke connection" }).click();
    await expect(page.getByRole("button", { name: "Revoke connection" })).toHaveCount(0);
    await expect(page.getByText(/No connected applications/)).toBeVisible();
    const revoked = await request.post("/oauth/token", {
      form: { grant_type: "refresh_token", client_id: metadata.cli_client_id, refresh_token: tokens.refresh_token }
    });
    expect(revoked.status()).toBe(400);
    expect((await revoked.json()).error).toBe("invalid_grant");
    await page.goto(device.verification_uri_complete);
    await page.getByRole("button", { name: "Connect CLI", exact: true }).click();
    await expect(page.getByRole("heading", { name: "Code unavailable", exact: true })).toBeVisible();
    await expect(page.locator(".auth-note--danger")).toContainText("instead of resending this page");
    await captureAuthViews(page, "oauth-device-code-unavailable");
  } finally {
    await context.close();
    await request.dispose();
  }
});

test("standalone MCP consent follows its registered callback and exchanges the PKCE code", async ({ browser, playwright, baseURL }) => {
  const session = process.env.NOTEGATE_WEB_E2E_OAUTH_BROWSER_SESSION;
  const rawFixture = process.env.NOTEGATE_WEB_E2E_OAUTH_CONSENT;
  expect(Boolean(session && rawFixture)).toBe(true);
  const fixture = JSON.parse(rawFixture!) as {
    id: string; secret: string; verifier: string; client_id: string; redirect_uri: string; state: string;
  };
  const callback = new URL(fixture.redirect_uri);
  let returned: { url: URL; referer: string | undefined } | undefined;
  const server = createServer((incoming, response) => {
    const url = new URL(incoming.url ?? "/", callback.origin);
    if (url.pathname === callback.pathname) {
      returned = { url, referer: incoming.headers.referer };
      response.writeHead(303, { location: "/done" });
      response.end();
    } else {
      response.writeHead(200, { "content-type": "text/html" });
      response.end("OAuth callback received");
    }
  });
  await new Promise<void>((resolve) => server.listen(Number(callback.port), "127.0.0.1", resolve));
  const request = await playwright.request.newContext({ baseURL });
  const context = await browser.newContext({ baseURL });
  try {
    await context.addCookies([
      { name: "notegate_browser_session", value: session!, url: baseURL!, httpOnly: true, sameSite: "Lax" },
      { name: `notegate_authorization_${fixture.id}`, value: fixture.secret, url: baseURL!, httpOnly: true, sameSite: "Lax" }
    ]);
    const page = await context.newPage();
    await page.goto(`/oauth/approve?id=${fixture.id}`);
    await expect(page.getByRole("heading", { name: "Connect application" })).toBeVisible();
    await expect(page.getByRole("heading", { level: 2, name: "Before you allow access", exact: true })).toBeVisible();
    await captureAuthViews(page, "oauth-consent");
    await page.getByRole("button", { name: "Allow access", exact: true }).click();
    await expect(page).toHaveURL(`${callback.origin}/done`);
    expect(returned).toBeDefined();
    const parameters = returned!.url.searchParams;
    expect(parameters.get("state")).toBe(fixture.state);
    expect(parameters.get("iss")).toBe(baseURL);
    expect(returned!.referer).toBeUndefined();
    const response = await request.post("/oauth/token", {
      form: {
        grant_type: "authorization_code", client_id: fixture.client_id, resource: `${baseURL}/mcp`,
        redirect_uri: fixture.redirect_uri, code: parameters.get("code")!, code_verifier: fixture.verifier
      }
    });
    expect(response.status()).toBe(200);
    const tokens = await response.json();
    expect(tokens.scope).toBe("mcp offline_access");
    expect((await request.post("/oauth/revoke", {
      form: { client_id: fixture.client_id, token: tokens.refresh_token }
    })).status()).toBe(200);
    const reused = await page.goto(`/oauth/approve?id=${fixture.id}`);
    expect(reused?.status()).toBe(400);
    await expect(page.getByRole("heading", { name: "Request unavailable", exact: true })).toBeVisible();
    await expect(page.locator(".auth-note--danger")).toContainText("Do not resend this page.");
    await captureAuthViews(page, "oauth-consent-unavailable");
  } finally {
    await context.close();
    await request.dispose();
    await new Promise<void>((resolve) => server.close(() => resolve()));
  }
});

test("CLI consent works without JavaScript and does not depend on browser storage", async ({ browser, playwright, baseURL }) => {
  const session = process.env.NOTEGATE_WEB_E2E_OAUTH_BROWSER_SESSION;
  expect(Boolean(session)).toBe(true);
  const request = await playwright.request.newContext({ baseURL });
  const context = await browser.newContext({ baseURL, javaScriptEnabled: false });
  try {
    await context.addCookies([{ name: "notegate_browser_session", value: session!, url: baseURL!, httpOnly: true, sameSite: "Lax" }]);
    const metadata = await (await request.get("/.well-known/oauth-authorization-server")).json();
    const device = await (await request.post("/oauth/device/authorize", {
      form: { client_id: metadata.cli_client_id, scope: "cli offline_access" }
    })).json();
    const page = await context.newPage();
    await page.goto(device.verification_uri_complete);
    await expect(page.getByLabel("Code shown in your terminal")).toHaveValue(device.user_code);
    await page.getByLabel("Code shown in your terminal").press("Enter");
    await expect(page.getByRole("heading", { name: "Request completed" })).toBeVisible();
    const response = await request.post("/oauth/token", {
      form: { grant_type: "urn:ietf:params:oauth:grant-type:device_code", client_id: metadata.cli_client_id, device_code: device.device_code }
    });
    expect(response.status()).toBe(200);
    const tokens = await response.json();
    expect((await request.post("/oauth/revoke", {
      form: { client_id: metadata.cli_client_id, token: tokens.refresh_token }
    })).status()).toBe(200);
  } finally {
    await context.close();
    await request.dispose();
  }
});

test("authentication theme remains usable when browser storage is unavailable", async ({ browser, baseURL }) => {
  const context = await browser.newContext({ baseURL, colorScheme: "dark" });
  try {
    await context.addInitScript(() => {
      Object.defineProperty(window, "localStorage", { get() { throw new DOMException("Unavailable", "SecurityError"); } });
    });
    const page = await context.newPage();
    await page.goto("/auth/success");
    await expect(page.locator("html")).toHaveAttribute("data-theme", "dark");
    await page.getByRole("button", { name: "Use light theme", exact: true }).click();
    await expect(page.locator("html")).toHaveAttribute("data-theme", "light");
  } finally {
    await context.close();
  }
});

test("browser completion and failed form submissions keep the authentication layout", async ({ browser, baseURL }) => {
  const context = await browser.newContext({ baseURL });
  try {
    const page = await context.newPage();
    await page.goto("/auth/success");
    await expect(page.getByRole("heading", { name: "Login complete", exact: true })).toBeVisible();
    await expect(page.locator(".auth-note--success")).toBeVisible();
    await captureAuthViews(page, "oauth-login-complete");

    const response = page.waitForResponse((value) => new URL(value.url()).pathname === "/oauth/device" && value.request().method() === "POST");
    await page.evaluate(() => {
      const form = document.createElement("form");
      form.method = "post";
      form.action = "/oauth/device";
      for (const [name, value] of [["user_code", "missing-session-fixture"], ["decision", "approve"]]) {
        const input = document.createElement("input");
        input.type = "hidden";
        input.name = name;
        input.value = value;
        form.append(input);
      }
      document.body.append(form);
      form.requestSubmit();
    });
    expect((await response).status()).toBe(400);
    await expect(page.getByRole("heading", { name: "Sign-in required", exact: true })).toBeVisible();
    await expect(page.locator(".auth-note--danger")).toContainText("Return to NoteGate to sign in");
    await expect(page.locator("form")).toHaveCount(0);
    await captureAuthViews(page, "oauth-session-required");

    const invalid = await page.goto("/auth/callback");
    expect(invalid?.status()).toBe(400);
    await expect(page.getByRole("heading", { name: "Login failed", exact: true })).toBeVisible();
    await expect(page.locator(".auth-note--danger")).toContainText("start a new sign-in");
    await captureAuthViews(page, "oauth-login-request-unavailable");
  } finally {
    await context.close();
  }
});
