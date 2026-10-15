import { defineConfig } from "vitest/config";

export default defineConfig({
  test: {
    environment: "jsdom",
    setupFiles: ["src/test/setup.ts"],
    exclude: ["e2e/**", "performance/**", "node_modules/**", "dist/**"]
  }
});
