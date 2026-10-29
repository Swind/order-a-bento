import { expect, test } from "@playwright/test";
import { createRoster, createVendor, devLogin, expectNoHorizontalOverflow, getPublicGroup } from "../helpers";

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
  await expect(b.getByRole("region", { name: "已送出訂單" })).toContainText("素食便當 ×1");
  await expect(b.getByRole("region", { name: "已送出訂單" })).toContainText("總額 $90");
  await expect(b.getByRole("button", { name: "增加 排骨飯" })).toHaveCount(0);

  await A.ctx.close();
  await B.ctx.close();
});
