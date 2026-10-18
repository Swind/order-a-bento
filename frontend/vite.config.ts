/// <reference types="vitest/config" />
import path from "node:path";
import tailwindcss from "@tailwindcss/vite";
import react from "@vitejs/plugin-react";
import { defineConfig } from "vite";

const target = "http://localhost:3000";

export default defineConfig({
  plugins: [react(), tailwindcss()],
  resolve: { alias: { "@": path.resolve(import.meta.dirname, "src") } },
  server: {
    proxy: { "/api": target, "/auth": target, "/healthz": target },
  },
  test: {
    environment: "jsdom",
    globals: true,
    setupFiles: ["./src/test-setup.ts"],
    css: false,
    // Must exceed asyncUtilTimeout (test-setup.ts) so a slow findBy* reports its own error.
    testTimeout: 15000,
  },
});
