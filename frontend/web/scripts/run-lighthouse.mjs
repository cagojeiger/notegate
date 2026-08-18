import { createServer } from "node:http";
import { mkdir, readFile, rm, stat, writeFile } from "node:fs/promises";
import { extname, resolve, sep } from "node:path";
import { promisify } from "node:util";
import { gzip } from "node:zlib";

import { chromium } from "@playwright/test";
import { launch } from "chrome-launcher";
import lighthouse from "lighthouse";

const distDir = resolve("dist");
const reportsDir = resolve("lighthouse-reports");
const runs = 3;
const gzipAsync = promisify(gzip);
const gzipCache = new Map();
const compressibleExtensions = new Set([
  ".css",
  ".html",
  ".js",
  ".json",
  ".mjs",
  ".svg",
  ".webmanifest"
]);
const thresholds = [
  {
    name: "performance score",
    mode: "optimistic",
    aggregation: "max",
    limit: 0.9,
    passes: (value) => value >= 0.9,
    value: (result) => categoryScore(result, "performance")
  },
  {
    name: "accessibility score",
    mode: "optimistic",
    aggregation: "max",
    limit: 1,
    passes: (value) => value >= 1,
    value: (result) => categoryScore(result, "accessibility")
  },
  {
    name: "best-practices score",
    mode: "optimistic",
    aggregation: "max",
    limit: 0.9,
    passes: (value) => value >= 0.9,
    value: (result) => categoryScore(result, "best-practices")
  },
  {
    name: "largest-contentful-paint",
    mode: "optimistic",
    aggregation: "min",
    limit: 2_500,
    passes: (value) => value <= 2_500,
    value: (result) => auditNumericValue(result, "largest-contentful-paint")
  },
  {
    name: "cumulative-layout-shift",
    mode: "optimistic",
    aggregation: "min",
    limit: 0.1,
    passes: (value) => value <= 0.1,
    value: (result) => auditNumericValue(result, "cumulative-layout-shift")
  },
  {
    name: "total-blocking-time",
    mode: "optimistic",
    aggregation: "min",
    limit: 200,
    passes: (value) => value <= 200,
    value: (result) => auditNumericValue(result, "total-blocking-time")
  },
  {
    name: "script transfer size",
    mode: "pessimistic",
    aggregation: "max",
    limit: 122_000,
    passes: (value) => value <= 122_000,
    value: scriptTransferSize
  }
];

const server = createStaticServer();
let chrome;

try {
  await rm(reportsDir, { recursive: true, force: true });
  await mkdir(reportsDir, { recursive: true });
  const port = await listen(server);
  const url = `http://127.0.0.1:${port}/`;
  chrome = await launch({
    chromePath: process.env.CHROME_PATH || chromium.executablePath(),
    chromeFlags: [
      "--headless=new",
      "--disable-dev-shm-usage",
      ...(process.env.CI ? ["--no-sandbox"] : [])
    ],
    logLevel: "silent"
  });

  const results = [];
  for (let run = 1; run <= runs; run += 1) {
    const result = await lighthouse(url, {
      port: chrome.port,
      output: "html",
      logLevel: "error",
      onlyCategories: ["performance", "accessibility", "best-practices"]
    });
    if (!result) throw new Error(`Lighthouse run ${run} returned no result`);
    if (typeof result.report !== "string") {
      throw new Error(`Lighthouse run ${run} returned an unexpected report format`);
    }
    results.push(result.lhr);
    await writeFile(resolve(reportsDir, `run-${run}.report.html`), result.report);
    await writeFile(
      resolve(reportsDir, `run-${run}.report.json`),
      `${JSON.stringify(result.lhr, null, 2)}\n`
    );
  }

  const summary = evaluate(results);
  await writeFile(
    resolve(reportsDir, "summary.json"),
    `${JSON.stringify(summary, null, 2)}\n`
  );
  for (const result of summary) {
    console.log(
      `${result.passed ? "PASS" : "FAIL"} ${result.name}: ${result.aggregate} `
      + `(${result.mode}, limit ${result.limit}; runs ${result.values.join(", ")})`
    );
  }
  if (summary.some((result) => !result.passed)) {
    throw new Error("Lighthouse budgets failed");
  }
} finally {
  chrome?.kill();
  await close(server);
}

