import { expect, test, type Locator, type Page } from "@playwright/test";
import { createGroupOrder, createRoster, createVendor, devLogin, expectNoHorizontalOverflow, getPublicGroup, submitOrder } from "../helpers";

async function expectTouchTarget(locator: Locator, height = 44, width?: number) {
  // Dialog opening transitions briefly scale the rect; verify the settled target without reducing the minimum.
  await expect.poll(async () => (await locator.boundingBox())?.height ?? 0).toBeGreaterThanOrEqual(height);
  if (width) await expect.poll(async () => (await locator.boundingBox())?.width ?? 0).toBeGreaterThanOrEqual(width);
}

async function expectFixedBar(page: Page) {
  const bar = page.getByRole("region", { name: "訂單送出操作" });
  await expect(bar).toBeVisible();
  await expect(bar.getByRole("button")).toHaveCount(1);
  await expect(bar.getByRole("button", { name: /取消/ })).toHaveCount(0);
  await expectTouchTarget(bar.getByRole("button"), 48);
  await expect.poll(async () => {
    const height = await bar.evaluate((el) => el.getBoundingClientRect().height);
    const variable = await page.evaluate(() => parseFloat(document.documentElement.style.getPropertyValue("--order-bar-h")));
    return Math.abs(height - variable);
  }).toBeLessThanOrEqual(1);
  await page.evaluate(() => window.scrollTo(0, document.body.scrollHeight));
  const privacy = await page.getByText("為核對訂單，系統會記錄 IP 與裝置資訊").boundingBox();
  const box = await bar.boundingBox();
  expect(privacy).not.toBeNull();
  expect(privacy!.y + privacy!.height).toBeLessThanOrEqual(box!.y);
  expect(box!.y + box!.height).toBeCloseTo(page.viewportSize()!.height, 0);
  return box!.height;
}

