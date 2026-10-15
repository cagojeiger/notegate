import { appendFileSync, writeFileSync } from "node:fs";
import { expect, test, type Request } from "@playwright/test";

import type { RestNode, Space, ReadTextResponse, TextResponse } from "../src/api/types";
import { instrument, writeReport, type Measurement } from "./measure";

const fixture = { rootDocuments: 120, nestedDepth: 3, trashItems: 55, apiCalls: 55, trials: 3 };
// Keep roughly 87KB while fitting the diff contract (1,500 combined lines).
const content = `# Baseline document\n\n${Array.from({ length: 300 }, (_, index) =>
  `Paragraph ${index}: ${"synthetic Markdown for browser rendering, with **emphasis** and a stable measurement payload. ".repeat(3)}\n`
).join("\n")}`;

function textResponseShape({ text }: ReadTextResponse) {
  return {
    unchanged: "unchanged" in text && text.unchanged === true,
    contentReturned: "content_returned" in text ? text.content_returned : null,
    fullBody: "content" in text && typeof text.content === "string",
    updatedAtPresent: "updated_at" in text,
    updatedByPresent: "updated_by" in text
  };
}

type TextRevalidation = ReturnType<typeof textResponseShape> & {
  trial: number; getStarts: number; conditional: boolean; cacheBodyCorrect: boolean;
  revalidationMs: number; encodedBodyBytes: number; decodedBodyBytes: number; transferBytes: number;
};

