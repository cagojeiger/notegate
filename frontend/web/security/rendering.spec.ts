import { mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { createServer } from "node:http";
import { resolve } from "node:path";
import { expect, test, type BrowserContext, type Page } from "@playwright/test";

import type { Me, ReadTextResponse, RestNode, Space, SpacesListResponse } from "../src/api/types";

const origin = "http://127.0.0.1:9191";
const resultsDir = resolve(import.meta.dirname, "../../../security-results");
const png = Buffer.from("iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mNk+A8AAQUBAScY42YAAAAASUVORK5CYII=", "base64");
type ProbeResult = {
  id: string;
  outcome: "pass" | "finding" | "setup-error";
  phase: string;
  pageErrors: number;
  collectorRequests: number;
  blockedOutOfScopeRequests: number;
  screenshot?: string;
};
class SetupError extends Error {}

function writeResults(result: ProbeResult) {
  mkdirSync(resultsDir, { recursive: true });
  const jsonPath = resolve(resultsDir, "browser-results.json");
  // Playwright replaces a worker after a failed test. Keep earlier findings on disk.
  let previous: ProbeResult[] = [];
  try {
    previous = (JSON.parse(readFileSync(jsonPath, "utf8")) as { results: ProbeResult[] }).results;
  } catch (error) {
    if ((error as NodeJS.ErrnoException).code !== "ENOENT") throw error;
  }
  const results = [...previous.filter((entry) => entry.id !== result.id), result];
  writeFileSync(jsonPath, JSON.stringify({
    target: origin,
    scope: "Disposable CI application; synthetic documents; loopback image collector only",
    results
  }, null, 2));
  writeFileSync(resolve(resultsDir, "summary-browser.md"), [
    "# Browser security probes",
    "",
    "Real authenticated HTTP requests and production browser rendering. No API response mocks.",
    "A setup error is incomplete coverage, never a passing security result.",
    "",
    "| Probe | Outcome | Phase | Page errors | Image requests |",
    "| --- | --- | --- | ---: | ---: |",
    ...results.map((result) => `| ${result.id} | ${result.outcome} | ${result.phase} | ${result.pageErrors} | ${result.collectorRequests} |`),
    ""
  ].join("\n"));
}

const probes = [
  { id: "yaml-shared-alias", frontmatter: "first: &shared { label: synthetic }\nsecond: *shared" },
  { id: "yaml-object-cycle", frontmatter: "value: &cycle { self: *cycle }" },
  { id: "yaml-array-cycle", frontmatter: "value: &cycle [*cycle]" },
  { id: "markdown-image-consent" },
  { id: "mermaid-image-consent" }
];

for (const probe of probes) {
  test(probe.id, async ({ browser }) => {
    const result: ProbeResult = {
      id: probe.id, outcome: "setup-error", phase: "configuration",
      pageErrors: 0, collectorRequests: 0, blockedOutOfScopeRequests: 0
    };
    let context: BrowserContext | undefined;
    let page: Page | undefined;
    const collector = createServer((request, response) => {
      if (request.method !== "GET" || request.url !== "/synthetic.png") {
        response.writeHead(404).end();
        return;
      }
      result.collectorRequests += 1;
      response.writeHead(200, { "Content-Type": "image/png", "Cache-Control": "no-store" });
      response.end(png);
    });
    let testingPayload = false;
    try {
      const session = process.env.NOTEGATE_DAST_BROWSER_SESSION_A;
      if (process.env.GITHUB_ACTIONS !== "true" || !session) throw new SetupError("Missing disposable CI identity.");
      await new Promise<void>((resolve, reject) => {
        collector.once("error", reject);
        collector.listen(0, "127.0.0.1", resolve);
      });
      const address = collector.address();
      if (!address || typeof address === "string") throw new SetupError("Missing loopback collector.");
      const collectorOrigin = `http://127.0.0.1:${address.port}`;
      const image = `${collectorOrigin}/synthetic.png`;
      context = await browser.newContext({
        baseURL: origin, viewport: { width: 1440, height: 900 }, serviceWorkers: "block"
      });
      await context.addCookies([{
        name: "notegate_browser_session", value: session, url: origin, httpOnly: true, sameSite: "Lax"
      }]);
      await context.route("**/*", async (route) => {
        const url = new URL(route.request().url());
        if (url.origin === origin || url.origin === collectorOrigin || ["data:", "blob:"].includes(url.protocol)) {
          await route.continue();
        } else {
          result.blockedOutOfScopeRequests += 1;
          await route.abort("blockedbyclient");
        }
      });
      const api = context.request;
      async function request<T>(path: string, method = "GET", data?: unknown): Promise<T> {
        // Never let Playwright's transport error include cookie headers in the report.
        let response;
        try {
          response = await api.fetch(path, { method, data, headers: { Origin: origin } });
        } catch {
          throw new SetupError("Fixture transport failed.");
        }
        if (!response.ok()) throw new SetupError(`Fixture HTTP request returned ${response.status()}.`);
        return await response.json() as T;
      }
      result.phase = "authenticated-fixture";
      const me = await request<Me>("/api/v1/me");
      if (!me.account.id || me.account.kind !== "user") throw new SetupError("Expected an authenticated test user.");
      const owned = await request<SpacesListResponse>("/api/v1/spaces");
      const space = owned.spaces.find((space) => space.permission === "write")
        ?? await request<Space>("/api/v1/spaces", "POST", { name: "synthetic-dast-browser" });
      const prefix = `/api/v1/spaces/${space.id}`;
      async function create(name: string, content: string) {
        const node = await request<RestNode>(`${prefix}/nodes`, "POST", {
          parent_id: space.root_node_id, name, kind: "text", content
        });
        const saved = await request<ReadTextResponse>(`${prefix}/text/${node.id}?max_lines=5000&max_bytes=1048576`);
        if (!("content" in saved.text) || saved.text.content !== content) throw new SetupError("Fixture content was not persisted.");
        return node;
      }
      const controlHeading = `Synthetic control ${probe.id}`;
      const heading = `Synthetic payload ${probe.id}`;
      const control = await create(`${probe.id}-control.md`, `# ${controlHeading}\n\nNormal synthetic content.\n${probe.id === "mermaid-image-consent" ? "\n```mermaid\ngraph LR\n  A[Control] --> B[Ready]\n```\n" : ""}`);
      let content = `# ${heading}\n\nSynthetic document body.\n`;
      if ("frontmatter" in probe) content = `---\n${probe.frontmatter}\n---\n${content}`;
      if (probe.id === "markdown-image-consent") content += `\n![Synthetic image](${image})\n`;
      if (probe.id === "mermaid-image-consent") content += `\n\`\`\`mermaid\nflowchart LR\n  A@{ img: "${image}", label: "Synthetic image", pos: "b", h: 40, constraint: "on" }\n\`\`\`\n`;
      const target = await create(`${probe.id}-payload.md`, content);
      page = await context.newPage();
      page.on("pageerror", () => { result.pageErrors += 1; });
      const tree = page.getByRole("tree", { name: "Files", exact: true });
      const preview = page.getByRole("region", { name: "Document preview", exact: true });
      await page.goto("/", { waitUntil: "domcontentloaded" });
      await page.getByRole("complementary", { name: "Space navigation", exact: true })
        .getByRole("button", { name: space.name, exact: true }).click();
      async function open(node: RestNode, expectedContent: string) {
        const [response] = await Promise.all([
          page!.waitForResponse((response) => response.request().method() === "GET"
            && new URL(response.url()).pathname === `${prefix}/text/${node.id}`),
          tree.getByRole("button", { name: node.name, exact: true }).click()
        ]);
        if (!response.ok() || await response.finished()) throw new SetupError("Document HTTP read did not complete.");
        const body = await response.json() as ReadTextResponse;
        if (!("content" in body.text) || body.text.content !== expectedContent) throw new SetupError("Browser did not receive the fixture document.");
      }
      result.phase = "normal-render-positive-control";
      // Fetch the known persisted body to check the browser request against the same input.
      const controlText = await request<ReadTextResponse>(`${prefix}/text/${control.id}?max_lines=5000&max_bytes=1048576`);
      if (!("content" in controlText.text)) throw new SetupError("Missing control body.");
      await open(control, controlText.text.content);
      await expect(preview.getByRole("heading", { name: controlHeading, exact: true })).toBeVisible();
      await expect(tree).toBeVisible();
      expect(result.pageErrors).toBe(0);
      if (probe.id === "mermaid-image-consent") {
        await expect(preview.locator(".ng-mermaid svg")).toHaveCount(0);
        await preview.getByRole("button", { name: "Load diagram", exact: true }).click();
        await expect(preview.locator(".ng-mermaid svg")).toBeVisible();
      }

      result.phase = "payload-http-read";
      await open(target, content);
      testingPayload = true;
      result.phase = "payload-render";
      await expect(preview.getByRole("heading", { name: heading, exact: true })).toBeVisible();
      await expect(tree).toBeVisible();
      if (probe.id === "yaml-shared-alias") {
        await expect(preview.getByText('{"label":"synthetic"}', { exact: true })).toHaveCount(2);
      }
      if (probe.id === "markdown-image-consent") {
        const button = preview.getByRole("button", { name: "Load external image: Synthetic image", exact: true });
        await expect(button).toBeVisible();
        await page.waitForTimeout(300);
        expect(result.collectorRequests, "External image loaded before consent").toBe(0);
        await button.click();
        const loaded = preview.getByRole("img", { name: "Synthetic image", exact: true });
        try {
          await expect.poll(() => result.collectorRequests, { timeout: 5_000 }).toBeGreaterThan(0);
          await expect(loaded).toBeVisible();
          await expect.poll(() => loaded.evaluate((element) => (element as HTMLImageElement).naturalWidth)).toBeGreaterThan(0);
        } catch {
          throw new SetupError("External image positive control did not load.");
        }
      }
      if (probe.id === "mermaid-image-consent") {
        await expect(preview.getByRole("button", { name: "Load diagram", exact: true })).toBeVisible();
        await expect(preview.locator(".ng-mermaid svg")).toHaveCount(0);
        await page.waitForTimeout(300);
        expect(result.collectorRequests, "Mermaid loaded an image before consent").toBe(0);
        await preview.getByRole("button", { name: "Load diagram", exact: true }).click();
        // A parsing failure is incomplete coverage, not proof that image consent works.
        try {
          await expect(preview.locator(".ng-mermaid svg")).toBeVisible();
          await expect.poll(() => result.collectorRequests, { timeout: 5_000 }).toBeGreaterThan(0);
        } catch {
          throw new SetupError("Approved Mermaid image positive control did not finish loading.");
        }
      }
      expect(result.pageErrors, "Unexpected browser exception").toBe(0);
      result.outcome = "pass";
      result.phase = "complete";
    } catch (error) {
      result.outcome = testingPayload && !(error instanceof SetupError) ? "finding" : "setup-error";
      if (page) {
        const filename = `browser-${probe.id}.png`;
        try {
          mkdirSync(resultsDir, { recursive: true });
          await page.screenshot({ path: resolve(resultsDir, filename) });
          result.screenshot = filename;
        } catch { /* A crashed renderer may not provide a screenshot. */ }
      }
      // Details contain only synthetic assertions; suppress transport diagnostics with credentials.
      if (error instanceof SetupError) throw new Error(`${probe.id}: setup failed during ${result.phase}.`);
      throw error;
    } finally {
      writeResults(result);
      await context?.close();
      await new Promise<void>((resolve) => collector.close(() => resolve()));
    }
  });
}
