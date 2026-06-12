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
  const dialogResponses: string[] = [];
  page.on("dialog", async (dialog) => {
    await dialog.accept(dialogResponses.shift() ?? "");
  });

  await page.goto("/");
  await page.getByLabel("User API key").fill(apiKey!);
  await page.getByRole("button", { name: "Open dashboard" }).click();
  await expect(page.getByText("Notegate", { exact: true })).toBeVisible();

  dialogResponses.push(spaceName);
  await page.getByLabel("Add space").click();
  await expect(page.locator("body")).toContainText(spaceName);

  dialogResponses.push(textName);
  await page.getByLabel("Create node").click();
  await page.getByRole("button", { name: "New text" }).first().click();
  await expect(page.locator("body")).toContainText(textName);

  await page.getByRole("button", { name: "Edit", exact: true }).click();
  await page.locator("textarea").fill(`# ${textName}\n\nCreated by web smoke e2e.\n`);
  await page.getByRole("button", { name: "Save" }).click();
  await expect(page.getByText("Created by web smoke e2e.")).toBeVisible();

  dialogResponses.push(JSON.stringify({ source: "web-e2e", suffix }));
  await page.getByRole("button", { name: "Edit metadata" }).click();
  await expect(page.getByText('"source": "web-e2e"')).toBeVisible();

  const dir = join(tmpdir(), "notegate-web-e2e");
  mkdirSync(dir, { recursive: true });
  const uploadPath = join(dir, fileName);
  writeFileSync(uploadPath, "small upload from web smoke e2e\n");

  dialogResponses.push(fileName);
  await page.getByLabel("Create node").click();
  const fileChooserPromise = page.waitForEvent("filechooser");
  await page.getByText("Upload file").click();
  const fileChooser = await fileChooserPromise;
  await fileChooser.setFiles(uploadPath);
  await expect(page.locator("body")).toContainText(fileName);
  await expect(page.getByRole("button", { name: "Download" })).toBeVisible();

  dialogResponses.push("");
  await page.getByLabel("Manage space").click();
  await page.getByRole("button", { name: "Delete space" }).click();
  await expect(page.locator("body")).not.toContainText(spaceName);
});
