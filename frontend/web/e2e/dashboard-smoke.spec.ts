import { expect, test } from "@playwright/test";
import { mkdirSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";

const apiKey = process.env.NOTEGATE_WEB_E2E_API_KEY;

test("dev API key dashboard supports space, text, metadata, and file basics", async ({ page }) => {
  test.skip(!apiKey, "set NOTEGATE_WEB_E2E_API_KEY to run dashboard smoke e2e");
  const suffix = Date.now().toString(36);
  const spaceName = `web-e2e-${suffix}`;
  const textName = `note-${suffix}.md`;
  const fileName = `asset-${suffix}.txt`;

  await page.goto("/");
  await page.getByText("Developer API key fallback").click();
  await page.getByLabel("User API key").fill(apiKey!);
  await page.getByRole("button", { name: "Open with API key" }).click();
  await expect(page.getByText("Notegate", { exact: true })).toBeVisible();

  await page.locator('button[aria-label="Add space"]:visible').click();
  await page.getByLabel("Space name").fill(spaceName);
  await page.getByRole("button", { name: "Create", exact: true }).click();
  await expect(page.locator("body")).toContainText(spaceName);

  await page.getByLabel("Create node").click();
  await page.getByRole("button", { name: "New text" }).first().click();
  await page.getByLabel("Name", { exact: true }).fill(textName);
  await page.getByRole("button", { name: "Create", exact: true }).click();
  await expect(page.locator("body")).toContainText(textName);

  await page.getByRole("button", { name: "Edit", exact: true }).click();
  await page.locator("textarea").fill(`# ${textName}\n\nCreated by web smoke e2e.\n`);
  await page.getByRole("button", { name: "Save" }).click();
  await expect(page.getByText("Created by web smoke e2e.")).toBeVisible();

  await page.getByRole("button", { name: "Edit metadata" }).click();
  await page.getByLabel("Metadata JSON").fill(JSON.stringify({ source: "web-e2e", suffix }));
  await page.getByRole("button", { name: "Save", exact: true }).click();
  await expect(page.getByText('"source": "web-e2e"')).toBeVisible();

  const dir = join(tmpdir(), "notegate-web-e2e");
  mkdirSync(dir, { recursive: true });
  const uploadPath = join(dir, fileName);
  writeFileSync(uploadPath, "small upload from web smoke e2e\n");

  await page.getByLabel("Create node").click();
  const fileChooserPromise = page.waitForEvent("filechooser");
  await page.getByText("Upload file").click();
  const fileChooser = await fileChooserPromise;
  await fileChooser.setFiles(uploadPath);
  await page.getByRole("button", { name: "Upload", exact: true }).click();
  const fileNode = page.getByRole("button", { name: fileName, exact: true });
  await expect(fileNode).toBeVisible();
  await fileNode.click();
  await expect(page.getByRole("button", { name: "Download" })).toBeVisible();

  const downloadPromise = page.waitForEvent("download");
  await page.getByRole("button", { name: "Download" }).click();
  const download = await downloadPromise;
  expect(download.suggestedFilename()).toBe(fileName);

  await page.getByLabel("Manage space").click();
  await page.getByRole("button", { name: "Delete space" }).click();
  await page.getByRole("button", { name: "Delete", exact: true }).click();
  await expect(page.locator("body")).not.toContainText(spaceName);
});
