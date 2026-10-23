import { defineConfig, devices } from "@playwright/test";

export default defineConfig({
  testDir: "./e2e",
  outputDir: "./test-results/browser-functional",
  timeout: 180_000,
  forbidOnly: !!process.env.CI,
  expect: { timeout: 10_000 },
  workers: 1,
  retries: 0,
  reporter: "list",
  use: {
    ...devices["Desktop Chrome"],
    viewport: { width: 1440, height: 900 },
    // Disposable credentials and the single-use consent fixture must not enter traces.
    trace: "off"
  },
  projects: [
    {
      name: "workbench",
      testMatch: "workbench-live.spec.ts",
      use: { baseURL: process.env.NOTEGATE_WEB_BASE_URL ?? "http://127.0.0.1:5173" }
    },
    {
      name: "standalone-oauth",
      testMatch: "standalone-oauth.spec.ts",
      use: { baseURL: process.env.NOTEGATE_OAUTH_BASE_URL ?? "http://127.0.0.1:9191" }
    }
  ]
});
