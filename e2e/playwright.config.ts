import { defineConfig } from "@playwright/test";
import os from "node:os";
import path from "node:path";

const PORT = 3990;
export const BASE_URL = `http://localhost:${PORT}`;
// Fresh DB directory per run: removed before the server starts and again in globalTeardown.
export const DB_DIR = path.join(os.tmpdir(), `bento-e2e-${PORT}`);

const launchOptions = process.env.CHROME_PATH
  ? { executablePath: process.env.CHROME_PATH }
  : {};

const LINE_UA =
  "Mozilla/5.0 (iPhone; CPU iPhone OS 18_1 like Mac OS X) AppleWebKit/605.1.15 (KHTML, like Gecko) Mobile/15E148 Safari Line/14.17.0";

export default defineConfig({
  testDir: "./tests",
  outputDir: "./test-results",
  globalTeardown: "./global-teardown.ts",
  fullyParallel: true,
  workers: process.env.CI ? 2 : 4,
  retries: process.env.CI ? 1 : 0,
  forbidOnly: !!process.env.CI,
  reporter: [["list"], ["html", { open: "never" }]],
  use: {
    baseURL: BASE_URL,
    trace: "retain-on-failure",
    screenshot: "only-on-failure",
    launchOptions,
  },
  projects: [
    {
      name: "mobile",
      use: {
        browserName: "chromium",
        viewport: { width: 390, height: 844 },
        deviceScaleFactor: 2,
        isMobile: true,
        hasTouch: true,
        userAgent: LINE_UA,
      },
    },
    {
      name: "desktop",
      testMatch: ["auth-directory.spec.ts", "management.spec.ts"],
      use: {
        browserName: "chromium",
        viewport: { width: 1280, height: 800 },
      },
    },
  ],
  webServer: {
    command: `rm -rf "${DB_DIR}" && mkdir -p "${DB_DIR}" && exec ../backend/target/release/server`,
    url: `${BASE_URL}/healthz`,
    reuseExistingServer: false,
    timeout: 60_000,
    stdout: "ignore",
    stderr: "pipe",
    env: {
      PORT: String(PORT),
      APP_ENV: "development",
      DEV_LOGIN: "true",
      AI_DEMO: "true",
      APP_BASE_URL: BASE_URL,
      ALLOWED_ORIGINS: BASE_URL,
      PUBLIC_RATE_LIMIT_BURST: "1000",
      DATABASE_URL: `sqlite://${DB_DIR}/e2e.db?mode=rwc`,
    },
  },
});
