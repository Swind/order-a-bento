import { expect, test } from "@playwright/test";
import { E2E_ADMIN_EMAIL, E2E_ADMIN_NAME, E2E_ADMIN_PASSWORD } from "../admin-account";
import { expectNoHorizontalOverflow } from "../helpers";

// The server runs without OIDC; the admin account was created with
// `server admin create --password-stdin` against the same database before startup.

test("CLI-created admin logs in with email and password, without Google", async ({ page }) => {
  await page.goto("/rosters");
  await expect(page).toHaveURL(/\/login\?returnTo=%2Frosters/);

  // Only the enabled methods are offered: no Google button, password form present.
  await expect(page.getByLabel("密碼")).toBeVisible();
  await expect(page.getByRole("button", { name: "使用單一登入（OIDC）" })).toHaveCount(0);
  await expect(page.getByLabel("密碼")).toHaveAttribute("autocomplete", "current-password");
  await expectNoHorizontalOverflow(page);

  const cfg = await (await page.request.get("/api/v1/auth/config")).json();
  expect(cfg).toMatchObject({ oidcEnabled: false, passwordLoginEnabled: true });
  const google = await page.request.get("/auth/oidc/login", { maxRedirects: 0 });
  expect(google.status()).toBe(503);

  // Wrong password: the generic message, still logged out.
  await page.fill("#login-email", E2E_ADMIN_EMAIL);
  await page.fill("#login-password", "not-the-password");
  await page.locator("form[aria-label='本機帳號登入'] button[type=submit]").click();
  await expect(page.getByRole("alert")).toContainText("Email 或密碼不正確");
  await expect(page).toHaveURL(/\/login/);

  // Correct password (email is case-insensitive): lands on the page we were sent from.
  await page.fill("#login-email", E2E_ADMIN_EMAIL.toUpperCase());
  await page.fill("#login-password", E2E_ADMIN_PASSWORD);
  await page.locator("form[aria-label='本機帳號登入'] button[type=submit]").click();
  await expect(page).toHaveURL(/\/rosters$/);

  const me = await (await page.request.get("/api/v1/me")).json();
  expect(me).toMatchObject({ email: E2E_ADMIN_EMAIL, displayName: E2E_ADMIN_NAME });

  // The admin area works for this account.
  await page.getByRole("button", { name: /新增名單/ }).first().click();
  await page.fill("#roster-name", "密碼登入測試名單");
  await page.fill("#roster-members", "甲\n乙");
  await page.getByRole("dialog").getByRole("button", { name: /建立|新增|儲存/ }).click();
  await expect(page.getByText("密碼登入測試名單").first()).toBeVisible();
  await expectNoHorizontalOverflow(page);
});