for (const width of [320, 390, 768, 1280]) {
  test(`UI regression at ${width}px: dialog, draft, snapshot and statistics`, async ({ page }, info) => {
    await page.setViewportSize({ width, height: 844 });
    await devLogin(page, `ui-${width}`);
    const names = Array.from({ length: 45 }, (_, i) => `參與者${String(i).padStart(2, "0")}這是需要完整閱讀的長姓名`);
    const last = names.at(-1)!;
    const meal = "超長品名香煎豪華雙主餐季節時蔬便當附湯以及飲料";
    await createRoster(page, "非常長的部門名單名稱供開團前確認使用", names);
    await createVendor(page, "非常長的店家名稱供核對使用", [{ name: meal, price: 100000 }]);
    const addressResponse = await page.request.post("/api/v1/delivery-addresses", { data: { name: "完整長送餐地址", address: "台北市中正區非常長的道路名稱一百二十三號十二樓產品團隊會議室" } });
    expect(addressResponse.ok()).toBe(true);
    const address = await addressResponse.json();
    let addressesFail = true;
    await page.route("**/api/v1/delivery-addresses", (route) => addressesFail
      ? route.fulfill({ status: 500, contentType: "application/json", body: JSON.stringify({ error: { code: "internal_error", message: "temporary failure" } }) })
      : route.continue());
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
    const ownership = page.getByLabel("開團歸屬", { exact: true });
    await expectTouchTarget(ownership);
    expect(await ownership.evaluate((el) => [el.tagName, getComputedStyle(el).fontSize])).toEqual(["SELECT", "16px"]);
    const delivery = page.getByLabel("送餐地址（選填）", { exact: true });
    await expect(delivery).toBeDisabled();
    const retryAddress = page.getByRole("button", { name: "重試載入地址", exact: true });
    await expectTouchTarget(retryAddress);
    await expectTouchTarget(delivery);
    addressesFail = false;
    await retryAddress.click();
    await expect(delivery).toBeEnabled();
    await delivery.selectOption(address.id);
    await expect(delivery).toHaveValue(address.id);
    await delivery.focus();
    expect(await delivery.evaluate((el) => [el.tagName, getComputedStyle(el).fontSize, getComputedStyle(el).boxShadow !== "none"])).toEqual(["SELECT", "16px", true]);
    await page.unroute("**/api/v1/delivery-addresses");
    await page.getByLabel("標題", { exact: true }).fill(`UI ${width} 這是必須完整閱讀並且允許換行的團隊午餐長標題`);
    await page.getByLabel("備註（選填）", { exact: true }).fill("第一行備註\n第二行備註");
    await expectNoHorizontalOverflow(page);
    await page.screenshot({ path: info.outputPath("wizard-summary.png"), fullPage: true });
    await page.getByRole("button", { name: "開團", exact: true }).click();
    await expect(page).toHaveURL(/group-orders\/[0-9a-f-]+$/);
    const managementUrl = page.url();
    const groupId = managementUrl.split("/").at(-1)!;
    const secondary = page.getByText("連結與其他操作", { exact: true });
    await expect(page.getByRole("link", { name: "開啟訂購頁" })).toBeHidden();
    await expect(page.getByLabel("分享連結", { exact: true })).toBeHidden();
    await expectTouchTarget(secondary);
    await expectTouchTarget(page.getByRole("button", { name: "複製連結", exact: true }));
    await secondary.focus();
    await page.keyboard.press("Enter");
    const shareInput = page.getByLabel("分享連結", { exact: true });
    await expect(shareInput).toBeVisible();
    await expectTouchTarget(shareInput);
    const shareUrl = await shareInput.inputValue();
    await expect(page.getByRole("link", { name: "開啟訂購頁" })).toHaveAttribute("href", shareUrl);
    await secondary.click();
    const pub = await getPublicGroup(page, shareUrl);
    const pid = pub.participants.find((p) => p.name === last)!.id;
    await submitOrder(page, shareUrl, pid, [{ itemId: pub.menu[0].id, quantity: 20 }], `device-${width}`);

    await page.reload();
    const summary = page.locator("dl").filter({ has: page.locator("dt", { hasText: "已訂人數" }) });
    await expect(summary).toContainText("1 人");
    await expect(summary).toContainText("未訂 44 人");
    await expect(summary).toContainText("20 份");
    await expect(summary).toContainText("$2,000,000");
    const largeAmountRow = page.getByRole("list", { name: "品項統計" }).getByRole("listitem");
    const longNameBox = await largeAmountRow.getByText(meal, { exact: true }).boundingBox();
    const largeAmountBox = await largeAmountRow.getByText("$2,000,000", { exact: true }).boundingBox();
    expect(longNameBox).not.toBeNull();
    expect(largeAmountBox).not.toBeNull();
    expect(longNameBox!.x + longNameBox!.width).toBeLessThanOrEqual(largeAmountBox!.x);
    const groups = summary.locator("div").filter({ has: page.locator(":scope > dt") });
    const people = await groups.filter({ hasText: "已訂人數" }).boundingBox();
    const quantity = await groups.filter({ hasText: "總份數" }).boundingBox();
    const amount = await groups.filter({ hasText: "總金額" }).boundingBox();
    expect(people).not.toBeNull();
    expect(quantity).not.toBeNull();
    expect(amount).not.toBeNull();
    expect(people!.y).toBeCloseTo(quantity!.y, 0);
    if (width < 640) {
      expect(amount!.y).toBeGreaterThanOrEqual(people!.y + people!.height);
      expect(amount!.width).toBeGreaterThan(quantity!.width);
    } else expect(amount!.y).toBeCloseTo(people!.y, 0);
    await expectNoHorizontalOverflow(page);
    await page.screenshot({ path: info.outputPath("management-collecting.png"), fullPage: true });
    await summary.scrollIntoViewIfNeeded();
    await page.screenshot({ path: info.outputPath("management-summary.png") });
    await page.getByRole("tab", { name: "明細", exact: true }).click();
    await expect(summary).toHaveCount(0);
    await page.getByRole("tab", { name: "統計", exact: true }).click();

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
    const cancel = page.getByRole("button", { name: "取消我的訂單", exact: true });
    const savedSummary = page.getByRole("region", { name: "上次送出的訂單" });
    await expect(savedSummary).toContainText(`${meal} ×20`);
    await expect(savedSummary.getByRole("button", { name: "取消我的訂單", exact: true })).toBeVisible();
    await expectTouchTarget(cancel);
    await expectTouchTarget(page.getByRole("button", { name: `減少 ${meal}` }), 44, 44);
    const barHeight = await expectFixedBar(page);
    await info.attach("bar-height-after", { body: JSON.stringify({ width, height: barHeight, beforeHeight: 161, reduction: 161 - barHeight }), contentType: "application/json" });
    console.log(`${info.project.name} ${width}px fixed bar: ${barHeight}px (before 161px)`);
    await expectNoHorizontalOverflow(page);
    await page.screenshot({ path: info.outputPath("public-dirty.png"), fullPage: true });
    await page.screenshot({ path: info.outputPath("public-dirty-fixed-bar.png") });

    await cancel.click();
    const cancelDialog = page.getByRole("dialog", { name: "確認取消訂單" });
    const keep = cancelDialog.getByRole("button", { name: "保留", exact: true });
    const confirmCancel = cancelDialog.getByRole("button", { name: "確定取消", exact: true });
    await expect(keep).toBeFocused();
    await expect(cancelDialog).toContainText("尚未送出的修改也會在取消成功後清除");
    for (let n = 0; n < 5; n++) {
      await page.keyboard.press(n % 2 ? "Shift+Tab" : "Tab");
      await expect.poll(() => cancelDialog.evaluate((el) => el.contains(document.activeElement))).toBe(true);
    }
    await expectTouchTarget(keep);
    await expectTouchTarget(confirmCancel);
    await expectNoHorizontalOverflow(page);
    await page.screenshot({ path: info.outputPath("cancel-confirm.png") });
    await page.keyboard.press("Escape");
    await expect(cancelDialog).toBeHidden();
    await expect(cancel).toBeFocused();
    await expect(page.getByLabel("訂單備註（選填）")).toHaveValue("尚未送出的第一行\n尚未送出的第二行");
    await cancel.click();
    await keep.click();
    await expect(cancel).toBeFocused();

    // Keep the real backend response pending to verify busy keyboard behavior and single submission.
    const endpoint = `**/participants/${pid}/submissions`;
    let release!: () => void;
    const pending = new Promise<void>((resolve) => { release = resolve; });
    let requests = 0;
    await page.route(endpoint, async (route) => {
      requests++;
      const payload = route.request().postDataJSON();
      expect(payload.lines).toEqual([]);
      expect(payload).not.toHaveProperty("note");
      await pending;
      await route.continue();
    });
    await cancel.click();
    await confirmCancel.click();
    await expect.poll(() => requests).toBe(1);
    await expect(keep).toBeDisabled();
    await expect(cancelDialog.getByRole("button", { name: /取消中|確定取消/ })).toBeDisabled();
    await page.keyboard.press("Escape");
    await expect(cancelDialog).toBeVisible();
    await page.mouse.click(1, 1);
    await expect(cancelDialog).toBeVisible();
    release();
    await expect(cancelDialog).toBeHidden();
    await page.unroute(endpoint);
    expect(requests).toBe(1);
    await expect(cancel).toHaveCount(0);
    await expect(page.getByRole("status")).toBeFocused();
    await expect(page.getByLabel("訂單備註（選填）")).toHaveValue("");
    await expect(page.getByRole("button", { name: "送出訂單", exact: true })).toBeDisabled();
    expect(await expectFixedBar(page)).toBeCloseTo(barHeight, 0);
    await page.screenshot({ path: info.outputPath("public-cancelled.png"), fullPage: true });

    await page.getByRole("button", { name: `增加 ${meal}` }).click();
    await page.getByRole("button", { name: "送出訂單", exact: true }).click();
    await expect(page.getByRole("button", { name: "訂單已送出", exact: true })).toBeDisabled();
    await page.getByRole("button", { name: `增加 ${meal}` }).click();

    // Server closes while this client still has an editable draft: exercise real 409 handling.
    const close = await page.request.patch(`/api/v1/group-orders/${groupId}`, { data: { status: "closed" } });
    expect(close.ok()).toBe(true);
    await page.getByRole("button", { name: "儲存修改" }).click();
    const saved = page.getByRole("region", { name: "已送出訂單" });
    await expect(saved).toContainText(`${meal} ×1`);
    await expect(saved).toContainText("總額 $100,000");
    await expect(page.getByText("尚未送出的第一行", { exact: true })).toHaveCount(0);
    await expect(page.getByRole("textbox")).toHaveCount(0);
    await expect(page.getByRole("region", { name: "訂單送出操作" })).toHaveCount(0);
    await expect(cancel).toHaveCount(0);
    expect(await page.evaluate(() => document.documentElement.style.getPropertyValue("--order-bar-h"))).toBe("");
    await expectNoHorizontalOverflow(page);
    await page.screenshot({ path: info.outputPath("readonly-snapshot.png"), fullPage: true });

    await page.goto(managementUrl);
    await expect(page.getByText("分享與訂購連結", { exact: true })).toBeVisible();
    const list = page.getByRole("list", { name: "品項統計" });
    await expect(list).toContainText(meal);
    await expect(list).toContainText("$100,000");
    await expectNoHorizontalOverflow(page);
    const row = list.getByRole("listitem");
    const nameBox = await row.getByText(meal, { exact: true }).boundingBox();
    const amountBox = await row.getByText("$100,000", { exact: true }).boundingBox();
    expect(nameBox!.x + nameBox!.width).toBeLessThanOrEqual(amountBox!.x);
    expect(await row.locator("label").evaluate((el) => el.getBoundingClientRect().height)).toBeGreaterThanOrEqual(44);
    await page.screenshot({ path: info.outputPath("statistics.png"), fullPage: true });
    await page.getByRole("tab", { name: "領取＆繳費" }).click();
    const figures = page.getByText("已收 $0 / 應收 $100,000");
    await expect(figures).toBeVisible();
    expect(await figures.evaluate((el) => !!(el.compareDocumentPosition(document.querySelector('[aria-label="篩選"]')!) & Node.DOCUMENT_POSITION_FOLLOWING))).toBe(true);
    await expectNoHorizontalOverflow(page);
    const tabUrl = page.url();
    const closedShare = page.getByText("分享與訂購連結", { exact: true });
    await closedShare.click();
    await page.getByText("連結與其他操作", { exact: true }).click();
    await expect(page.getByLabel("分享連結", { exact: true })).toHaveValue(shareUrl);
    expect(page.url()).toBe(tabUrl);
    expect(await closedShare.evaluate((el) => {
      const panel = document.querySelector('[role="tabpanel"]');
      return !!panel && !!(panel.compareDocumentPosition(el) & Node.DOCUMENT_POSITION_FOLLOWING);
    })).toBe(true);
    await page.getByRole("tab", { name: "統計" }).focus();
    await page.keyboard.press("Enter");
    await expect(page.getByRole("tab", { name: "統計" })).toHaveAttribute("aria-selected", "true");
  });
}

