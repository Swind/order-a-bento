import { expect, test } from "@playwright/test";
import { devLogin } from "../helpers";

test("toast does not cover the header", async ({ page }) => {
  await devLogin(page, "toast");
  await page.goto("/rosters");
  await page.getByRole("button", { name: /新增名單/ }).first().click();
  await page.fill("#roster-name", "測試");
  await page.getByRole("dialog").getByRole("button", { name: /建立|新增|儲存/ }).click();
  const toast = page.locator("[data-sonner-toast]").first();
  await expect(toast).toBeVisible();
  await page.waitForTimeout(800); // enter animation
  const tb = await toast.boundingBox();
  const hb = await page.locator("header").first().boundingBox();
  expect(tb!.y).toBeGreaterThanOrEqual(hb!.y + hb!.height);
});