test("authenticated workbench performance baseline", async ({ browser, playwright, baseURL }) => {
  // A missing identity is a configuration failure, never a silently skipped benchmark.
  const session = process.env.NOTEGATE_WEB_E2E_BROWSER_SESSION;
  const apiKey = process.env.NOTEGATE_PUBLIC_V2_API_KEY;
  expect(session).toBeTruthy();
  expect(apiKey).toBeTruthy();
  expect(new URL(baseURL!).hostname).toMatch(/^(localhost|127\.0\.0\.1)$/);
  const api = await playwright.request.newContext({ baseURL, extraHTTPHeaders: {
    Cookie: `notegate_browser_session=${session}`,
    Origin: new URL(baseURL!).origin
  } });
  async function request<T>(path: string, method = "GET", data?: unknown): Promise<T> {
    const response = await api.fetch(path, { method, data });
    expect(response.ok(), `${method} fixture request returned ${response.status()}`).toBe(true);
    return response.status() === 204 ? undefined as T : await response.json() as T;
  }
  const measurements: Measurement[] = [];
  const textRevalidations: TextRevalidation[] = [];
  let conditionalProbe: (ReturnType<typeof textResponseShape> & { jsonBodyBytes: number }) | null = null;
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
      const written = await request<TextResponse>(`${path}/text/${document.id}`, "PUT", { storage_format: "plain", content: initial });
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
        // Recent reveal virtualizes the first Files row away; use the real editor history.
        await page.getByRole("button", { name: "Back to 000-entry.md", exact: true }).click();
        await expect(preview.getByRole("heading", { name: "Entry 0", exact: true })).toBeVisible();
        // Real time makes the inactive text cache stale (5s); exclude it from the journey.
        await page.waitForTimeout(5_500);
        const textPath = `${path}/text/${document.id}`;
        let textGetStarts = 0;
        const trackText = (request: Request) => {
          if (request.method() === "GET" && new URL(request.url()).pathname === textPath) textGetStarts += 1;
        };
        page.on("request", trackText);
        try {
          await measure("unchanged-text-reopen", async () => {
            const since = await page.evaluate(() => performance.now());
            const [response] = await Promise.all([
              page.waitForResponse((response) => response.request().method() === "GET"
                && new URL(response.url()).pathname === textPath),
              recent.getByRole("button", { name: /^baseline\.md/ }).click()
            ]);
            expect(response.status()).toBe(200);
            expect(await response.finished()).toBeNull();
            const body = await response.json() as ReadTextResponse;
            const shape = textResponseShape(body);
            if ("content" in body.text) expect(body.text.content).toBe(initial);
            else expect(shape.unchanged && shape.contentReturned === false).toBe(true);
            const resource = () => page.evaluate(({ url, since }) => {
              const entries = performance.getEntriesByType("resource") as PerformanceResourceTiming[];
              const entry = entries.find((entry) => entry.name === url && entry.startTime >= since && entry.responseEnd > 0);
              return entry ? {
                revalidationMs: entry.responseEnd - since,
                encodedBodyBytes: entry.encodedBodySize,
                decodedBodyBytes: entry.decodedBodySize,
                transferBytes: entry.transferSize
              } : null;
            }, { url: response.url(), since });
            await expect.poll(resource).not.toBeNull();
            const bytes = await resource();
            if (!bytes) throw new Error("Missing completed text resource timing");
            expect(bytes.encodedBodyBytes).toBeGreaterThan(0);
            expect(bytes.decodedBodyBytes).toBeGreaterThan(0);
            expect(bytes.transferBytes).toBeGreaterThan(0);
            await expect(preview.getByRole("heading", { name: "Baseline document", exact: true })).toBeVisible();
            await expect(page.getByRole("button", { name: "Edit", exact: true })).toBeEnabled();
            textRevalidations.push({ trial, ...shape, ...bytes, getStarts: 0, cacheBodyCorrect: false,
              conditional: new URL(response.request().url()).searchParams.has("if_none_match_sha256") });
          });
        } finally {
          page.off("request", trackText);
          const sample = textRevalidations.find((sample) => sample.trial === trial);
          if (sample) sample.getStarts = textGetStarts;
        }
        expect(textGetStarts, "text GET starts during stale reopen").toBe(1);
        await measure("editor-open", async () => {
          await page.getByRole("button", { name: "Edit", exact: true }).click();
          await expect(page.getByRole("textbox", { name: "Edit text content" })).toHaveValue(initial);
        });
        expect(measurements.at(-1)?.api.filter((sample) => sample.route.startsWith("GET ") && sample.route.includes("/text/"))).toHaveLength(0);
        textRevalidations[textRevalidations.length - 1].cacheBodyCorrect = true;
        if (trial === 1) {
          // APIRequestContext is separate from the page: JSON bytes are not browser wire bytes.
          const probe = await api.get(`${textPath}?max_lines=5000&max_bytes=1048576&if_none_match_sha256=${encodeURIComponent(written.text.content_sha256)}`);
          expect(probe.status()).toBe(200);
          const body = await probe.body();
          conditionalProbe = { ...textResponseShape(JSON.parse(body.toString()) as ReadTextResponse), jsonBodyBytes: body.byteLength };
          expect(conditionalProbe.unchanged).toBe(true);
          expect(conditionalProbe.contentReturned).toBe(false);
          expect(conditionalProbe.fullBody).toBe(false);
        }
        const updated = initial.replace("Paragraph 150:", "Changed paragraph 150:");
        await page.getByRole("textbox", { name: "Edit text content" }).fill(updated);
        await measure("save-to-preview", async () => {
          await page.getByRole("button", { name: "Save", exact: true }).click();
          await expect(page.getByRole("textbox", { name: "Edit text content" })).toHaveCount(0);
          await expect(preview).toContainText("Changed paragraph 150:");
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
    const directory = "test-results/browser-performance";
    writeFileSync(`${directory}/text-revalidation.json`, JSON.stringify({ completed, measurements: textRevalidations, conditionalProbe }, null, 2));
    appendFileSync(`${directory}/summary.md`, [
      "", "## Unchanged text revalidation", "",
      "Stale reopen after 5,500ms outside the measured journey. Exact text GET only; no mocked network. Revalidation ends at responseEnd, not paint.",
      "Encoded/decoded sizes are response-body bytes; transfer includes browser-reported overhead. No URLs, identifiers, content hashes, bodies, or headers are exported.", "",
      "| Trial | GET starts | Conditional | Unchanged | Full body | Content returned | Updated at/by present | Cache body correct | Revalidation ms | Encoded B | Decoded B | Transfer B |",
      "|---|---:|---|---|---|---|---|---|---:|---:|---:|---:|",
      ...textRevalidations.map((row) => `| ${row.trial} | ${row.getStarts} | ${row.conditional} | ${row.unchanged} | ${row.fullBody} | ${row.contentReturned ?? "absent"} | ${row.updatedAtPresent}/${row.updatedByPresent} | ${row.cacheBodyCorrect} | ${row.revalidationMs.toFixed(1)} | ${row.encodedBodyBytes} | ${row.decodedBodyBytes} | ${row.transferBytes} |`),
      "", "Separate APIRequestContext probe: JSON response bytes, not browser wire bytes.", "",
      "| JSON bytes | Unchanged | Full body | Content returned | Updated at/by present |",
      "|---:|---|---|---|---|",
      ...(conditionalProbe ? [`| ${conditionalProbe.jsonBodyBytes} | ${conditionalProbe.unchanged} | ${conditionalProbe.fullBody} | ${conditionalProbe.contentReturned} | ${conditionalProbe.updatedAtPresent}/${conditionalProbe.updatedByPresent} |`] : []), ""
    ].join("\n"));
    await api.dispose();
  }
});
