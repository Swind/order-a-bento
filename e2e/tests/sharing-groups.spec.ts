import { expect, test, type Page } from "@playwright/test";
import { randomUUID } from "node:crypto";
import {
  createGroupOrder, createRoster, createVendor, devLogin,
  expectNoHorizontalOverflow, getPublicGroup, submitOrder,
} from "../helpers";

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
    await expect(member.getByLabel("開團歸屬")).toHaveValue("");
    await member.getByRole("button", { name: "開團", exact: true }).click();
    await expect(member).toHaveURL(/group-orders\/[0-9a-f-]+$/);
    const order = { id: new URL(member.url()).pathname.split("/").pop()! };
    expect(await (await member.request.get(`/api/v1/group-orders/${order.id}`)).json()).toMatchObject({
      sharingGroupId: null, sharingGroupName: null, canManage: true,
    });
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

async function addMember(admin: Page, groupId: string, member: Page, role = "member") {
  const identity = await (await member.request.get("/api/v1/me")).json();
  const response = await admin.request.post(`/api/v1/sharing-groups/${groupId}/members`, {
    data: { userId: identity.id, role },
  });
  expect(response.ok(), await response.text()).toBeTruthy();
  return identity.id as string;
}

// Exercise every management write with valid data: hiding controls alone is not authorization.
async function expectDeniedWrites(
  page: Page, orderId: string, participantId: string, itemId: string, statuses: number[],
) {
  const base = `/api/v1/group-orders/${orderId}`;
  const writes = [
    { method: "PATCH", url: base, data: { title: "不允許的修改" } },
    { method: "PATCH", url: base, data: { status: "closed" } },
    { method: "POST", url: `${base}/share-token/rotate`, data: {} },
    { method: "POST", url: `${base}/participants`, data: { name: "不允許的新成員" } },
    { method: "PATCH", url: `${base}/offered-items/${itemId}`, data: { soldOut: true } },
    { method: "POST", url: `${base}/participants/${participantId}/submissions`, data: {
      lines: [{ itemId, quantity: 2 }], idempotencyKey: randomUUID(),
    } },
    { method: "PUT", url: `${base}/participants/${participantId}/settlement`, data: {
      pickedUp: true, paid: true,
    } },
  ];
  for (const { url, ...options } of writes) {
    const response = await page.request.fetch(url, options);
    expect(statuses, `${options.method} ${url}: ${await response.text()}`).toContain(response.status());
  }
}

async function expectNoManagementControls(page: Page, participantName: string, itemName: string) {
  await expect(page.getByRole("button", { name: /關閉收單|重新開放|重新產生連結|修改標題|修改截止時間|新增成員/ })).toHaveCount(0);
  await page.getByRole("tab", { name: /領取/ }).click();
  await expect(page.getByRole("switch", { name: `${participantName} 繳費` })).toBeDisabled();
  await expect(page.getByRole("switch", { name: `${participantName} 領取` })).toBeDisabled();
  await page.getByRole("tab", { name: "統計" }).click();
  await expect(page.getByRole("switch", { name: `${itemName} 售完` })).toBeDisabled();
  await page.getByRole("tab", { name: "明細" }).click();
  await expect(page.getByRole("button", { name: /代訂|代為|售完/ })).toHaveCount(0);
  await expectNoHorizontalOverflow(page);
}

