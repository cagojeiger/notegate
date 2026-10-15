import { defineConfig, devices } from "@playwright/test";

export default defineConfig({
  testDir: "./performance",
  outputDir: "./test-results/browser-performance",
  timeout: 180_000,
  expect: { timeout: 10_000 },
  workers: 1,
  retries: 0,
  reporter: "list",
  use: {
    ...devices["Desktop Chrome"],
    viewport: { width: 1440, height: 900 },
    baseURL: process.env.NOTEGATE_WEB_BASE_URL ?? "http://127.0.0.1:9191",
    // Request headers contain the disposable session. Export measurements, not traces.
    trace: "off"
  }
});