function evaluate(results) {
  return thresholds.map((threshold) => {
    const values = results.map(threshold.value);
    const aggregate = threshold.aggregation === "max"
      ? Math.max(...values)
      : Math.min(...values);
    return {
      name: threshold.name,
      mode: threshold.mode,
      limit: threshold.limit,
      values,
      aggregate,
      passed: threshold.passes(aggregate)
    };
  });
}

function categoryScore(result, category) {
  const score = result.categories[category]?.score;
  if (typeof score !== "number") throw new Error(`Missing Lighthouse category: ${category}`);
  return score;
}

function auditNumericValue(result, audit) {
  const value = result.audits[audit]?.numericValue;
  if (typeof value !== "number") throw new Error(`Missing Lighthouse audit: ${audit}`);
  return value;
}

function scriptTransferSize(result) {
  const details = result.audits["resource-summary"]?.details;
  const items = details && "items" in details ? details.items : undefined;
  const script = Array.isArray(items)
    ? items.find((item) => item.resourceType === "script")
    : undefined;
  const size = script && "transferSize" in script ? script.transferSize : undefined;
  if (typeof size !== "number") throw new Error("Missing Lighthouse script transfer size");
  return size;
}

function createStaticServer() {
  return createServer(async (request, response) => {
    try {
      const url = new URL(request.url ?? "/", "http://127.0.0.1");
      const relativePath = decodeURIComponent(url.pathname).replace(/^\/+/, "") || "index.html";
      let filePath = resolve(distDir, relativePath);
      if (filePath !== distDir && !filePath.startsWith(`${distDir}${sep}`)) {
        response.writeHead(403).end();
        return;
      }
      if (!(await isFile(filePath))) filePath = resolve(distDir, "index.html");
      let body = await readFile(filePath);
      const headers = {
        "Cache-Control": "no-store",
        "Content-Type": contentType(filePath),
        Vary: "Accept-Encoding"
      };
      if (acceptsGzip(request) && isCompressible(filePath)) {
        const cached = gzipCache.get(filePath);
        body = cached ?? await gzipAsync(body);
        if (!cached) gzipCache.set(filePath, body);
        headers["Content-Encoding"] = "gzip";
      }
      headers["Content-Length"] = String(body.byteLength);
      response.writeHead(200, headers);
      response.end(body);
    } catch (error) {
      console.error(error);
      response.writeHead(500).end();
    }
  });
}

function acceptsGzip(request) {
  return /(?:^|,)\s*gzip\s*(?:,|$)/iu.test(request.headers["accept-encoding"] ?? "");
}

function isCompressible(path) {
  return compressibleExtensions.has(extname(path));
}

async function isFile(path) {
  try {
    return (await stat(path)).isFile();
  } catch {
    return false;
  }
}

function listen(server) {
  return new Promise((resolvePort, reject) => {
    server.once("error", reject);
    server.listen(0, "127.0.0.1", () => {
      const address = server.address();
      if (!address || typeof address === "string") {
        reject(new Error("Static server did not expose a TCP port"));
        return;
      }
      resolvePort(address.port);
    });
  });
}

function close(server) {
  if (!server.listening) return Promise.resolve();
  return new Promise((resolveClose, reject) => {
    server.close((error) => error ? reject(error) : resolveClose());
  });
}

function contentType(path) {
  return {
    ".css": "text/css; charset=utf-8",
    ".html": "text/html; charset=utf-8",
    ".js": "text/javascript; charset=utf-8",
    ".json": "application/json; charset=utf-8",
    ".mjs": "text/javascript; charset=utf-8",
    ".png": "image/png",
    ".svg": "image/svg+xml",
    ".webmanifest": "application/manifest+json",
    ".woff2": "font/woff2"
  }[extname(path)] ?? "application/octet-stream";
}
