import { expect, test } from "@playwright/test";
import {
  createGroupOrder,
  createRoster,
  createVendor,
  devLogin,
  expectNoHorizontalOverflow,
  getPublicGroup,
  submitOrder,
} from "../helpers";

test("decimal dumpling prices stay exact through manual editing, public ordering and settlement", async ({ page }) => {
  await devLogin(page, "decimal-dumplings");
  const roster = await createRoster(page,"水餃名單",["王小明"]);
  const vendor = await createVendor(page,"八方雲集",[{name:"水餃",price:8}]);
  await page.goto(`/vendors/${vendor.id}`);
  await page.getByRole("button", {name:"編輯 水餃"}).click();
  await page.getByLabel("價格", {exact:true}).fill("7.5");
  await page.getByRole("form",{name:"編輯 水餃"}).getByRole("button",{name:"儲存",exact:true}).click();
  await expect(page.getByText("$7.5", {exact:true})).toBeVisible();
  const group = await createGroupOrder(page,"水餃小數驗收",roster.id,vendor.id);
  await page.goto(group.shareUrl);
  await page.getByRole("button",{name:"王小明",exact:true}).click();
  for (let i=0;i<3;i++) await page.getByRole("button",{name:"增加 水餃"}).click();
  await expect(page.getByRole("region",{name:"訂單送出操作"})).toContainText("$22.5");
  await page.getByRole("button",{name:"送出訂單",exact:true}).click();
  await expect(page.getByRole("region",{name:"已送出訂單",exact:true})).toContainText("總額 $22.5");
  await expect(page.getByRole("region",{name:"已送出訂單",exact:true})).toContainText("$7.5 × 3");
  await expectNoHorizontalOverflow(page);
  await page.screenshot({path:test.info().outputPath("decimal-dumplings-order.png"),fullPage:true});
  await page.goto(`/group-orders/${group.id}`);
  await page.getByRole("tab",{name:"統計"}).click();
  await expect(page.getByText("$22.5",{exact:true}).first()).toBeVisible();
  await page.getByRole("tab",{name:/領取/}).click();
  const paid = page.getByRole("switch",{name:"王小明 繳費"});
  await paid.click();
  await expect(paid).toHaveAttribute("aria-checked","true");
  await expect(page.getByText(/已收 \$22.5/)).toBeVisible();
  await expectNoHorizontalOverflow(page);
  await page.screenshot({path:test.info().outputPath("decimal-dumplings-settlement.png"),fullPage:true,animations:"disabled"});
  const summary = await (await page.request.get(`/api/v1/group-orders/${group.id}/summary`)).json();
  expect(summary.totalAmount).toBe(22.5); expect(summary.items[0].price).toBe(7.5); expect(summary.items[0].subtotal).toBe(22.5);
  expect(summary.settlement.paidAmount).toBe(22.5);
});

