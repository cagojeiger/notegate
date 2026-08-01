import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";
import { readFileSync } from "node:fs";

const backend = "http://localhost:9191";
const appVersion = readFileSync(new URL("../../VERSION", import.meta.url), "utf8").trim();
const backendProxy = {
  target: backend,
  changeOrigin: true,
  headers: {
    Origin: backend
  }
};

export default defineConfig({
  plugins: [react()],
  define: {
    "import.meta.env.VITE_NOTEGATE_VERSION": JSON.stringify(appVersion)
  },
  server: {
    port: 5173,
    proxy: {
      "/api": backendProxy,
      "/auth": backendProxy,
      "/mcp": backendProxy,
      "/openapi": backendProxy,
      "/swagger-ui": backendProxy,
      "/.well-known": backendProxy
    }
  }
});
