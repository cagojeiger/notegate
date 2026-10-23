import { expect, test } from "@playwright/test";

import { expectNoAccessibilityViolations } from "./support/accessibility";

test.beforeEach(async ({ page }) => {
  await page.route("**/api/v1/me", async (route) => {
    await route.fulfill({
      status: 401,
      contentType: "application/json",
      body: JSON.stringify({ error: "unauthorized", kind: "unauthorized", message: "unauthorized" })
    });
  });
});

for (const viewport of [
  { name: "desktop", width: 1440, height: 900 },
  { name: "tablet", width: 900, height: 1024 },
  { name: "mobile", width: 390, height: 844 }
]) {
  test(`login remains usable in light and dark mode on ${viewport.name}`, async ({ page }, testInfo) => {
    await page.setViewportSize({ width: viewport.width, height: viewport.height });
    await page.goto("/");

    const authScreen = page.locator("main.ng-auth-screen");
    const googleButton = page.getByRole("button", { name: "Continue with Google" });
    const googleMark = googleButton.locator('img[src="/google-g.png"]');
    await expect(page.getByText("Continue to NoteGate")).toBeVisible();
    await expect(googleButton).toBeVisible();
    await expect.poll(() => googleMark.evaluate((element: HTMLImageElement) => element.complete && element.naturalWidth > 0)).toBe(true);
    const authFontFamily = await authScreen.evaluate((element) => getComputedStyle(element).fontFamily);
    const googleButtonFontFamily = await googleButton.evaluate((element) => getComputedStyle(element).fontFamily);
    expect(authFontFamily).toContain("-apple-system");
    expect(authFontFamily).not.toContain("LINE Seed Sans KR");
    expect(googleButtonFontFamily).toBe(authFontFamily);
    await expect.poll(() => authScreen.evaluate((element) => getComputedStyle(element).overflowX)).toBe("hidden");
    await expect.poll(() => page.evaluate(() => document.documentElement.scrollWidth <= document.documentElement.clientWidth)).toBe(true);
    await expectNoAccessibilityViolations(page);
    await testInfo.attach(`login-${viewport.name}-light`, {
      body: await page.screenshot({ path: `test-results/login-${viewport.name}-light.png` }),
      contentType: "image/png"
    });

    await page.getByRole("button", { name: "Use dark theme" }).click();
    await expect(page.locator("html")).toHaveAttribute("data-theme", "dark");
    await expect(googleButton).toBeVisible();
    await expect.poll(() => googleButton.evaluate((element) => getComputedStyle(element).backgroundColor)).toBe("rgb(40, 38, 43)");
    await expect.poll(() => page.evaluate(() => document.documentElement.scrollWidth <= document.documentElement.clientWidth)).toBe(true);
    await expectNoAccessibilityViolations(page);
    await testInfo.attach(`login-${viewport.name}-dark`, {
      body: await page.screenshot({ path: `test-results/login-${viewport.name}-dark.png` }),
      contentType: "image/png"
    });
  });
}

test("login remains scrollable on a short desktop viewport", async ({ page }) => {
  await page.setViewportSize({ width: 1024, height: 320 });
  await page.goto("/");

  const authScreen = page.locator("main.ng-auth-screen");
  await expect(page.getByText("Continue to NoteGate")).toBeVisible();
  await expect.poll(() => authScreen.evaluate((element) => getComputedStyle(element).overflowY)).toBe("auto");
  await expect.poll(() => authScreen.evaluate((element) => element.scrollHeight > element.clientHeight)).toBe(true);

  const securityMessage = page.getByText("Your Google password stays with Google.", { exact: true });
  await securityMessage.scrollIntoViewIfNeeded();
  await expect(securityMessage).toBeVisible();
});

test("login does not introduce horizontal scrolling at 320 CSS pixels", async ({ page }) => {
  await page.setViewportSize({ width: 320, height: 640 });
  await page.goto("/");

  const authScreen = page.locator("main.ng-auth-screen");
  await expect(page.getByText("Continue to NoteGate")).toBeVisible();
  await expect.poll(() => authScreen.evaluate((element) => element.scrollWidth <= element.clientWidth)).toBe(true);
});

