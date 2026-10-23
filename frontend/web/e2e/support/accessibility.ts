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