test("native directory scope select remains usable at all four widths", async ({ page }) => {
  await devLogin(page, "ui-directory-scope");
  const response = await page.request.post("/api/v1/sharing-groups", { data: { name: "需要完整閱讀的共用群組名稱供資料範圍選擇驗證" } });
  expect(response.ok()).toBe(true);
  const group = await response.json();
  for (const width of [320, 390, 768, 1280]) {
    await page.setViewportSize({ width, height: 844 });
    for (const url of ["/rosters", "/vendors"]) {
      await page.goto(url);
      const scope = page.getByLabel("資料範圍", { exact: true });
      await expectTouchTarget(scope);
      await expect(scope.locator(`option[value="${group.id}"]`)).toHaveCount(1);
      await scope.selectOption(group.id);
      await expect(scope).toHaveValue(group.id);
      await scope.focus();
      expect(await scope.evaluate((el) => [el.tagName, getComputedStyle(el).fontSize, getComputedStyle(el).boxShadow !== "none"])).toEqual(["SELECT", "16px", true]);
      await expectNoHorizontalOverflow(page);
      await scope.selectOption("");
      await expect(scope).toHaveValue("");
    }
  }
});

test("share fallback, zero stats and failed cancellation preserve an empty draft", async ({ page }) => {
  await devLogin(page, "ui-fallback-cancel");
  const roster = await createRoster(page, "失敗重試名單", ["重試訂餐者"]);
  const vendor = await createVendor(page, "失敗重試店家", [{ name: "重試餐點", price: 120 }]);
  const group = await createGroupOrder(page, "失敗重試", roster.id, vendor.id);
  await page.goto(`/group-orders/${group.id}`);
  const summary = page.locator("dl").filter({ has: page.locator("dt", { hasText: "已訂人數" }) });
  await expect(summary).toContainText("0 人");
  await expect(summary).toContainText("0 份");
  await expect(summary).toContainText("$0");
  await page.evaluate(() => {
    Object.defineProperty(navigator, "clipboard", {
      configurable: true,
      value: { writeText: async () => { throw new Error("clipboard unavailable in test"); } },
    });
  });
  await page.getByRole("button", { name: "複製連結", exact: true }).click();
  const input = page.getByLabel("分享連結", { exact: true });
  await expect(input).toBeVisible();
  await expect(input).toBeFocused();
  await expect(input).toHaveValue(group.shareUrl);
  expect(await input.evaluate((el: HTMLInputElement) => [el.selectionStart, el.selectionEnd])).toEqual([0, group.shareUrl.length]);

  await page.goto(group.shareUrl);
  await page.getByRole("button", { name: "重試訂餐者", exact: true }).click();
  await page.getByRole("button", { name: "增加 重試餐點" }).click();
  await page.getByRole("button", { name: "送出訂單", exact: true }).click();
  await expect(page.getByRole("button", { name: "訂單已送出" })).toBeDisabled();
  await page.getByRole("button", { name: "減少 重試餐點" }).click();
  await expect(page.getByRole("button", { name: "儲存修改" })).toBeDisabled();
  const cancel = page.getByRole("button", { name: "取消我的訂單", exact: true });
  await expect(cancel).toBeEnabled();
  await page.route("**/submissions", (route) => route.fulfill({ status: 500, contentType: "application/json", body: JSON.stringify({ error: "internal_error", message: "temporary failure" }) }));
  await cancel.click();
  const dialog = page.getByRole("dialog", { name: "確認取消訂單" });
  await dialog.getByRole("button", { name: "確定取消", exact: true }).click();
  await expect(dialog.getByRole("button", { name: "確定取消", exact: true })).toBeEnabled();
  await expect(dialog).toBeVisible();
  await dialog.getByRole("button", { name: "保留", exact: true }).click();
  await expect(cancel).toBeFocused();
  await expect(page.getByRole("region", { name: "上次送出的訂單" })).toContainText("重試餐點 ×1");
  await expect(page.getByRole("group", { name: "重試餐點 數量" })).toContainText("0");
  await expect(page.getByRole("button", { name: "儲存修改" })).toBeDisabled();
  await page.unroute("**/submissions");
  await cancel.click();
  await dialog.getByRole("button", { name: "確定取消", exact: true }).click();
  await expect(dialog).toBeHidden();
  await expect(cancel).toHaveCount(0);
  await expect(page.getByRole("status")).toBeFocused();
});

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
