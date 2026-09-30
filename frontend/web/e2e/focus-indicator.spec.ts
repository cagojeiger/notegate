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

for (const theme of ["light", "dark"] as const) {
  test(`records the login theme control's keyboard focus in ${theme} mode`, async ({ page }) => {
    await page.addInitScript((value) => window.localStorage.setItem("notegate.theme", value), theme);
    await page.goto("/");

    const themeButton = page.getByRole("button", { name: theme === "light" ? "Use dark theme" : "Use light theme" });
    await expect(themeButton).toBeVisible();
    await page.keyboard.press("Tab");
    await expect(themeButton).toBeFocused();

    const style = await themeButton.evaluate((button) => {
      const computed = getComputedStyle(button);
      return {
        focusVisible: button.matches(":focus-visible"),
        outlineStyle: computed.outlineStyle,
        outlineWidth: computed.outlineWidth,
        outlineColor: computed.outlineColor,
        boxShadow: computed.boxShadow,
        surfaceColor: getComputedStyle(document.body).backgroundColor
      };
    });
    expect(style.focusVisible).toBe(true);
    console.log(`FOCUS_INDICATOR_METRIC ${JSON.stringify({ theme, ...style })}`);
  });
}
