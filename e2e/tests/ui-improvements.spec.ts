import { expect, test } from "@playwright/test";
import { createGroupOrder, createRoster, createVendor, devLogin, expectNoHorizontalOverflow, getPublicGroup, submitOrder } from "../helpers";

for (const width of [320, 390, 768, 1280]) {
  test(`UI regression at ${width}px: dialog, draft, snapshot and statistics`, async ({ page }, info) => {
    await page.setViewportSize({ width, height: 844 });
    await devLogin(page, `ui-${width}`);
    const names = Array.from({ length: 45 }, (_, i) => `參與者${String(i).padStart(2, "0")}這是需要完整閱讀的長姓名`);
    const last = names.at(-1)!;
    const meal = "超長品名香煎豪華雙主餐季節時蔬便當附湯以及飲料";
    await createRoster(page, "非常長的部門名單名稱供開團前確認使用", names);
    await createVendor(page, "非常長的店家名稱供核對使用", [{ name: meal, price: 100000 }]);
    await page.goto("/group-orders/new");
    await page.getByRole("button", { name: /非常長的部門名單/ }).focus();
    await page.keyboard.press("Enter");
    await page.getByRole("button", { name: "下一步" }).focus();
    await page.keyboard.press("Enter");
    await page.getByRole("button", { name: /非常長的店家/ }).focus();
    await page.keyboard.press("Enter");
    await page.getByRole("button", { name: "下一步" }).focus();
    await page.keyboard.press("Enter");
    await expect(page.getByRole("region", { name: "開團選擇摘要" })).toContainText("45 人");
    await page.getByLabel("標題", { exact: true }).fill(`UI ${width}`);
    await page.getByLabel("備註（選填）", { exact: true }).fill("第一行備註\n第二行備註");
    await expectNoHorizontalOverflow(page);
    await page.screenshot({ path: info.outputPath("wizard-summary.png"), fullPage: true });
    await page.getByRole("button", { name: "開團", exact: true }).click();
    await expect(page).toHaveURL(/group-orders\/[0-9a-f-]+$/);
    const managementUrl = page.url();
    const groupId = managementUrl.split("/").at(-1)!;
    const shareUrl = await page.locator("input[readonly]").first().inputValue();
    const pub = await getPublicGroup(page, shareUrl);
    const pid = pub.participants.find((p) => p.name === last)!.id;
    await submitOrder(page, shareUrl, pid, [{ itemId: pub.menu[0].id, quantity: 20 }], `device-${width}`);

    await page.goto(shareUrl);
    const origin = page.getByRole("button", { name: last });
    await origin.scrollIntoViewIfNeeded();
    await origin.click();
    const dialog = page.getByRole("dialog", { name: "確認身分" });
    await expect(dialog).toBeVisible();
    await expect(dialog.getByRole("button", { name: "選錯了" })).toBeFocused();
    const box = await dialog.boundingBox();
    expect(box!.y).toBeGreaterThanOrEqual(0);
    expect(box!.y + box!.height).toBeLessThanOrEqual(844);
    for (let n = 0; n < 8; n++) {
      await page.keyboard.press("Tab");
      await expect.poll(() => dialog.evaluate((el) => el.contains(document.activeElement))).toBe(true);
    }
    await page.screenshot({ path: info.outputPath("identity-dialog.png") });
    await page.keyboard.press("Escape");
    await expect(origin).toBeFocused();
    await expect(origin).toBeInViewport();
    await origin.click();
    await dialog.getByRole("button", { name: "是我", exact: true }).click();
    await expect(page.getByRole("heading", { name: last })).toBeFocused();
    await page.getByRole("button", { name: `減少 ${meal}` }).focus();
    await page.keyboard.press("Enter");
    await page.getByLabel("訂單備註（選填）").fill("尚未送出的第一行\n尚未送出的第二行");
    await expect(page.getByRole("status")).toHaveText("有尚未送出的修改");
    await expectNoHorizontalOverflow(page);
    await page.screenshot({ path: info.outputPath("public-dirty.png"), fullPage: true });

    // Server closes while this client still has an editable draft: exercise real 409 handling.
    const close = await page.request.patch(`/api/v1/group-orders/${groupId}`, { data: { status: "closed" } });
    expect(close.ok()).toBe(true);
    await page.getByRole("button", { name: "儲存修改" }).click();
    const saved = page.getByRole("region", { name: "已送出訂單" });
    await expect(saved).toContainText(`${meal} ×20`);
    await expect(saved).toContainText("總額 $2,000,000");
    await expect(page.getByText("尚未送出的第一行", { exact: true })).toHaveCount(0);
    await expect(page.getByRole("textbox")).toHaveCount(0);
    await expectNoHorizontalOverflow(page);
    await page.screenshot({ path: info.outputPath("readonly-snapshot.png"), fullPage: true });

    await page.goto(managementUrl);
    await expect(page.getByText("分享與訂購連結", { exact: true })).toBeVisible();
    const list = page.getByRole("list", { name: "品項統計" });
    await expect(list).toContainText(meal);
    await expect(list).toContainText("$2,000,000");
    await expectNoHorizontalOverflow(page);
    const row = list.getByRole("listitem");
    const nameBox = await row.getByText(meal, { exact: true }).boundingBox();
    const amountBox = await row.getByText("$2,000,000", { exact: true }).boundingBox();
    expect(nameBox!.x + nameBox!.width).toBeLessThanOrEqual(amountBox!.x);
    expect(await row.locator("label").evaluate((el) => el.getBoundingClientRect().height)).toBeGreaterThanOrEqual(44);
    await page.screenshot({ path: info.outputPath("statistics.png"), fullPage: true });
    await page.getByRole("tab", { name: "領取＆繳費" }).click();
    const figures = page.getByText("已收 $0 / 應收 $2,000,000");
    await expect(figures).toBeVisible();
    expect(await figures.evaluate((el) => !!(el.compareDocumentPosition(document.querySelector('[aria-label="篩選"]')!) & Node.DOCUMENT_POSITION_FOLLOWING))).toBe(true);
    await expectNoHorizontalOverflow(page);
    await page.getByRole("tab", { name: "統計" }).focus();
    await page.keyboard.press("Enter");
    await expect(page.getByRole("tab", { name: "統計" })).toHaveAttribute("aria-selected", "true");
  });
}

