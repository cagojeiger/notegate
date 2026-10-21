import { resolve } from "node:path";
import { defineConfig, devices } from "@playwright/test";

if (process.env.GITHUB_ACTIONS !== "true") {
  throw new Error("Security probes run only against the disposable GitHub Actions environment.");
}

export default defineConfig({
  testDir: "./security",
  outputDir: resolve(import.meta.dirname, "../../security-results/browser-output"),
  timeout: 45_000,
  forbidOnly: true,
  globalTimeout: 240_000,
  expect: { timeout: 10_000 },
  workers: 1,
  retries: 0,
  maxFailures: 0,
  reporter: "list",
  use: {
    ...devices["Desktop Chrome"],
    viewport: { width: 1440, height: 900 },
    baseURL: "http://127.0.0.1:9191",
    serviceWorkers: "block",
    trace: "off",
    video: "off",
    screenshot: "off"
  }
});
