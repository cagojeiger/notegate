import { mkdirSync, writeFileSync } from "node:fs";
import type { Page, Request } from "@playwright/test";

declare global {
  interface Window {
    baselineLongTasks: Array<{ start: number; duration: number }>;
  }
}

type ApiSample = { route: string; status: number | null; durationMs: number | null; failed: boolean };
export type Measurement = {
  trial: number;
  journey: string;
  readyMs: number;
  longTasks: number;
  longTaskMs: number;
  maxLongTaskMs: number;
  transferredBytes: number;
  api: ApiSample[];
};

export async function instrument(page: Page, trial: number, measurements: Measurement[]) {
  await page.addInitScript(() => {
    window.baselineLongTasks = [];
    performance.setResourceTimingBufferSize(2_000);
    new PerformanceObserver((list) => {
      for (const entry of list.getEntries()) {
        window.baselineLongTasks.push({ start: entry.startTime, duration: entry.duration });
      }
    }).observe({ type: "longtask", buffered: true });
  });
  let active: ApiSample[] | null = null;
  const requests = new Map<Request, ApiSample>();
  const failures: string[] = [];
  page.on("pageerror", (error) => failures.push(error.name));
  page.on("request", (request) => {
    const url = new URL(request.url());
    if (!url.pathname.startsWith("/api/")) return;
    // Never export headers, bodies, query strings, or identifiers.
    const sample: ApiSample = {
      route: `${request.method()} ${url.pathname.replace(/[0-9a-f]{8}-[0-9a-f-]{27,}/gi, ":id")}`,
      status: null, durationMs: null, failed: false
    };
    requests.set(request, sample);
    active?.push(sample);
  });
  page.on("response", (response) => {
    const sample = requests.get(response.request());
    if (!sample) return;
    sample.status = response.status();
    if (response.status() >= 400) failures.push(`${sample.route}: ${response.status()}`);
  });
  page.on("requestfinished", (request) => {
    const sample = requests.get(request);
    if (sample) {
      sample.durationMs = request.timing().responseEnd;
      requests.delete(request);
    }
  });
  page.on("requestfailed", (request) => {
    const sample = requests.get(request);
    if (sample) {
      sample.failed = true;
      failures.push(`${sample.route}: request failed`);
      requests.delete(request);
    }
  });

  return {
    failures,
    async measure(journey: string, actionAndReady: () => Promise<void>, navigation = false) {
      const api: ApiSample[] = [];
      active = api;
      const start = navigation ? 0 : await page.evaluate(() => performance.now());
      try {
        await actionAndReady();
        const result = await page.evaluate(async (since) => {
          // Allow a paint opportunity after the asserted DOM state. This includes
          // automation/assertion overhead; it is not INP or a paint timestamp.
          await new Promise<void>((resolve) => requestAnimationFrame(() => requestAnimationFrame(() => resolve())));
          const end = performance.now();
          const tasks = window.baselineLongTasks.filter((task) => task.start >= since && task.start < end);
          const resources = performance.getEntriesByType("resource") as PerformanceResourceTiming[];
          const navigationEntry = performance.getEntriesByType("navigation")[0] as PerformanceNavigationTiming | undefined;
          return {
            readyMs: end - since,
            longTasks: tasks.length,
            longTaskMs: tasks.reduce((sum, task) => sum + task.duration, 0),
            maxLongTaskMs: Math.max(0, ...tasks.map((task) => task.duration)),
            transferredBytes: resources.filter((entry) => entry.startTime >= since && entry.responseEnd <= end)
              .reduce((sum, entry) => sum + entry.transferSize, 0) + (since === 0 ? navigationEntry?.transferSize ?? 0 : 0)
          };
        }, start);
        measurements.push({ trial, journey, ...result, api });
      } finally {
        active = null;
      }
    }
  };
}

export function writeReport(measurements: Measurement[], fixture: Record<string, number>, completed: boolean) {
  const directory = "test-results/browser-performance";
  mkdirSync(directory, { recursive: true });
  writeFileSync(`${directory}/measurements.json`, JSON.stringify({
    commit: process.env.GITHUB_SHA ?? null, completed, fixture, measurements
  }, null, 2));
  const lines = [
    "## Authenticated browser baseline", "",
    `Completed: ${completed}. Three fresh browser contexts; one warm release server and disposable database.`,
    "1440×900 Chromium, no CPU/network throttling. Latencies include automation and DOM assertions, then two animation frames; not INP or production capacity.",
    "API count/timing covers requests started during the journey; byte counts cover completed resources (including lazy JS), with cache hits possibly zero. Pending durations remain null.",
    "", "| Journey | Samples | Ready median / max ms | >1s | API count median | API max ms | Long-task max ms | Transfer median KiB |",
    "|---|---:|---:|---:|---:|---:|---:|---:|"
  ];
  const median = (values: number[]) => {
    const sorted = [...values].sort((a, b) => a - b);
    return (sorted[Math.floor((sorted.length - 1) / 2)] + sorted[Math.floor(sorted.length / 2)]) / 2;
  };
  for (const journey of new Set(measurements.map((item) => item.journey))) {
    const rows = measurements.filter((item) => item.journey === journey);
    const times = rows.map((item) => item.readyMs);
    lines.push(`| ${journey} | ${rows.length} | ${median(times).toFixed(1)} / ${Math.max(...times).toFixed(1)} | ${times.filter((ms) => ms > 1000).length} | ${median(rows.map((item) => item.api.length))} | ${Math.max(0, ...rows.flatMap((item) => item.api.map((api) => api.durationMs ?? 0))).toFixed(1)} | ${Math.max(...rows.map((item) => item.maxLongTaskMs)).toFixed(1)} | ${(median(rows.map((item) => item.transferredBytes)) / 1024).toFixed(1)} |`);
  }
  const summary = `${lines.join("\n")}\n`;
  writeFileSync(`${directory}/summary.md`, summary);
}
