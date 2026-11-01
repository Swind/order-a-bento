import { expect, test } from "@playwright/test";
import { createGroupOrder, createRoster, createVendor, devLogin, expectNoHorizontalOverflow, getPublicGroup, submitOrder } from "../helpers";

test("wizard, participant ordering, impersonation, cancel", async ({ page, browser }) => {
  await devLogin(page, "ordering");
  await createRoster(page, "業務部", ["王小明", "李大華", "Amy 林"]);
  await createVendor(page, "好吃便當", [
    { name: "排骨飯", price: 100 },
    { name: "雞腿飯", price: 110 },
    { name: "素食便當", price: 90 },
  ]);

  // Wizard
  await page.goto("/group-orders/new");
  await page.getByRole("button", { name: /業務部/ }).click();
  await page.getByRole("button", { name: "下一步" }).click();
  await page.getByRole("button", { name: /好吃便當/ }).click();
  await page.getByRole("button", { name: "下一步" }).click();
  await page.fill("#go-title", "週五午餐");
  await expect(page.getByRole("region", { name: "開團選擇摘要" })).toContainText("名單：業務部 · 3 人");
  await expect(page.getByRole("region", { name: "開團選擇摘要" })).toContainText("店家：好吃便當");
  await expect(page.getByRole("region", { name: "開團選擇摘要" })).toContainText("開團歸屬：個人");
  await page.getByRole("button", { name: "開團", exact: true }).click();
  await expect(page).toHaveURL(/group-orders\/[0-9a-f-]+$/);
  await page.getByText("連結與其他操作", { exact: true }).click();
  const shareUrl = await page.getByLabel("分享連結", { exact: true }).inputValue();
  expect(shareUrl).toMatch(/\/g\/[A-Za-z0-9_-]{43}$/);
  await expectNoHorizontalOverflow(page);

  const html = await (await page.request.get(shareUrl)).text();
  expect(html).toContain("og:title");

  // The two participants use separate browser contexts (separate devices) with the same phone settings.
  const mk = async () => {
    const ctx = await browser.newContext({ viewport: page.viewportSize()! });
    return { ctx, p: await ctx.newPage() };
  };

  // Participant A
  const A = await mk();
  const a = A.p;
  await a.goto(shareUrl);
  await a.getByLabel("搜尋名字").fill("王");
  await a.getByRole("button", { name: /王小明/ }).click();
  await a.getByRole("button", { name: "增加 排骨飯" }).click();
  await a.getByRole("button", { name: /送出訂單/ }).click();
  await expect(a.getByText(/已於 .* 送出/).first()).toBeVisible();

  // Revisit: remembered, prefilled
  await a.reload();
  await expect(a.getByRole("button", { name: "訂單已送出" })).toBeDisabled();
  await expect(a.getByRole("group", { name: "排骨飯 數量" })).toContainText("1");
  await a.getByRole("button", { name: "減少 排骨飯" }).click();
  await a.getByRole("button", { name: "增加 雞腿飯" }).click();
  await a.getByRole("button", { name: "增加 雞腿飯" }).click();
  await expect(a.getByRole("status")).toContainText("有尚未送出的修改");
  await a.getByRole("button", { name: "儲存修改" }).click();
  await expect(a.getByRole("button", { name: "訂單已送出" })).toBeDisabled();
  await expect(a.getByText(/\$220/).first()).toBeVisible();

  // Participant B picks the same person: impersonation confirm
  const B = await mk();
  const b = B.p;
  await b.goto(shareUrl);
  await b.getByRole("button", { name: /王小明/ }).click();
  const dlg = b.getByRole("dialog", { name: "確認身分" });
  await expect(dlg).toContainText("你確定是本人嗎");
  await dlg.getByRole("button", { name: /選錯了/ }).click();

  // Full-width search finds half-width name
  await b.getByLabel("搜尋名字").fill("ａｍｙ");
  await expect(b.getByRole("button", { name: /Amy 林/ })).toBeVisible();
  await b.getByLabel("搜尋名字").fill("");
  await b.getByRole("button", { name: /李大華/ }).click();
  await b.getByRole("button", { name: "增加 素食便當" }).click();
  await b.getByRole("button", { name: /送出訂單/ }).click();
  await expect(b.getByText(/已於 .* 送出/).first()).toBeVisible();

  // Privacy footer sits above the sticky bar
  await b.evaluate(() => window.scrollTo(0, document.body.scrollHeight));
  const foot = await b.getByText("為核對訂單，系統會記錄 IP 與裝置資訊").boundingBox();
  const barTop = await b
    .getByRole("button", { name: /訂單已送出|送出訂單/ })
    .first()
    .evaluate((el) => {
      let e: Element | null = el;
      while (e && getComputedStyle(e).position !== "fixed") e = e.parentElement;
      return e ? e.getBoundingClientRect().top : null;
    });
  expect(foot).not.toBeNull();
  expect(barTop).not.toBeNull();
  expect(foot!.y + foot!.height).toBeLessThanOrEqual(barTop!);

  // A cancels
  await a.getByRole("button", { name: /取消我的訂單/ }).click();
  const confirm = a.getByRole("dialog", { name: "確認取消訂單" });
  await expect(confirm).toBeVisible();
  await expect(confirm.getByRole("button", { name: "保留", exact: true })).toBeFocused();
  await confirm.getByRole("button", { name: "確定取消", exact: true }).click();
  await expect(confirm).toBeHidden();

  await expect
    .poll(async () => {
      const pub = await getPublicGroup(page, shareUrl);
      return Object.fromEntries(pub.participants.map((p) => [p.name, p.hasOrdered]));
    })
    .toEqual({ 王小明: false, 李大華: true, "Amy 林": false });

  // A draft is discarded at cutoff; only the last persisted order remains.
  await b.getByRole("button", { name: "增加 排骨飯" }).click();
  await page.getByRole("button", { name: "關閉收單" }).click();
  await b.reload();
  await expect(b.getByRole("region", { name: "已送出訂單", exact: true })).toContainText("素食便當 ×1");
  await expect(b.getByRole("region", { name: "已送出訂單", exact: true })).toContainText("總額 $90");
  await expect(b.getByRole("button", { name: "增加 排骨飯" })).toHaveCount(0);

  await A.ctx.close();
  await B.ctx.close();
});

