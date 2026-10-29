import { expect, test, type Locator } from "@playwright/test";
import path from "node:path";
import fs from "node:fs";
import { createVendor, devLogin, expectNoHorizontalOverflow, writeTestPng } from "../helpers";

async function settleAnimations(locator: Locator) {
  await expect(locator).toBeVisible();
  await locator.evaluate(async (element) => {
    await new Promise<void>((resolve) => requestAnimationFrame(() => requestAnimationFrame(() => resolve())));
    await Promise.all(element.getAnimations({ subtree: true }).map((animation) => animation.finished));
    await new Promise<void>((resolve) => requestAnimationFrame(() => requestAnimationFrame(() => resolve())));
  });
}

async function expectTouchTargets(container: Locator, label: string) {
  await settleAnimations(container);
  const buttons = container.getByRole("button");
  expect(await buttons.count()).toBeGreaterThan(0);
  const measurements = [];
  for (const button of await buttons.all()) {
    await expect(button).toBeVisible();
    const box = await button.boundingBox();
    expect(box).not.toBeNull();
    expect(box!.height, `${label}: ${await button.getAttribute("aria-label")} height`).toBeGreaterThanOrEqual(44);
    expect(box!.width, `${label}: button width`).toBeGreaterThanOrEqual(44);
    measurements.push({ name: await button.getAttribute("aria-label") ?? await button.innerText(), ...box });
  }
  await test.info().attach(`${label}-touch-targets`, { body: JSON.stringify(measurements, null, 2), contentType: "application/json" });
}

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

test("vendor reference photo upload, preview, replace and delete preserve unsaved info", async ({ page }) => {
  await devLogin(page, "vendor-photo");
  const vendor = await createVendor(page, "參考照片店家", [{ name: "便當", price: 100 }]);
  await page.goto(`/vendors/${vendor.id}`);
  const section = page.getByRole("region", { name: "店家參考照片" });
  const image = section.getByAltText("店家參考照片", { exact: true });
  await expectTouchTargets(section, "empty");
  const file = path.resolve("../eval/cases/03-vendor-flyer/image.png");
  await page.getByLabel("電話", { exact: true }).fill("未儲存草稿");
  await section.getByLabel("選擇參考照片檔案").setInputFiles(file);
  await expect(section.getByAltText("待上傳照片", { exact: true })).toBeVisible();
  await expectTouchTargets(section, "local-selection");
  expect((await (await page.request.get(`/api/v1/vendors/${vendor.id}`)).json()).image).toBeNull();
  await section.getByRole("button", { name: "取消上傳" }).click();
  await section.getByLabel("選擇參考照片檔案").setInputFiles(file);
  await section.getByRole("button", { name: "確認上傳照片" }).click();
  await expect(image).toBeVisible();
  await expect(page.getByLabel("電話", { exact: true })).toHaveValue("未儲存草稿");
  const original = await (await page.request.get(`/api/v1/vendors/${vendor.id}`)).json();
  expect(original.image).toMatchObject({ contentType: "image/jpeg", width: 1200 });
  expect(original.phone).toBeNull();
  await page.reload();
  await expect(image).toBeVisible();
  await expect(image).toHaveJSProperty("naturalWidth", 1200);
  await section.scrollIntoViewIfNeeded();
  await expectNoHorizontalOverflow(page);
  await expectTouchTargets(section, "saved");
  await page.screenshot({ path: test.info().outputPath("vendor-image-saved.png"), fullPage: true });
  await section.getByRole("button", { name: "參考照片預覽", exact: true }).click();
  await expect(page.getByRole("dialog").getByAltText("參考照片預覽")).toBeVisible();
  await expectTouchTargets(page.getByRole("dialog"), "preview-close");
  await page.screenshot({ path: test.info().outputPath("vendor-image-preview.png"), fullPage: false });
  await page.keyboard.press("Escape");
  await expect(page.getByRole("dialog")).toHaveCount(0);
  await expect(section.getByRole("button", { name: "參考照片預覽", exact: true })).toBeFocused();
  await page.getByLabel("電話", { exact: true }).fill("替換刪除仍保留");
  const replacement = writeTestPng(1800, 600);
  expect(fs.statSync(replacement).size).toBeLessThanOrEqual(5 * 1024 * 1024);
  await section.getByLabel("選擇參考照片檔案").setInputFiles(replacement);
  await section.getByRole("button", { name: "確認替換照片" }).click();
  await expect(image).toHaveJSProperty("naturalWidth", 1568);
  await expect(page.getByLabel("電話", { exact: true })).toHaveValue("替換刪除仍保留");
  const replaced = await (await page.request.get(`/api/v1/vendors/${vendor.id}`)).json();
  expect(replaced.image.revision).not.toBe(original.image.revision);
  await section.getByRole("button", { name: "刪除照片", exact: true }).click();
  await expectTouchTargets(section, "delete-confirmation");
  await section.getByRole("group", { name: "確認刪除" }).getByRole("button", { name: "取消" }).click();
  await expect(image).toBeVisible();
  await section.getByRole("button", { name: "刪除照片", exact: true }).click();
  await section.getByRole("group", { name: "確認刪除" }).getByRole("button", { name: "確定", exact: true }).click();
  await expect(section.getByText("尚未上傳參考照片")).toBeVisible();
  await expect(page.getByLabel("電話", { exact: true })).toHaveValue("替換刪除仍保留");
  await page.reload();
  await expect(section.getByText("尚未上傳參考照片")).toBeVisible();
  expect((await page.request.get(`/api/v1/vendors/${vendor.id}/image`)).status()).toBe(404);
});

