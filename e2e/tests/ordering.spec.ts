import { expect, test } from "@playwright/test";
import { createGroupOrder, createRoster, createVendor, devLogin, expectNoHorizontalOverflow, getPublicGroup, submitOrder } from "../helpers";

interface PersistedOrder {
  cancelled: boolean;
  submittedAt: string;
  note: string | null;
  total: number;
  lines: { name: string; quantity: number; unitPrice: number; subtotal: number; note: string | null }[];
}

test("completion lifecycle uses persisted snapshots, explicit editing and remembered identity", async ({ page }, info) => {
  await devLogin(page, "completion-lifecycle");
  const name = "王小明這是需要完整顯示的長姓名與訂餐身分";
  const meals = ["超長品名香煎豪華雙主餐季節時蔬便當附湯以及飲料", "季節蔬食便當搭配水果與飲料"];
  const itemNote = "餐點私密備註不要辣請將醬汁另外包裝並且完整保留這段長備註";
  const orderNote = "訂單私密備註第一行請分開包裝\n第二行請送到產品團隊會議室並確認姓名";
  const roster = await createRoster(page, "完成流程名單", [name, "其他參與者"]);
  const vendor = await createVendor(page, "完成流程店家", meals.map((name, i) => ({ name, price: 123 + i * 37 })));
  const group = await createGroupOrder(page, "完成畫面與長文字驗收", roster.id, vendor.id);
  const pub = await getPublicGroup(page, group.shareUrl);
  const pid = pub.participants.find((p) => p.name === name)!.id;
  const base = `/api/v1/public/g/${group.shareUrl.split("/g/")[1]}`;
  const endpoint = `${base}/participants/${pid}/submissions`;
  const own = page.getByRole("tab", { name: "我要點餐", exact: true });
  const overview = page.getByRole("tab", { name: "大家訂了什麼", exact: true });
  const status = page.getByRole("status");
  const saved = page.getByRole("region", { name: "已送出訂單", exact: true });
  const bar = page.getByRole("region", { name: "訂單送出操作" });
  const noBar = async () => {
    await expect(bar).toHaveCount(0);
    expect(await page.evaluate(() => document.documentElement.style.getPropertyValue("--order-bar-h"))).toBe("");
    await expectNoHorizontalOverflow(page);
  };
  const snapshot = async (label: string) => {
    const response = await page.request.get(`${base}/participants/${pid}/order`);
    expect(response.ok()).toBe(true);
    const wire = await response.json() as { order: PersistedOrder };
    await info.attach(`${label}-server.json`, { body: JSON.stringify(wire, null, 2), contentType: "application/json" });
    return wire.order;
  };
  const expectSaved = async (order: Awaited<ReturnType<typeof snapshot>>) => {
    expect(order.cancelled).toBe(false);
    await expect(status).toHaveText("訂單已送出");
    await expect(status).toHaveClass(/bg-green-50/);
    await expect(status).toHaveClass(/text-green-900/);
    await expect(status.locator("svg")).toHaveCount(1);
    const edit = page.getByRole("button", { name: "修改訂單", exact: true });
    await expect.poll(async () => (await edit.boundingBox())!.height).toBe(48);
    await expect.poll(async () => Math.abs((await edit.boundingBox())!.width - (await saved.boundingBox())!.width)).toBeLessThan(1);
    const cancelSection = page.getByRole("region", { name: "上次送出的訂單", exact: true });
    expect(await cancelSection.evaluate((el) => getComputedStyle(el).backgroundColor)).toBe("rgba(0, 0, 0, 0)");
    await expect(cancelSection.getByRole("button")).toHaveClass(/text-muted-foreground/);
    expect(await cancelSection.getByRole("button").evaluate((el) => getComputedStyle(el).backgroundColor)).toBe("rgba(0, 0, 0, 0)");
    await expect(saved).toContainText(name);
    await expect(saved).toContainText("已登記至本團，並非店家已接單");
    for (const line of order.lines) {
      await expect(saved).toContainText(`${line.name} ×${line.quantity}`);
      await expect(saved).toContainText(`$${line.unitPrice.toLocaleString("en-US")} × ${line.quantity}`);
      await expect(saved).toContainText(`小計 $${line.subtotal.toLocaleString("en-US")}`);
      if (line.note) await expect(saved).toContainText(line.note);
    }
    if (order.note) await expect(saved).toContainText(order.note.replace(/\s+/g, " "));
    await expect(saved).toContainText(`總額 $${order.total.toLocaleString("en-US")}`);
    await expect(saved.locator("time")).toHaveAttribute("datetime", order.submittedAt);
    await noBar();
  };
  await page.goto(group.shareUrl);
  await page.getByRole("button", { name, exact: true }).click();
  for (const meal of meals) await page.getByRole("button", { name: `增加 ${meal}` }).click();
  await page.getByRole("button", { name: "＋備註", exact: true }).first().click();
  await page.getByLabel(`${meals[0]} 備註`, { exact: true }).fill(itemNote);
  await page.getByLabel("訂單備註（選填）").fill(orderNote);
  await overview.click();
  await noBar();
  const hiddenOwn = page.getByRole("tabpanel", { name: "我要點餐", includeHidden: true });
  await expect(hiddenOwn).toHaveAttribute("hidden", "");
  await expect(hiddenOwn).toHaveAttribute("inert", "");
  await own.click();
  await expect(page.getByLabel(`${meals[0]} 備註`, { exact: true })).toHaveValue(itemNote);
  await expect(page.getByLabel("訂單備註（選填）")).toHaveValue(orderNote);

  // Delay a real successful backend response: no completion before acknowledgement, no hidden-tab focus theft.
  let release!: () => void;
  const pending = new Promise<void>((resolve) => { release = resolve; });
  let received = false;
  await page.route(`**${endpoint}`, async (route) => {
    const response = await route.fetch();
    expect(response.ok()).toBe(true);
    await info.attach("initial-post-server.json", { body: await response.body(), contentType: "application/json" });
    received = true;
    await pending;
    await route.fulfill({ response });
  });
  await page.getByRole("button", { name: "送出訂單", exact: true }).click();
  await expect.poll(() => received).toBe(true);
  await expect(saved).toHaveCount(0);
  await expect(page.getByRole("button", { name: "送出中…", exact: true })).toBeDisabled();
  await overview.click();
  release();
  await expect(hiddenOwn.getByRole("status", { includeHidden: true })).toHaveText("訂單已送出");
  await expect(overview).toBeFocused();
  await own.click();
  await expect(own).toBeFocused();
  await page.unroute(`**${endpoint}`);
  const first = await snapshot("initial");
  await expectSaved(first);
  await info.attach("success-layout.json", { body: JSON.stringify(await status.evaluate((el) => ({
    background: getComputedStyle(el).backgroundColor, color: getComputedStyle(el).color,
    width: el.getBoundingClientRect().width, height: el.getBoundingClientRect().height,
  }))), contentType: "application/json" });
  // Capture the durable page, not the transient toast animation over its header.
  await expect(page.locator("[data-sonner-toast]")).toHaveCount(0, { timeout: 10000 });
  await page.evaluate(() => window.scrollTo(0, 0));
  await page.screenshot({ path: info.outputPath("completion-viewport.png") });
  await page.screenshot({ path: info.outputPath("completion-full.png"), fullPage: true });

  let writes = 0;
  page.on("request", (request) => { if (request.method() === "POST" && request.url().endsWith(endpoint)) writes++; });
  await page.getByRole("button", { name: "修改訂單", exact: true }).click();
  await expect(page.getByRole("heading", { name: "修改訂單", exact: true })).toBeFocused();
  await expect(page.getByRole("region", { name: "上次送出的訂單", exact: true })).toHaveClass(/bg-accent/);
  await page.getByRole("button", { name: `增加 ${meals[0]}` }).click();
  await page.getByLabel("訂單備註（選填）").fill("將被放棄的修改");
  await overview.click();
  await noBar();
  await own.click();
  await expect(page.getByRole("group", { name: `${meals[0]} 數量` })).toContainText("2");
  await expect(page.getByLabel("訂單備註（選填）")).toHaveValue("將被放棄的修改");
  await page.getByRole("button", { name: "放棄修改", exact: true }).click();
  await expect(status).toBeFocused();
  await expectSaved(first);
  expect(writes).toBe(0);
  expect(await snapshot("after-abandon")).toEqual(first);
  await page.getByRole("button", { name: "修改訂單", exact: true }).click();
  await expect(page.getByLabel("訂單備註（選填）")).toHaveValue(orderNote);
  await expect(page.getByRole("group", { name: `${meals[0]} 數量` })).toContainText("1");
  await page.getByRole("button", { name: `增加 ${meals[0]}` }).click();
  await bar.scrollIntoViewIfNeeded();
  await page.screenshot({ path: info.outputPath("editing-viewport.png") });
  await page.getByRole("button", { name: "儲存修改", exact: true }).click();
  await expect(status).toHaveText("訂單已送出");
  await expect(status).toBeFocused();
  const updated = await snapshot("updated");
  expect(updated.lines.find((line) => line.name === meals[0])?.quantity).toBe(2);
  await expectSaved(updated);
  await page.getByRole("button", { name: "取消我的訂單", exact: true }).click();
  await page.getByRole("dialog").getByRole("button", { name: "確定取消", exact: true }).click();
  await expect(status).toHaveText("你的訂單已取消");
  await expect(status).toBeFocused();
  await expect(status).not.toHaveClass(/bg-green/);
  await expect(status.locator("svg")).toHaveCount(0);
  expect(await status.evaluate((el) => getComputedStyle(el).backgroundColor)).toBe("rgba(0, 0, 0, 0)");
  const reorder = page.getByRole("button", { name: "重新訂餐", exact: true });
  await expect.poll(async () => (await reorder.boundingBox())!.height).toBe(48);
  await expect.poll(async () => Math.abs((await reorder.boundingBox())!.width - (await status.boundingBox())!.width)).toBeLessThan(1);
  expect((await snapshot("cancelled")).cancelled).toBe(true);
  await noBar();
  await expect(page.locator("[data-sonner-toast]")).toHaveCount(0, { timeout: 10000 });
  await page.evaluate(() => window.scrollTo(0, 0));
  await page.screenshot({ path: info.outputPath("cancelled-viewport.png") });
  await page.getByRole("button", { name: "重新訂餐", exact: true }).click();
  await expect(page.getByRole("heading", { name: "選擇餐點", exact: true })).toBeFocused();
  await expect(page.getByLabel("訂單備註（選填）")).toHaveValue("");
  await expect(page.getByLabel(`${meals[0]} 備註`, { exact: true })).toHaveCount(0);
  await page.getByRole("button", { name: `增加 ${meals[1]}` }).click();
  await expect(bar).toBeVisible();
  await expect.poll(() => page.evaluate(() => parseFloat(document.documentElement.style.getPropertyValue("--order-bar-h")))).toBeGreaterThan(0);
  await page.getByRole("button", { name: "送出訂單", exact: true }).click();
  await expect(status).toHaveText("訂單已送出");
  const reordered = await snapshot("reordered");
  await expectSaved(reordered);
  await page.goto("/login");
  await page.goto(group.shareUrl);
  await expectSaved(reordered);
  expect(await page.evaluate((token) => localStorage.getItem(`bento:last:${token}`), group.shareUrl.split("/g/")[1])).toBe(pid);
  await expect(page.getByLabel("搜尋名字")).toHaveCount(0);
});

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

  // Revisit: remembered, saved summary, then explicitly edit.
  await a.reload();
  await expect(a.getByRole("status")).toHaveText("訂單已送出");
  await a.getByRole("button", { name: "修改訂單", exact: true }).click();
  await expect(a.getByRole("group", { name: "排骨飯 數量" })).toContainText("1");
  await a.getByRole("button", { name: "減少 排骨飯" }).click();
  await a.getByRole("button", { name: "增加 雞腿飯" }).click();
  await a.getByRole("button", { name: "增加 雞腿飯" }).click();
  await expect(a.getByRole("status")).toContainText("有尚未送出的修改");
  await a.getByRole("button", { name: "儲存修改" }).click();
  await expect(a.getByRole("status")).toHaveText("訂單已送出");
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

  // Summary retains the privacy footer without a stale sticky bar.
  await b.evaluate(() => window.scrollTo(0, document.body.scrollHeight));
  const foot = await b.getByText("為核對訂單，系統會記錄 IP 與裝置資訊").boundingBox();
  expect(foot).not.toBeNull();
  await expect(b.getByRole("region", { name: "訂單送出操作" })).toHaveCount(0);
  expect(await b.evaluate(() => document.documentElement.style.getPropertyValue("--order-bar-h"))).toBe("");

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
  await b.getByRole("button", { name: "修改訂單", exact: true }).click();
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
  await page.getByRole("button", { name: "修改訂單", exact: true }).click();
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