for (const theme of ["light", "dark"] as const) {
  test(`public information stays accessible without a session in ${theme} mode`, async ({ page }, testInfo) => {
    await page.addInitScript((value) => window.localStorage.setItem("notegate.theme", value), theme);
    await page.setViewportSize({ width: 320, height: 640 });
    let sessionRequests = 0;
    await page.route("**/api/v1/me", async (route) => {
      sessionRequests += 1;
      await route.fulfill({
        status: 503,
        contentType: "application/json",
        body: JSON.stringify({ error: "auth_unavailable", kind: "auth_unavailable", message: "unavailable" })
      });
    });

    for (const [path, title] of [
      ["/terms", "Terms of Service"],
      ["/privacy", "Privacy Policy"],
      ["/support", "Help and contact"]
    ]) {
      await page.goto(path);
      await expect(page.getByRole("heading", { level: 1, name: title, exact: true })).toBeVisible();
      await expect(page.getByText("In preparation", { exact: true })).toBeVisible();
      await expect(page.locator("html")).toHaveAttribute("data-theme", theme);
      await expect(page.getByRole("checkbox")).toHaveCount(0);
      const backLink = page.getByRole("link", { name: "Back to NoteGate", exact: true });
      await expect(backLink).toHaveAttribute("href", "/");
      const target = await backLink.boundingBox();
      expect(target).not.toBeNull();
      expect(target!.width).toBeGreaterThanOrEqual(44);
      expect(target!.height).toBeGreaterThanOrEqual(44);
      await expect.poll(() => page.evaluate(() => document.documentElement.scrollWidth <= document.documentElement.clientWidth)).toBe(true);
      await expectNoAccessibilityViolations(page);
      expect(sessionRequests).toBe(0);

      if (path === "/terms") {
        await testInfo.attach(`public-terms-mobile-${theme}`, {
          body: await page.screenshot(),
          contentType: "image/png"
        });
      }
    }
  });
}

test("login footer provides touch-sized native links without collecting agreement", async ({ page }) => {
  await page.setViewportSize({ width: 320, height: 640 });
  await page.goto("/");
  const footer = page.getByRole("navigation", { name: "Legal and support" });
  await expect(footer).toBeVisible();
  await expect(page.getByRole("checkbox")).toHaveCount(0);

  for (const [name, path] of [["Terms", "/terms"], ["Privacy", "/privacy"], ["Support", "/support"]]) {
    const link = footer.getByRole("link", { name, exact: true });
    await expect(link).toHaveAttribute("href", path);
    const target = await link.boundingBox();
    expect(target).not.toBeNull();
    expect(target!.width).toBeGreaterThanOrEqual(44);
    expect(target!.height).toBeGreaterThanOrEqual(44);
  }

  const terms = footer.getByRole("link", { name: "Terms", exact: true });
  await terms.focus();
  await page.keyboard.press("Tab");
  await expect(footer.getByRole("link", { name: "Privacy", exact: true })).toBeFocused();
  await page.keyboard.press("Shift+Tab");
  await expect(terms).toBeFocused();
  expect(await terms.evaluate((link) => link.matches(":focus-visible"))).toBe(true);
  await page.keyboard.press("Enter");
  await expect(page).toHaveURL(/\/terms$/);
  await expect(page.getByRole("heading", { level: 1, name: "Terms of Service", exact: true })).toBeVisible();
});