test("group ownership is explicit; current admins and member organizers manage, others only read", async ({ page, browser }) => {
  const organizerContext = await browser.newContext({ baseURL: test.info().project.use.baseURL });
  const readerContext = await browser.newContext({ baseURL: test.info().project.use.baseURL });
  try {
    const organizer = await organizerContext.newPage();
    const reader = await readerContext.newPage();
    await devLogin(page, "owned-admin");
    await devLogin(organizer, "owned-organizer");
    await devLogin(reader, "owned-reader");
    const group = await create(page, "/api/v1/sharing-groups", { name: "訂單共用群組" });
    const organizerId = await addMember(page, group.id, organizer);
    const readerId = await addMember(page, group.id, reader);

    // Personal directory sources can create a group-owned order: source and ownership are independent.
    const roster = await createRoster(organizer, "開團者私人名單", ["共用午餐同學"]);
    const vendor = await createVendor(organizer, "開團者私人店家", [{ name: "共用午餐便當", price: 100 }]);
    const personal = await createGroupOrder(organizer, "明確個人歸屬", roster.id, vendor.id, { sharingGroupId: null });
    expect((await page.request.get(`/api/v1/group-orders/${personal.id}`)).status()).toBe(404);
    await organizer.goto("/group-orders/new");
    await organizer.getByRole("button", { name: /開團者私人名單/ }).click();
    await organizer.getByRole("button", { name: "下一步" }).click();
    await organizer.getByRole("button", { name: /開團者私人店家/ }).click();
    await organizer.getByRole("button", { name: "下一步" }).click();
    await expect(organizer.getByLabel("開團歸屬")).toHaveValue("");
    await organizer.getByLabel("開團歸屬").selectOption(group.id);
    await organizer.fill("#go-title", "群組共同午餐");
    await organizer.getByRole("button", { name: "開團", exact: true }).click();
    await expect(organizer).toHaveURL(/group-orders\/[0-9a-f-]+$/);
    const id = new URL(organizer.url()).pathname.split("/").pop()!;
    const base = `/api/v1/group-orders/${id}`;
    const detail = await (await organizer.request.get(base)).json();
    expect(detail).toMatchObject({ sharingGroupId: group.id, sharingGroupName: group.name ?? "訂單共用群組", canManage: true });
    const pub = await getPublicGroup(organizer, detail.shareUrl);
    const pid = pub.participants[0].id;
    const itemId = pub.menu[0].id;
    await submitOrder(organizer, detail.shareUrl, pid, [{ itemId, quantity: 1 }], "owned-device");

    for (const actor of [page, organizer, reader]) {
      const response = await actor.request.get("/api/v1/group-orders");
      expect(response.ok()).toBeTruthy();
      expect(await response.json()).toEqual(expect.arrayContaining([
        expect.objectContaining({ id, sharingGroupId: group.id, sharingGroupName: "訂單共用群組", canManage: actor !== reader }),
      ]));
      await actor.goto("/group-orders");
      await actor.getByText("群組共同午餐", { exact: true }).click();
      await expect(actor).toHaveURL(new RegExp(`/group-orders/${id}$`));
    }
    expect(await (await reader.request.get(base)).json()).toMatchObject({ canManage: false });
    await expectNoManagementControls(reader, "共用午餐同學", "共用午餐便當");
    await expectDeniedWrites(reader, id, pid, itemId, [403]);
    expect(await (await organizer.request.get(base)).json()).toMatchObject({ title: "群組共同午餐", status: "open", shareUrl: detail.shareUrl });
    expect((await getPublicGroup(organizer, detail.shareUrl)).participants).toHaveLength(1);

    // Both non-owner admin and original organizer have real write permission.
    expect((await page.request.patch(base, { data: { note: "管理員已確認" } })).ok()).toBeTruthy();
    expect((await organizer.request.post(`${base}/participants/${pid}/submissions`, { data: {
      lines: [{ itemId, quantity: 1 }], idempotencyKey: randomUUID(),
    } })).ok()).toBeTruthy();
    await organizer.reload();
    await expect(organizer.getByRole("button", { name: "關閉收單" })).toBeVisible();

    // Promotion grants management; downgrade revokes it without removing read access.
    const membership = `/api/v1/sharing-groups/${group.id}/members/${readerId}`;
    expect((await page.request.patch(membership, { data: { role: "admin" } })).ok()).toBeTruthy();
    expect(await (await reader.request.get(base)).json()).toMatchObject({ canManage: true });
    expect((await reader.request.patch(base, { data: { note: "升級管理員已確認" } })).ok()).toBeTruthy();
    expect((await page.request.patch(membership, { data: { role: "member" } })).ok()).toBeTruthy();
    expect(await (await reader.request.get(base)).json()).toMatchObject({ canManage: false });
    await expectDeniedWrites(reader, id, pid, itemId, [403]);
    await reader.reload();
    await expectNoManagementControls(reader, "共用午餐同學", "共用午餐便當");

    for (const [actor, userId] of [[reader, readerId], [organizer, organizerId]] as const) {
      expect((await page.request.delete(`/api/v1/sharing-groups/${group.id}/members/${userId}`)).ok()).toBeTruthy();
      for (const suffix of ["", "/summary", "/participants", `/participants/${pid}/submissions`]) {
        expect((await actor.request.get(`${base}${suffix}`)).status()).toBe(404);
      }
      await expectDeniedWrites(actor, id, pid, itemId, [404]);
      expect(await (await actor.request.get("/api/v1/group-orders")).json()).not.toEqual(
        expect.arrayContaining([expect.objectContaining({ id })]),
      );
      await actor.goto("/group-orders");
      await expect(actor.getByText("群組共同午餐", { exact: true })).toHaveCount(0);
    }
    expect(await (await organizer.request.get(`/api/v1/group-orders/${personal.id}`)).json()).toMatchObject({
      sharingGroupId: null, sharingGroupName: null, canManage: true,
    });
    expect((await page.request.patch(base, { data: { note: "移除開團者後仍可管理" } })).ok()).toBeTruthy();
  } finally {
    await organizerContext.close();
    await readerContext.close();
  }
});

