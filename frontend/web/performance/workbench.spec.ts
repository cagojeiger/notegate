import { expect, test } from "@playwright/test";

import type { RestNode, Space, ReadTextResponse } from "../src/api/types";
import { instrument, writeReport, type Measurement } from "./measure";

const fixture = { rootDocuments: 120, nestedDepth: 3, trashItems: 55, apiCalls: 55, trials: 3 };
const content = `# Baseline document\n\n${Array.from({ length: 800 }, (_, index) =>
  `Paragraph ${index}: synthetic Markdown for browser rendering, with **emphasis** and a stable measurement payload.\n`
).join("\n")}`;

test("authenticated workbench performance baseline", async ({ browser, playwright, baseURL }) => {
  // A missing identity is a configuration failure, never a silently skipped benchmark.
  const session = process.env.NOTEGATE_WEB_E2E_BROWSER_SESSION;
  const apiKey = process.env.NOTEGATE_PUBLIC_V2_API_KEY;
  expect(session).toBeTruthy();
  expect(apiKey).toBeTruthy();
  expect(new URL(baseURL!).hostname).toMatch(/^(localhost|127\.0\.0\.1)$/);
  const api = await playwright.request.newContext({ baseURL, extraHTTPHeaders: {
    Cookie: `notegate_browser_session=${session}`
  } });
  async function request<T>(path: string, method = "GET", data?: unknown): Promise<T> {
    const response = await api.fetch(path, { method, data });
    expect(response.ok(), `${method} fixture request returned ${response.status()}`).toBe(true);
    return response.status() === 204 ? undefined as T : await response.json() as T;
  }
  const measurements: Measurement[] = [];
  let completed = false;
  try {
    const space = await request<Space>("/api/v1/spaces", "POST", { name: "Browser baseline" });
    const path = `/api/v1/spaces/${space.id}`;
    const create = (parent_id: string, name: string, kind: "text" | "folder", body?: string) =>
      request<RestNode>(`${path}/nodes`, "POST", { parent_id, name, kind, content: body });
    for (let index = 0; index < fixture.rootDocuments; index++) {
      await create(space.root_node_id, `${String(index).padStart(3, "0")}-entry.md`, "text", `# Entry ${index}\n\nFixture document.\n`);
    }
    let parent = space.root_node_id;
    for (let depth = 0; depth < fixture.nestedDepth; depth++) {
      parent = (await create(parent, `folder-${depth}`, "folder")).id;
    }
    const document = await create(parent, "baseline.md", "text", content);
    for (let index = 0; index < fixture.trashItems; index++) {
      const item = await create(space.root_node_id, `deleted-${index}.md`, "text", "Trash fixture.\n");
      await request(`${path}/nodes/${item.id}?recursive=false`, "DELETE");
    }
    // Real external calls produce API history; browser v1 calls remain unrecorded.
    for (let index = 0; index < fixture.apiCalls; index++) {
      const response = await api.get("/api/v2/me", { headers: { Authorization: `Bearer ${apiKey}` } });
      expect(response.status()).toBe(200);
    }
    for (let trial = 1; trial <= fixture.trials; trial++) {
      const initial = `${content}\nTrial ${trial} initial state.\n`;
      await request(`${path}/text/${document.id}`, "PUT", { storage_format: "plain", content: initial });
      const context = await browser.newContext({ baseURL, viewport: { width: 1440, height: 900 } });
      await context.addCookies([{ name: "notegate_browser_session", value: session!, url: baseURL!, httpOnly: true, sameSite: "Lax" }]);
      const page = await context.newPage();
      const { measure, failures } = await instrument(page, trial, measurements);
      try {
        const preview = page.getByRole("region", { name: "Document preview", exact: true });
        const tree = page.getByRole("tree", { name: "Files" });
        const recent = page.locator("[data-recent-list]");
        await measure("workspace-cold-context", async () => {
          await page.goto("/", { waitUntil: "domcontentloaded" });
          await expect(tree.getByRole("button", { name: "000-entry.md", exact: true })).toBeVisible();
          await expect(recent.getByRole("button", { name: /^baseline\.md/ })).toBeVisible();
        }, true);
        await measure("files-document-open", async () => {
          await tree.getByRole("button", { name: "000-entry.md", exact: true }).click();
          await expect(preview.getByRole("heading", { name: "Entry 0", exact: true })).toBeVisible();
        });
        await measure("recent-nested-document-open", async () => {
          await recent.getByRole("button", { name: /^baseline\.md/ }).click();
          await expect(preview.getByRole("heading", { name: "Baseline document", exact: true })).toBeVisible();
          await expect(tree.getByRole("treeitem", { selected: true })).toContainText("baseline.md");
          await expect(tree.getByRole("button", { name: "baseline.md", exact: true })).toBeInViewport();
        });
        await measure("editor-open", async () => {
          await page.getByRole("button", { name: "Edit", exact: true }).click();
          await expect(page.getByRole("textbox", { name: "Edit text content" })).toHaveValue(initial);
        });
        const updated = initial.replace("Paragraph 400:", "Changed paragraph 400:");
        await page.getByRole("textbox", { name: "Edit text content" }).fill(updated);
        await measure("save-to-preview", async () => {
          await page.getByRole("button", { name: "Save", exact: true }).click();
          await expect(page.getByRole("textbox", { name: "Edit text content" })).toHaveCount(0);
          await expect(preview).toContainText("Changed paragraph 400:");
        });
        const saved = await request<ReadTextResponse>(`${path}/text/${document.id}?max_lines=5000&max_bytes=1048576`);
        expect(saved.text.storage_format).toBe("plain");
        expect("content" in saved.text && saved.text.content).toBe(updated);
        await measure("version-comparison", async () => {
          await page.getByRole("button", { name: "Version history", exact: true }).click();
          const dialog = page.getByRole("dialog");
          await expect(dialog.getByRole("region", { name: "Version comparison", exact: true })).toBeVisible();
          await expect(dialog.getByLabel("Change summary")).toContainText("+1 added");
          await expect(dialog.getByRole("button", { name: "Restore this version" })).toBeEnabled();
        });
        await page.getByRole("dialog").getByRole("button", { name: "Close", exact: true }).first().click();
        await measure("history-changes", async () => {
          await page.getByRole("button", { name: "History", exact: true }).click();
          await expect(page.getByRole("tabpanel", { name: "Changes history" }).locator("ol > li")).toHaveCount(50);
        });
        const history = page.getByRole("dialog", { name: "History", exact: true });
        await measure("history-audit", async () => {
          await history.getByRole("tab", { name: "Audit", exact: true }).click();
          await expect(history.getByText("No audit events.", { exact: true }).or(history.locator("ol > li").first())).toBeVisible();
        });
        for (const [tab, empty] of [["MCP", "No MCP calls."], ["CLI", "No CLI calls."], ["Jobs", "No background jobs yet."]]) {
          await measure(`history-${tab.toLowerCase()}-empty`, async () => {
            await history.getByRole("tab", { name: tab, exact: true }).click();
            await expect(history.getByText(empty, { exact: true })).toBeVisible();
          });
        }
        await measure("history-api-first-page", async () => {
          await history.getByRole("tab", { name: "API", exact: true }).click();
          await expect(history.locator("ol > li")).toHaveCount(50);
        });
        await measure("history-api-next-page", async () => {
          await history.getByRole("button", { name: "Load more", exact: true }).click();
          await expect(history.locator("ol > li")).toHaveCount(fixture.apiCalls);
        });
        await history.getByRole("button", { name: "Close", exact: true }).click();
        await measure("trash-first-page", async () => {
          await page.getByRole("button", { name: "Trash", exact: true }).click();
          await expect(page.getByRole("list", { name: "Deleted items" }).locator("li")).toHaveCount(50);
        });
        const trash = page.getByRole("dialog", { name: "Trash", exact: true });
        await measure("trash-next-page", async () => {
          await trash.getByRole("button", { name: "Load more", exact: true }).click();
          await expect(trash.getByRole("list", { name: "Deleted items" }).locator("li")).toHaveCount(fixture.trashItems);
        });
        await measure("trash-select-details", async () => {
          await trash.getByRole("button", { name: "Select deleted-0.md", exact: true }).click();
          await expect(trash.getByRole("region", { name: "Details for deleted-0.md" })).toBeVisible();
          await expect(trash.getByRole("button", { name: "Restore deleted-0.md", exact: true })).toBeEnabled();
        });
        expect(failures, "unexpected API or browser errors").toEqual([]);
      } finally {
        await context.close();
      }
    }
    completed = true;
  } finally {
    writeReport(measurements, { ...fixture, documentBytes: Buffer.byteLength(content) }, completed);
    await api.dispose();
  }
});
