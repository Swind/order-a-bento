import { expect, test, type Page } from "@playwright/test";
import { createRoster, devLogin, expectNoHorizontalOverflow } from "../helpers";

async function create(page: Page, url: string, data: unknown) {
  const response = await page.request.post(url, { data });
  expect(response.ok(), `${url}: ${response.status()} ${await response.text()}`).toBeTruthy();
  return response.json();
}

test("shared directory is read-only for members and orders remain personal after removal", async ({ page, browser }) => {
  const memberContext = await browser.newContext({ baseURL: test.info().project.use.baseURL });
  const member = await memberContext.newPage();
  try {
    await devLogin(page, "sharing-admin");
    await devLogin(member, "sharing-member");
    const memberIdentity = await (await member.request.get("/api/v1/me")).json();
    const adminIdentity = await (await page.request.get("/api/v1/me")).json();
    const group = await create(page, "/api/v1/sharing-groups", { name: "共同教室" });
    const lastAdmin = await page.request.patch(`/api/v1/sharing-groups/${group.id}/members/${adminIdentity.id}`, {
      data: { role: "member" },
    });
    expect(lastAdmin.status()).toBe(409);
    await page.goto(`/sharing-groups/${group.id}`);
    await page.getByLabel("使用者 Email").fill(memberIdentity.email);
    await page.getByRole("button", { name: "查找帳號" }).click();
    await expect(page.getByRole("radio")).toBeChecked();
    await page.getByRole("button", { name: "加入成員", exact: true }).click();
    await expect(page.getByLabel("使用者 Email")).toHaveValue("");
    const privateRoster = await createRoster(page, "私人名單", ["私人同學"]);
    const roster = await create(page, "/api/v1/rosters", {
      name: "共用名單", sharingGroupId: group.id, members: [{ name: "共用同學" }],
    });
    const vendor = await create(page, "/api/v1/vendors", {
      name: "共用便當店", sharingGroupId: group.id, menuItems: [{ name: "便當", price: 100 }],
    });

    expect((await member.request.get(`/api/v1/rosters/${privateRoster.id}`)).status()).toBe(404);
    const visible = await member.request.get(`/api/v1/rosters/${roster.id}`);
    expect(visible.ok()).toBeTruthy();
    expect((await visible.json()).canManage).toBe(false);
    const denied = await member.request.patch(`/api/v1/rosters/${roster.id}`, { data: { name: "不允許" } });
    expect([403, 404]).toContain(denied.status());
    const memberAdmin = await member.request.patch(`/api/v1/sharing-groups/${group.id}`, { data: { name: "不允許" } });
    expect(memberAdmin.status()).toBe(403);

    await member.goto("/rosters");
    await member.getByLabel("資料範圍").selectOption(group.id);
    await member.getByText("共用名單", { exact: true }).click();
    await expect(member.getByText("共用同學", { exact: true })).toBeVisible();
    await expect(member.locator("#batch-members")).toHaveCount(0);
    await expectNoHorizontalOverflow(member);

    await member.goto("/group-orders/new");
    await member.getByRole("button", { name: /共用名單/ }).click();
    await member.getByRole("button", { name: "下一步" }).click();
    await member.getByRole("button", { name: /共用便當店/ }).click();
    await member.getByRole("button", { name: "下一步" }).click();
    await member.fill("#go-title", "成員自己的午餐");
    await member.getByRole("button", { name: "開團", exact: true }).click();
    await expect(member).toHaveURL(/group-orders\/[0-9a-f-]+$/);
    const order = { id: new URL(member.url()).pathname.split("/").pop()! };
    expect((await page.request.get(`/api/v1/group-orders/${order.id}`)).status()).toBe(404);
    const removal = await page.request.delete(`/api/v1/sharing-groups/${group.id}/members/${memberIdentity.id}`);
    expect(removal.ok()).toBeTruthy();
    expect((await member.request.get(`/api/v1/rosters/${roster.id}`)).status()).toBe(404);
    expect((await member.request.get(`/api/v1/vendors/${vendor.id}`)).status()).toBe(404);
    expect((await member.request.get(`/api/v1/group-orders/${order.id}`)).ok()).toBeTruthy();
    await member.goto(`/group-orders/${order.id}`);
    await expect(member.getByText("成員自己的午餐").first()).toBeVisible();
  } finally {
    await memberContext.close();
  }
});