test("stats, checklist, history, close and rotate", async ({ page, browser }) => {
  await page.context().grantPermissions(["clipboard-read", "clipboard-write"]);
  await devLogin(page, "management");
  const r = await createRoster(page, "業務部", ["王小明", "李大華", "陳美麗"]);
  const v = await createVendor(page, "好吃便當", [
    { name: "排骨飯", price: 100 },
    { name: "雞腿飯", price: 110 },
  ]);
  const g = await createGroupOrder(page, "週五午餐", r.id, v.id);
  const pub = await getPublicGroup(page, g.shareUrl);
  const P = Object.fromEntries(pub.participants.map((p) => [p.name, p.id]));
  const I = Object.fromEntries(pub.menu.map((m) => [m.name, m.id]));
  await submitOrder(page, g.shareUrl, P["王小明"], [{ itemId: I["排骨飯"], quantity: 2 }], "aaa-1111");
  await submitOrder(page, g.shareUrl, P["李大華"], [{ itemId: I["雞腿飯"], quantity: 1 }], "bbb-2222");
  await submitOrder(page, g.shareUrl, P["陳美麗"], [{ itemId: I["排骨飯"], quantity: 1 }], "aaa-1111");

  // List
  await page.goto("/group-orders");
  await expect(page.getByText("週五午餐").first()).toBeVisible();
  await expect(page.getByText(/已訂 3\s*\/\s*3/).first()).toBeVisible();
  await expectNoHorizontalOverflow(page);

  // Stats
  await page.getByText("週五午餐").first().click();
  await expect(page).toHaveURL(/group-orders\/[0-9a-f-]+/);
  await page.getByRole("tab", { name: "統計" }).click();
  await expect(page.locator("dl").filter({ has: page.locator("dt", { hasText: "總份數" }) })).toContainText("4 份");
  await expectNoHorizontalOverflow(page);

  // Checklist: mark paid, then resubmit
  await page.getByRole("tab", { name: /領取/ }).click();
  const sw = page.getByRole("switch", { name: "王小明 繳費" });
  await sw.click();
  await expect(sw).toHaveAttribute("aria-checked", "true");
  await submitOrder(page, g.shareUrl, P["王小明"], [{ itemId: I["排骨飯"], quantity: 3 }], "aaa-1111");
  await page.reload();
  await expect(page.getByText(/尚差 \$100/).first()).toBeVisible();
  await expectNoHorizontalOverflow(page);

  // Details + history sheet
  await page.getByRole("tab", { name: "明細" }).click();
  await expect(page.getByText(/改過 2 次/).first()).toBeVisible();
  await expect(page.getByText(/同裝置/).first()).toBeVisible();
  const confirmation = page.getByRole("textbox", { name: "訂餐確認列表" });
  await expect(confirmation).toHaveValue(/王小明：排骨飯 ×3/);
  await expect(confirmation).toHaveValue(/李大華：雞腿飯 ×1/);
  await expect(confirmation).toHaveValue(/陳美麗：排骨飯 ×1/);
  await expect(confirmation).toHaveValue(/已訂 3 人，共 5 個/);
  await page.getByRole("button", { name: "複製訂餐確認列表" }).click();
  await expect(page.getByText("已複製訂餐確認列表", { exact: true })).toBeVisible();
  expect(await page.evaluate(() => navigator.clipboard.readText())).toBe(await confirmation.inputValue());
  await expectNoHorizontalOverflow(page);
  await page.getByText("王小明").first().click();
  await expect(page.getByText("目前有效").first()).toBeVisible();
  await expect(page.getByText(/此裝置也替.*陳美麗/).first()).toBeVisible();
  await page.keyboard.press("Escape");

  // Close
  await page.getByRole("button", { name: "關閉收單" }).click();
  await expect(page.getByRole("button", { name: "重新開放" })).toBeVisible();
  await expect(page.getByText("分享與訂購連結", { exact: true })).toBeVisible();
  await expect(page.getByRole("tab", { name: "明細" })).toHaveAttribute("aria-selected", "true");

  const pubCtx = await browser.newContext();
  const p = await pubCtx.newPage();
  await p.goto(g.shareUrl);
  await expect(p.getByText("已關閉").first()).toBeVisible();
  await p.getByRole("button", { name: /李大華/ }).click();
  await p.getByRole("dialog").getByRole("button", { name: "是我" }).click();
  await expect(p.getByRole("region", { name: "已送出訂單" })).toContainText("雞腿飯 ×1");
  const submit = p.getByRole("button", { name: /送出訂單|更新訂單/ });
  if (await submit.count()) await expect(submit.first()).toBeDisabled();

  // Rotate link (available while closed)
  await page.getByText("更多操作", { exact: true }).click();
  await expectNoHorizontalOverflow(page);
  await page.getByRole("button", { name: "重新產生連結" }).click();
  await page.getByRole("button", { name: "確定重新產生連結" }).click();
  await page.getByText("分享與訂購連結", { exact: true }).click();
  await page.getByText("連結與其他操作", { exact: true }).click();
  await expect(page.getByLabel("分享連結", { exact: true })).not.toHaveValue(g.shareUrl);

  const old = await pubCtx.newPage();
  await old.goto(g.shareUrl);
  await expect(old.getByText("這個訂購連結無效或已失效").first()).toBeVisible();
  await pubCtx.close();
});
