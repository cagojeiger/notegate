import { expect, type Page } from "@playwright/test";

export async function openSettings(page: Page, tab = "General") {
  await page.getByRole("button", { name: "Settings", exact: true }).click();
  await page.getByRole("tab", { name: tab, exact: true }).click();
  const view = page.locator("[data-settings-view]");
  await expect(view).toBeVisible();
  return view;
}

export async function closeSettingsInspector(page: Page) {
  const sheet = page.getByRole("dialog", { name: / Inspector$/ });
  if (await sheet.isVisible()) await sheet.getByRole("button", { name: "Close", exact: true }).click();
}

export async function showSettingsInspector(page: Page) {
  const toggle = page.getByRole("button", { name: "Toggle settings inspector" });
  if (await toggle.getAttribute("aria-pressed") !== "true") await toggle.click();
}

export async function chooseHistorySection(page: Page, name: string) {
  await closeSettingsInspector(page);
  await page.getByRole("combobox", { name: "History type", exact: true }).selectOption({ label: name });
}

export async function selectTrashItem(page: Page, name: string) {
  await closeSettingsInspector(page);
  await page.getByRole("list", { name: "Deleted items", exact: true }).getByRole("button").filter({ has: page.getByText(name, { exact: true }) }).click();
}
