import { defineConfig, devices } from "@playwright/test";

const baseURL = process.env.NOTEGATE_WEB_BASE_URL ?? "http://127.0.0.1:5173";

export default defineConfig({
  testDir: "./e2e",
  timeout: 30_000,
  retries: process.env.CI ? 1 : 0,
  expect: { timeout: 10_000 },
  use: {
    baseURL,
    trace: "retain-on-failure"
  },
  projects: [
    {
      name: "desktop-chromium",
      use: { ...devices["Desktop Chrome"] }
    }
  ]
});