test("overview fresh reads, private-field allowlist, cancellation and closed/archive reads", async ({ page }) => {
  await devLogin(page, "overview-lifecycle");
  const roster = await createRoster(page, "總覽生命週期", ["已訂者", "未訂者"]);
  const vendor = await createVendor(page, "總覽店", [{ name: "排骨飯", price: 100 }]);
  const group = await createGroupOrder(page, "總覽生命週期", roster.id, vendor.id);
  const pub = await getPublicGroup(page, group.shareUrl);
  const base = `/api/v1/public/g/${group.shareUrl.split("/g/")[1]}`;
  const submitted = await page.request.post(`${base}/participants/${pub.participants[0].id}/submissions`, {
    data: { lines: [{ itemId: pub.menu[0].id, quantity: 2, note: "秘密餐點備註" }], note: "秘密訂單備註",
      idempotencyKey: crypto.randomUUID(), client: { deviceId: "secret-device" } },
  });
  expect(submitted.ok()).toBe(true);
  const response = await page.request.get(`${base}/orders`);
  expect(response.headers()["cache-control"]).toBe("no-store");
  expect(response.headers()["x-robots-tag"]).toContain("noindex");
  const wire = await response.json();
  expect(Object.keys(wire).sort()).toEqual(["acceptingOrders", "deadlineAt", "orders", "status"]);
  expect(Object.keys(wire.orders[0]).sort()).toEqual(["lines", "name", "participantId", "submittedAt"]);
  expect(Object.keys(wire.orders[0].lines[0]).sort()).toEqual(["itemId", "name", "quantity"]);
  await page.goto(group.shareUrl);
  const overview = page.getByRole("tab", { name: "大家訂了什麼" });
  const own = page.getByRole("tab", { name: "我要點餐" });
  const section = page.getByRole("region", { name: "大家的已送出訂單" });
  await overview.click();
  await expect(section).toContainText("排骨飯 ×2");
  await expect(section).not.toContainText("秘密");
  await own.click();
  await page.getByRole("button", { name: /已訂者/ }).click();
  await page.getByRole("dialog").getByRole("button", { name: "是我", exact: true }).click();
  await page.getByRole("button", { name: "增加 排骨飯" }).click();
  let fail = true;
  await page.route(`**${base}/orders`, (route) => fail
    ? route.fulfill({ status: 500, json: { error: "internal_error", message: "temporary failure" } }) : route.continue());
  await overview.click();
  await expect(section.getByRole("alert")).toContainText("上次成功取得的舊資料");
  await expect(section).toContainText("排骨飯 ×2");
  await expect(section.locator("time")).toHaveCount(1);
  await own.click();
  await expect(page.getByRole("group", { name: "排骨飯 數量" })).toContainText("3");
  await expect(page.getByLabel("排骨飯 備註", { exact: true })).toHaveValue("秘密餐點備註");
  await expect(page.getByLabel("訂單備註（選填）")).toHaveValue("秘密訂單備註");
  fail = false;
  await page.getByRole("button", { name: "取消我的訂單" }).click();
  await page.getByRole("dialog").getByRole("button", { name: "確定取消", exact: true }).click();
  await expect(page.getByRole("dialog")).toHaveCount(0);
  await overview.click();
  await expect(section).toContainText("目前還沒有人訂餐");
  await submitOrder(page, group.shareUrl, pub.participants[1].id, [{ itemId: pub.menu[0].id, quantity: 4 }], "other-device");
  await section.getByRole("button", { name: "重新整理" }).click();
  await expect(section).toContainText("排骨飯 ×4");
  for (const status of ["closed", "archived"]) {
    const updated = await page.request.patch(`/api/v1/group-orders/${group.id}`, { data: { status } });
    expect(updated.ok()).toBe(true);
    await section.getByRole("button", { name: "重新整理" }).click();
    await expect(page.getByRole("banner")).toContainText(status === "closed" ? "已關閉" : "已封存");
    await expect(section).toContainText("排骨飯 ×4");
    await own.click();
    await expect(page.getByRole("region", { name: "訂單送出操作" })).toHaveCount(0);
    await overview.click();
  }
});

