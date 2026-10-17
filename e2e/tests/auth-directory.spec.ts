import { expect, test } from "@playwright/test";
import { devLogin, expectNoHorizontalOverflow } from "../helpers";

test("login, rosters and vendors directory", async ({ page }) => {
  await page.goto("/rosters");
  await expect(page).toHaveURL(/\/login\?returnTo=%2Frosters/);
  await expectNoHorizontalOverflow(page);

  await devLogin(page, "directory");

  // Roster: create via UI
  await page.goto("/rosters");
  await page.getByRole("button", { name: /新增名單/ }).first().click();
  await page.fill("#roster-name", "業務部");
  await page.fill("#roster-members", "Amy 林, 分機 12\n王小明");
  await page.getByRole("dialog").getByRole("button", { name: /建立|新增|儲存/ }).click();
  await expect(page.getByText("業務部").first()).toBeVisible();
  await expectNoHorizontalOverflow(page);

  await page.getByText("業務部").first().click();
  await expect(page).toHaveURL(/\/rosters\/[0-9a-f-]+$/);

  // Batch add: full-width name merges into the existing 「Amy 林」
  await page.fill("#batch-members", "ＡＭＹ　林，業務\n王小明 \n陳美麗");
  await page.locator("form:has(#batch-members) button[type=submit]").click();
  await expect(page.getByText("新增 1 位、更新 1 位、未變更 1 位")).toBeVisible();
  await expect(page.getByText("陳美麗").first()).toBeVisible();
  await expectNoHorizontalOverflow(page);

  // Vendor: create, menu batch
  await page.goto("/vendors");
  await page.getByRole("button", { name: /新增店家/ }).first().click();
  const dlg = page.getByRole("dialog");
  await dlg.getByLabel(/店名|名稱/).first().fill("好吃便當");
  await dlg.getByRole("button", { name: /建立|新增|儲存/ }).click();
  await expect(page).toHaveURL(/\/vendors\/[0-9a-f-]+$/);

  await page.locator("textarea").last().fill("排骨飯 $100\n雞腿飯 110元\n滷肉飯,60,小碗");
  await page.locator("form:has(textarea) button[type=submit]").last().click();
  await expect(page.getByText("滷肉飯").first()).toBeVisible();
  await expect(page.getByText("排骨飯").first()).toBeVisible();
  await expect(page.getByText("雞腿飯").first()).toBeVisible();
  await expect(page.getByText(/\$\s?100\b/).first()).toBeVisible();
  await expect(page.getByText(/\$\s?110\b/).first()).toBeVisible();
  await expect(page.getByText(/\$\s?60\b/).first()).toBeVisible();
  await expectNoHorizontalOverflow(page);

  // Duplicate vendor name
  await page.goto("/vendors");
  await page.getByRole("button", { name: /新增店家/ }).first().click();
  const dlg2 = page.getByRole("dialog");
  await dlg2.getByLabel(/店名|名稱/).first().fill("好吃便當 ");
  await dlg2.getByRole("button", { name: /建立|新增|儲存/ }).click();
  await expect(dlg2.getByText("已有同名店家")).toBeVisible();
  await expect(dlg2.getByRole("link").first()).toBeVisible();
});
