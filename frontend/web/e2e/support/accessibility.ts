import AxeBuilder from "@axe-core/playwright";
import { expect, type Page } from "@playwright/test";

const WCAG_TAGS = ["wcag2a", "wcag2aa", "wcag21a", "wcag21aa", "wcag22aa"];

export async function expectNoAccessibilityViolations(page: Page, includeSelector?: string): Promise<void> {
  const scan = new AxeBuilder({ page }).withTags(WCAG_TAGS);
  if (includeSelector) scan.include(includeSelector);
  const result = await scan.analyze();

  expect(
    result.violations,
    JSON.stringify(result.violations, null, 2)
  ).toEqual([]);
}

export function contrastRatio(first: string, second: string): number {
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