for (const status of [401, 403, 404]) {
  test(`overview ${status} blocks both tabs despite a late effective-order success`, async ({ page }) => {
    await devLogin(page, `overview-denied-${status}`);
    const roster = await createRoster(page, "撤權名單", ["撤權者"]);
    const vendor = await createVendor(page, "撤權店", [{ name: "排骨飯", price: 100 }]);
    const group = await createGroupOrder(page, "撤權", roster.id, vendor.id);
    const pub = await getPublicGroup(page, group.shareUrl);
    const base = `/api/v1/public/g/${group.shareUrl.split("/g/")[1]}`;
    let release!: () => void;
    const pending = new Promise<void>((resolve) => { release = resolve; });
    let started = false;
    let delivered = false;
    await page.route(`**${base}/participants/${pub.participants[0].id}/order`, async (route) => {
      const response = await route.fetch();
      started = true;
      await pending;
      await route.fulfill({ response });
      delivered = true;
    });
    await page.route(`**${base}/orders`, (route) => route.fulfill({ status, json: { error: "forbidden", message: "denied" } }));
    await page.goto(group.shareUrl);
    await page.getByRole("button", { name: "撤權者", exact: true }).click();
    await expect.poll(() => started).toBe(true);
    await page.getByRole("tab", { name: "大家訂了什麼" }).click();
    await expect(page.getByRole("alert")).toContainText(status === 404 ? "失效" : "無權限");
    release();
    await expect.poll(() => delivered).toBe(true);
    await expect(page.getByRole("tab")).toHaveCount(0);
    await expect(page.getByRole("dialog")).toHaveCount(0);
    await expect(page.getByRole("region", { name: "訂單送出操作" })).toHaveCount(0);
    expect(await page.evaluate(() => document.documentElement.style.getPropertyValue("--order-bar-h"))).toBe("");
  });
}

test("overview refreshes on entry, visible intervals and visibility resume only", async ({ page }) => {
  await devLogin(page, "overview-poll");
  const roster = await createRoster(page, "輪詢名單", ["輪詢者"]);
  const vendor = await createVendor(page, "輪詢店", [{ name: "排骨飯", price: 100 }]);
  const group = await createGroupOrder(page, "輪詢", roster.id, vendor.id);
  let reads = 0;
  page.on("request", (request) => { if (request.url().endsWith("/orders")) reads++; });
  await page.clock.install();
  await page.goto(group.shareUrl);
  const own = page.getByRole("tab", { name: "我要點餐" });
  const overview = page.getByRole("tab", { name: "大家訂了什麼" });
  const refresh = page.getByRole("button", { name: "重新整理", exact: true });
  await overview.click();
  await expect(refresh).toBeEnabled();
  expect(reads).toBe(1);
  await page.clock.fastForward(30_001);
  await expect.poll(() => reads).toBe(2);
  await expect(refresh).toBeEnabled();
  // Synthetic visibility event: deterministic coverage, not an OS background-tab assertion.
  await page.evaluate(() => {
    Object.defineProperty(document, "visibilityState", { configurable: true, value: "hidden" });
    document.dispatchEvent(new Event("visibilitychange"));
  });
  await page.clock.fastForward(60_001);
  expect(reads).toBe(2);
  await page.evaluate(() => {
    Object.defineProperty(document, "visibilityState", { configurable: true, value: "visible" });
    document.dispatchEvent(new Event("visibilitychange"));
  });
  await expect.poll(() => reads).toBe(3);
  await expect(refresh).toBeEnabled();
  await own.click();
  await page.clock.fastForward(60_001);
  expect(reads).toBe(3);
  await overview.click();
  await expect.poll(() => reads).toBe(4);
  await expect(refresh).toBeEnabled();
});