for (const theme of ["light", "dark"] as const) {
  test(`account setup preview works by keyboard without saving or authenticating in ${theme} mode`, async ({ page }, testInfo) => {
    await page.addInitScript((value) => window.localStorage.setItem("notegate.theme", value), theme);
    const authRequests: string[] = [];
    await page.route((url) => /^\/(api|auth|oauth)(\/|$)/.test(url.pathname), async (route) => {
      authRequests.push(new URL(route.request().url()).pathname);
      await route.fulfill({ status: 503, contentType: "application/json", body: "{}" });
    });
    await page.setViewportSize({ width: 1440, height: 900 });
    await page.goto("/signup/preview");
    await expect(page.getByRole("heading", { level: 1, name: "Before you start", exact: true })).toBeVisible();
    await expect(page.getByText("Preview only. Nothing is saved and no account is created.", { exact: true })).toBeVisible();
    const checkbox = page.getByRole("checkbox", { name: "I agree to the Terms of Service.", exact: true });
    const continueButton = page.getByRole("button", { name: "Agree and continue", exact: true });
    await expect(checkbox).not.toBeChecked();
    await expect(continueButton).toBeDisabled();
    await expect(page.getByRole("link", { name: "Back to sign in", exact: true })).toHaveAttribute("href", "/");
    for (const [name, path] of [["Terms of Service", "/terms"], ["Privacy Policy", "/privacy"]]) {
      const link = page.getByRole("link", { name, exact: true });
      await expect(link).toHaveAttribute("href", path);
      await expect(link).toHaveAttribute("target", "_blank");
      await expect(link).toHaveAttribute("rel", /\bnoopener\b/);
    }
    const storageBefore = await page.evaluate(() => ({ local: { ...localStorage }, session: { ...sessionStorage } }));
    await expect(page.locator("html")).toHaveAttribute("data-theme", theme);
    await expectNoAccessibilityViolations(page);
    await testInfo.attach(`signup-preview-desktop-${theme}`, {
      body: await page.screenshot({ path: `test-results/login-signup-preview-desktop-${theme}.png` }),
      contentType: "image/png"
    });

    await checkbox.focus();
    await page.keyboard.press("Space");
    await expect(checkbox).toBeChecked();
    await expect(continueButton).toBeEnabled();
    await page.keyboard.press("Tab");
    await expect(continueButton).toBeFocused();
    await page.keyboard.press("Enter");
    await expect(page.getByRole("status")).toHaveText("Preview complete. No agreement or account was saved.");
    await expectNoAccessibilityViolations(page);
    await testInfo.attach(`signup-preview-complete-desktop-${theme}`, {
      body: await page.screenshot({ path: `test-results/login-signup-preview-complete-desktop-${theme}.png` }),
      contentType: "image/png"
    });
    await page.setViewportSize({ width: 320, height: 1100 });
    expect(await checkbox.evaluate((input) => input.closest("label")?.getBoundingClientRect().height ?? 0)).toBeGreaterThanOrEqual(44);
    const buttonTarget = await continueButton.boundingBox();
    expect(buttonTarget).not.toBeNull();
    expect(buttonTarget!.height).toBeGreaterThanOrEqual(44);
    await expect.poll(() => page.evaluate(() => document.documentElement.scrollWidth <= document.documentElement.clientWidth)).toBe(true);
    await expectNoAccessibilityViolations(page);
    expect(await page.evaluate(() => ({ local: { ...localStorage }, session: { ...sessionStorage } }))).toEqual(storageBefore);
    expect(authRequests).toEqual([]);
    await testInfo.attach(`signup-preview-complete-mobile-${theme}`, {
      body: await page.screenshot({ path: `test-results/login-signup-preview-complete-mobile-${theme}.png` }),
      contentType: "image/png"
    });

    await page.reload();
    await expect(checkbox).not.toBeChecked();
    await expect(continueButton).toBeDisabled();
    await expect(page.getByRole("status")).toHaveCount(0);
    expect(await page.evaluate(() => ({ local: { ...localStorage }, session: { ...sessionStorage } }))).toEqual(storageBefore);
    expect(authRequests).toEqual([]);
    await expect(page.locator("html")).toHaveAttribute("data-theme", theme);
    await expectNoAccessibilityViolations(page);
    await testInfo.attach(`signup-preview-mobile-${theme}`, {
      body: await page.screenshot({ path: `test-results/login-signup-preview-mobile-${theme}.png` }),
      contentType: "image/png"
    });
  });
}

