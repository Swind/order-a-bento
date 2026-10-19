import { defineConfig } from "@playwright/test";
import os from "node:os";
import path from "node:path";
import { E2E_ADMIN_EMAIL, E2E_ADMIN_NAME, E2E_ADMIN_PASSWORD } from "./admin-account";

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
      testMatch: ["auth-directory.spec.ts", "management.spec.ts", "sharing-groups.spec.ts"],
      use: {
        browserName: "chromium",
        viewport: { width: 1280, height: 800 },
      },
    },
  ],
  webServer: {
    // Fresh DB, then the local admin via the CLI (no web UI, no Google), then the server itself.
    command:
      `rm -rf "${DB_DIR}" && mkdir -p "${DB_DIR}" && ` +
      `printf '%s\\n' '${E2E_ADMIN_PASSWORD}' | ` +
      `../backend/target/release/server admin create --email '${E2E_ADMIN_EMAIL}' --name '${E2E_ADMIN_NAME}' --password-stdin && ` +
      `exec ../backend/target/release/server`,
    url: `${BASE_URL}/healthz`,
    reuseExistingServer: false,
    timeout: 60_000,
    stdout: "ignore",
    stderr: "pipe",
    env: {
      PORT: String(PORT),
      APP_ENV: "development",
      DEV_LOGIN: "true",
      OIDC_ISSUER_URL: "",
      OIDC_CLIENT_ID: "",
      OIDC_CLIENT_SECRET: "",
      OIDC_ADDITIONAL_AUDIENCES: "",
      ADMIN_EMAIL: E2E_ADMIN_EMAIL,
      ADMIN_PASSWORD: "",
      PASSWORD_LOGIN_ENABLED: "true",
      AI_DEMO: "true",
      // A real key would win over AI_DEMO; blank them so neither the shell nor
      // a repo-root .env (dotenvy never overrides set vars) can reach a live model.
      AI_API_KEY: "",
      ANTHROPIC_API_KEY: "",
      APP_BASE_URL: BASE_URL,
      ALLOWED_ORIGINS: BASE_URL,
      PUBLIC_RATE_LIMIT_BURST: "1000",
      DATABASE_URL: `sqlite://${DB_DIR}/e2e.db?mode=rwc`,
    },
  },
});
