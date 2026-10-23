import { expect, test } from "@playwright/test";

import { contrastRatio } from "./support/accessibility";

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
  test(`keeps the login theme control's keyboard focus visible in ${theme} mode`, async ({ page }) => {
    await page.addInitScript((value) => window.localStorage.setItem("notegate.theme", value), theme);
    await page.goto("/");
    await expect(page.getByRole("heading", { name: "Continue to NoteGate", exact: true })).toBeVisible();

    const homeLink = page.getByRole("link", { name: "NoteGate home", exact: true });
    const themeButton = page.getByRole("button", { name: theme === "light" ? "Use dark theme" : "Use light theme" });
    await expect(homeLink).toHaveAttribute("href", "/");
    await expect(themeButton).toBeVisible();
    await page.keyboard.press("Tab");
    await expect(homeLink).toBeFocused();
    expect(await homeLink.evaluate((link) => link.matches(":focus-visible"))).toBe(true);
    await page.keyboard.press("Tab");
    await expect(themeButton).toBeFocused();

    const style = await themeButton.evaluate((button) => {
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
    console.log(`FOCUS_INDICATOR_METRIC ${JSON.stringify({ theme, outlineContrast })}`);
  });
}
