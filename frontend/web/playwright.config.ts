import { defineConfig, devices } from "@playwright/test";

const baseURL = process.env.NOTEGATE_WEB_BASE_URL ?? "http://127.0.0.1:5173";

export default defineConfig({
  testDir: "./e2e",
  timeout: 30_000,
  expect: { timeout: 10_000 },
  use: {
    baseURL,
    channel: "chrome",
    trace: "on-first-retry"
  },
  projects: [
    {
      name: "desktop-chrome",
      use: { ...devices["Desktop Chrome"] }
    }
  ]
});
