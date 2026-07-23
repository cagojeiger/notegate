import { expect, test } from "@playwright/test";

test.beforeEach(async ({ page }) => {
  await page.route("**/api/v1/me", async (route) => {
    await route.fulfill({
      status: 401,
      contentType: "application/json",
      body: JSON.stringify({ error: "unauthorized", kind: "unauthorized", message: "unauthorized" })
    });
  });
});

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