test("archived group orders remain visible but all management and public writes are refused", async ({ page, browser }) => {
  const memberContext = await browser.newContext({ baseURL: test.info().project.use.baseURL });
  try {
    const member = await memberContext.newPage();
    await devLogin(page, "archive-admin");
    await devLogin(member, "archive-organizer");
    const group = await create(page, "/api/v1/sharing-groups", { name: "封存訂單群組" });
    await addMember(page, group.id, member);
    const roster = await createRoster(member, "封存名單", ["封存同學"]);
    const vendor = await createVendor(member, "封存店家", [{ name: "封存便當", price: 100 }]);
    const order = await createGroupOrder(member, "封存共用午餐", roster.id, vendor.id, { sharingGroupId: group.id });
    const pub = await getPublicGroup(member, order.shareUrl);
    const pid = pub.participants[0].id;
    const itemId = pub.menu[0].id;
    await submitOrder(member, order.shareUrl, pid, [{ itemId, quantity: 1 }], "archive-device");
    const base = `/api/v1/group-orders/${order.id}`;
    for (const status of ["closed", "archived"]) {
      expect((await page.request.patch(base, { data: { status } })).ok()).toBeTruthy();
    }
    for (const actor of [page, member]) {
      const response = await actor.request.get(base);
      expect(response.ok()).toBeTruthy();
      expect(await response.json()).toMatchObject({ status: "archived", sharingGroupId: group.id });
      await actor.goto(`/group-orders/${order.id}`);
      await expect(actor.getByText("此團已封存，內容為唯讀。")).toBeVisible();
      await expectNoManagementControls(actor, "封存同學", "封存便當");
      await expectDeniedWrites(actor, order.id, pid, itemId, [409, 422]);
    }
    const token = order.shareUrl.split("/g/")[1];
    // Existing order update and cancellation (empty lines) must both fail.
    for (const lines of [[{ itemId, quantity: 2 }], []]) {
      const response = await page.request.post(`/api/v1/public/g/${token}/participants/${pid}/submissions`, {
        data: { lines, idempotencyKey: randomUUID(), client: { deviceId: "archive-device" } },
      });
      expect(response.status()).toBe(409);
    }
    expect(await (await page.request.get(base)).json()).toMatchObject({ title: "封存共用午餐", status: "archived", shareUrl: order.shareUrl });
    expect((await getPublicGroup(page, order.shareUrl)).participants).toEqual([
      expect.objectContaining({ id: pid, hasOrdered: true }),
    ]);
  } finally {
    await memberContext.close();
  }
});
