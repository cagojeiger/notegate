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
  test(`keeps the login theme control's keyboard focus visible in ${theme} mode`, async ({ page }) => {
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
        surfaceColor: getComputedStyle(button.closest("main") ?? document.body).backgroundColor
      };
    });
    expect(style.focusVisible).toBe(true);
    expect(style.outlineStyle).toBe("solid");
    expect(Number.parseFloat(style.outlineWidth)).toBeGreaterThanOrEqual(2);
    const outlineContrast = contrastRatio(style.outlineColor, style.surfaceColor);
    expect(outlineContrast).toBeGreaterThanOrEqual(3);
    console.log(`FOCUS_INDICATOR_METRIC ${JSON.stringify({ theme, outlineContrast })}`);
  });
}

function contrastRatio(first: string, second: string): number {
  const luminance = (color: string) => {
    const channels = color.match(/^rgb\((\d+), (\d+), (\d+)\)$/);
    if (!channels) throw new Error(`Expected a solid RGB color, got ${color}`);
    const [red, green, blue] = channels.slice(1).map((value) => {
      const channel = Number(value) / 255;
      return channel <= 0.04045 ? channel / 12.92 : ((channel + 0.055) / 1.055) ** 2.4;
    });
    return 0.2126 * red + 0.7152 * green + 0.0722 * blue;
  };
  const a = luminance(first);
  const b = luminance(second);
  return (Math.max(a, b) + 0.05) / (Math.min(a, b) + 0.05);
}