for (const theme of ["light", "dark"] as const) {
  for (const viewport of [
    { name: "desktop", width: 1440, height: 900 },
    { name: "mobile", width: 320, height: 900 }
  ]) {
    test(`authentication states share the page layout on ${viewport.name} in ${theme} mode`, async ({ page }, testInfo) => {
      await page.addInitScript((value) => window.localStorage.setItem("notegate.theme", value), theme);
      await page.setViewportSize(viewport);
      let releaseInitial: () => void = () => undefined;
      let releaseRetry: () => void = () => undefined;
      const initial = new Promise<void>((resolve) => { releaseInitial = resolve; });
      const retry = new Promise<void>((resolve) => { releaseRetry = resolve; });
      let sessionChecks = 0;
      await page.route("**/api/v1/me", async (route) => {
        sessionChecks += 1;
        if (sessionChecks === 1) {
          await initial;
          await route.fulfill({ status: 503, contentType: "application/json", body: JSON.stringify({ error: "auth_unavailable", kind: "auth_unavailable", message: "unavailable" }) });
        } else {
          await retry;
          await route.fulfill({ status: 401, contentType: "application/json", body: JSON.stringify({ error: "unauthorized", kind: "unauthorized", message: "unauthorized" }) });
        }
      });
      async function capture(state: string) {
        await expect(page.locator("main.ng-auth-screen")).toBeVisible();
        await expect(page.locator("html")).toHaveAttribute("data-theme", theme);
        await expect.poll(() => page.evaluate(() => document.documentElement.scrollWidth <= document.documentElement.clientWidth)).toBe(true);
        await expectNoAccessibilityViolations(page);
        await testInfo.attach(`auth-${state}-${viewport.name}-${theme}`, {
          body: await page.screenshot({ path: `test-results/login-${state}-${viewport.name}-${theme}.png` }),
          contentType: "image/png"
        });
      }

      await page.goto("/");
      await expect(page.getByRole("heading", { name: "Checking session", exact: true })).toBeVisible();
      await expect(page.getByRole("status")).toHaveAttribute("aria-busy", "true");
      await capture("checking");
      releaseInitial();
      await expect(page.getByRole("heading", { name: "Authentication temporarily unavailable", exact: true })).toBeVisible();
      await capture("unavailable");
      const retryButton = page.getByRole("button", { name: /^Retry/ });
      await retryButton.click();
      await expect(retryButton).toBeDisabled();
      await expect(retryButton).toHaveAttribute("aria-busy", "true");
      await expect(retryButton).toHaveText("Retrying…");
      await expect(retryButton).toHaveAccessibleName("Retrying…");
      await capture("retrying");
      releaseRetry();
      const googleButton = page.getByRole("button", { name: "Continue with Google", exact: true });
      await expect(googleButton).toBeVisible();

      await page.evaluate(() => { window.open = () => null; });
      await googleButton.click();
      await expect(page.getByRole("status")).toContainText("Your browser blocked the sign-in window.");
      await capture("popup-blocked");
      await page.evaluate(() => { window.open = () => ({ closed: false, focus() {} }) as Window; });
      await googleButton.click();
      await expect(page.getByRole("status")).toHaveText("Complete sign-in in the new window. This page will continue automatically.");
      await capture("popup-waiting");
    });
  }
}

test("the fallback sign-in link provides a touch target and keyboard navigation", async ({ page }) => {
  await page.setViewportSize({ width: 320, height: 900 });
  await page.context().route("**/auth/login", async (route) => {
    await route.fulfill({ status: 200, contentType: "text/html", body: "<!doctype html><title>Sign-in fixture</title>" });
  });
  await page.goto("/");
  const link = page.getByRole("link", { name: "Open Google sign-in in a new window", exact: true });
  await expect(link).toHaveAttribute("href", "/auth/login");
  await expect(link).toHaveAttribute("target", "notegate-login");
  const target = await link.boundingBox();
  expect(target).not.toBeNull();
  expect(target!.width).toBeGreaterThanOrEqual(44);
  expect(target!.height).toBeGreaterThanOrEqual(44);
  await link.focus();
  await page.keyboard.press("Tab");
  await page.keyboard.press("Shift+Tab");
  await expect(link).toBeFocused();
  expect(await link.evaluate((element) => element.matches(":focus-visible"))).toBe(true);
  const popupOpened = page.waitForEvent("popup");
  await page.keyboard.press("Enter");
  const popup = await popupOpened;
  await expect(popup).toHaveURL(/\/auth\/login$/);
  await popup.close();
});
