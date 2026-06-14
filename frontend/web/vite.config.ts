import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";

const backend = "http://localhost:9191";
const backendProxy = {
  target: backend,
  changeOrigin: true,
  headers: {
    Origin: backend
  }
};

export default defineConfig({
  plugins: [react()],
  server: {
    port: 5173,
    proxy: {
      "/api": backendProxy,
      "/auth": backendProxy,
      "/mcp": backendProxy,
      "/.well-known": backendProxy
    }
  }
});