test("deadline timer discards draft and performs a fresh persisted-order read", async ({ page }) => {
  test.setTimeout(60000);
  await devLogin(page, "ui-deadline");
  const roster = await createRoster(page, "截止測試", ["截止測試者"]);
  const vendor = await createVendor(page, "截止店家", [{ name: "快照餐點", price: 123 }]);
  const group = await createGroupOrder(page, "自動截止", roster.id, vendor.id, { deadlineAt: new Date(Date.now() + 12000).toISOString() });
  const pub = await getPublicGroup(page, group.shareUrl);
  await submitOrder(page, group.shareUrl, pub.participants[0].id, [{ itemId: pub.menu[0].id, quantity: 1 }], "deadline-device");
  await page.goto(group.shareUrl);
  await page.getByRole("button", { name: /截止測試者/ }).click();
  await page.getByRole("dialog").getByRole("button", { name: "是我", exact: true }).click();
  await page.getByRole("button", { name: "增加 快照餐點" }).click();
  await expect(page.getByRole("status")).toHaveText("有尚未送出的修改");
  let freshReads = 0;
  page.on("request", (request) => { if (request.url().endsWith(`/participants/${pub.participants[0].id}/order`)) freshReads++; });
  const saved = page.getByRole("region", { name: "已送出訂單" });
  await expect(saved).toContainText("快照餐點 ×1", { timeout: 40000 });
  await expect(saved).toContainText("總額 $123");
  expect(freshReads).toBeGreaterThan(0);
  await expect(page.getByRole("button", { name: "儲存修改" })).toHaveCount(0);
  expect(await page.evaluate(() => document.documentElement.style.getPropertyValue("--order-bar-h"))).toBe("");
});