test("vendor image tiny preview and retry keep 44px touch targets", async ({ page }) => {
  await devLogin(page, "vendor-tiny-photo");
  const vendor = await createVendor(page, "極小參考照片店家", []);
  await page.goto(`/vendors/${vendor.id}`);
  const section = page.getByRole("region", { name: "店家參考照片" });
  await section.getByLabel("選擇參考照片檔案").setInputFiles(writeTestPng(1, 1));
  await expect(section.getByAltText("待上傳照片", { exact: true })).toHaveJSProperty("naturalWidth", 1);
  await expectTouchTargets(section, "tiny-local");
  await section.getByRole("button", { name: "待上傳照片預覽", exact: true }).click();
  await expectTouchTargets(page.getByRole("dialog"), "tiny-local-close");
  await page.getByRole("dialog").getByRole("button", { name: "Close", exact: true }).click();
  await expect(page.getByRole("dialog")).toHaveCount(0);
  await section.getByRole("button", { name: "確認上傳照片" }).click();
  await expect(section.getByAltText("店家參考照片", { exact: true })).toHaveJSProperty("naturalWidth", 1);
  await page.route(`**/api/v1/vendors/${vendor.id}/image`, async (route) => {
    await route.fulfill({ status: 503, contentType: "application/problem+json", body: JSON.stringify({ code: "internal_error", detail: "E2E retry fixture" }) });
  }, { times: 1 });
  await page.reload();
  await expect(section.getByRole("button", { name: "重新載入照片" })).toBeVisible();
  await expectTouchTargets(section, "load-retry");
  await section.getByRole("button", { name: "重新載入照片" }).click();
  await expect(section.getByAltText("店家參考照片", { exact: true })).toHaveJSProperty("naturalWidth", 1);
  await expectTouchTargets(section, "tiny-saved");
  await section.getByRole("button", { name: "參考照片預覽", exact: true }).click();
  await expectTouchTargets(page.getByRole("dialog"), "tiny-saved-close");
  await page.screenshot({ path: test.info().outputPath("vendor-image-tiny-preview.png"), fullPage: false });
  await page.getByRole("dialog").getByRole("button", { name: "Close", exact: true }).click();
  await expect(page.getByRole("dialog")).toHaveCount(0);
});
