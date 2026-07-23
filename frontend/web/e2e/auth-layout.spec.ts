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
  test(`login remains usable in light and dark mode on ${viewport.name}`, async ({ page }) => {
    await page.setViewportSize({ width: viewport.width, height: viewport.height });
    await page.goto("/");

    const authScreen = page.locator("main.ng-auth-screen");
    const googleButton = page.getByRole("button", { name: "Continue with Google" });
    const googleMark = googleButton.locator('img[src="/google-g.png"]');
    await expect(page.getByText("Continue to NoteGate")).toBeVisible();
    await expect(googleButton).toBeVisible();
    await expect.poll(() => googleMark.evaluate((element: HTMLImageElement) => element.complete && element.naturalWidth > 0)).toBe(true);
    await expect.poll(() => authScreen.evaluate((element) => getComputedStyle(element).overflowX)).toBe("hidden");
    await expect.poll(() => page.evaluate(() => document.documentElement.scrollWidth <= document.documentElement.clientWidth)).toBe(true);
    await expectNoAccessibilityViolations(page);

    await page.getByRole("button", { name: "Use dark theme" }).click();
    await expect(page.locator("html")).toHaveAttribute("data-theme", "dark");
    await expect(googleButton).toBeVisible();
    await expect.poll(() => googleButton.evaluate((element) => getComputedStyle(element).backgroundColor)).toBe("rgb(19, 19, 20)");
    await expect.poll(() => page.evaluate(() => document.documentElement.scrollWidth <= document.documentElement.clientWidth)).toBe(true);
    await expectNoAccessibilityViolations(page);
  });
}

test("login remains scrollable on a short desktop viewport", async ({ page }) => {
  await page.setViewportSize({ width: 1024, height: 320 });
  await page.goto("/");

  const authScreen = page.locator("main.ng-auth-screen");
  await expect(page.getByText("Continue to NoteGate")).toBeVisible();
  await expect.poll(() => authScreen.evaluate((element) => getComputedStyle(element).overflowY)).toBe("auto");
  await expect.poll(() => authScreen.evaluate((element) => element.scrollHeight > element.clientHeight)).toBe(true);

  const securityMessage = page.getByText("Google SSO is NoteGate's only production sign-in method.", { exact: false });
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
