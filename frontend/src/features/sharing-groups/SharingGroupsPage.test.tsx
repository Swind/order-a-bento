import { screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { http, HttpResponse } from "msw";
import { beforeEach, describe, expect, it } from "vitest";
import { renderApp } from "@/test/render";
import { ME, problem, server } from "@/test/server";

const group = { id: "s1", name: "午餐同好", role: "admin", memberCount: 1 };
const member = { id: ME.id, displayName: ME.displayName, email: ME.email, role: "admin", identityKind: "local" };

beforeEach(() => server.use(
  http.get("*/api/v1/me", () => HttpResponse.json(ME)),
  http.get("*/api/v1/auth/config", () => HttpResponse.json({ aiImportEnabled: true, aiDemo: false })),
  http.get("*/api/v1/sharing-groups", () => HttpResponse.json([group])),
  http.get("*/api/v1/sharing-groups/s1", () => HttpResponse.json(group)),
  http.get("*/api/v1/sharing-groups/s1/members", () => HttpResponse.json([member])),
));

describe("sharing groups", () => {
  it("shows the exact local ID and creates a group", async () => {
    const user = userEvent.setup();
    let posted: unknown;
    server.use(http.post("*/api/v1/sharing-groups", async ({ request }) => {
      posted = await request.json();
      return HttpResponse.json(group, { status: 201 });
    }));
    renderApp("/sharing-groups");
    expect(await screen.findByText(ME.id, { selector: "code" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "複製使用者 ID" })).toBeEnabled();
    await user.click(screen.getByRole("button", { name: "複製使用者 ID" }));
    expect(await navigator.clipboard.readText()).toBe(ME.id);
    await userEvent.type(screen.getByLabelText("群組名稱"), " 新群組 ");
    await userEvent.click(screen.getByRole("button", { name: "建立共用群組" }));
    await waitFor(() => expect(posted).toEqual({ name: "新群組" }));
    expect(await screen.findByRole("link", { name: /午餐同好 · 管理員/ })).toHaveAttribute("href", "/sharing-groups/s1");
  });

  it("renames, adds an exact organizer ID, and invalidates directory list and detail caches", async () => {
    const requests: unknown[] = [];
    server.use(
      http.patch("*/api/v1/sharing-groups/s1", async ({ request }) => { requests.push(await request.json()); return HttpResponse.json(group); }),
      http.post("*/api/v1/sharing-groups/s1/members", async ({ request }) => { requests.push(await request.json()); return new HttpResponse(null, { status: 204 }); }),
    );
    const { queryClient } = renderApp("/sharing-groups/s1");
    for (const key of [["rosters", "s1"], ["roster", "r1"], ["vendors", "s1"], ["vendor", "v1"]]) queryClient.setQueryData(key, {});
    const name = await screen.findByLabelText("群組名稱");
    await userEvent.clear(name);
    await userEvent.type(name, "新名稱");
    await userEvent.click(screen.getByRole("button", { name: "儲存名稱" }));
    await waitFor(() => expect(requests).toContainEqual({ name: "新名稱" }));
    await userEvent.type(screen.getByLabelText("本站使用者 ID"), "exact-organizer-id");
    await userEvent.selectOptions(screen.getByLabelText("加入角色"), "admin");
    await userEvent.click(screen.getByRole("button", { name: "加入成員" }));
    await waitFor(() => expect(requests).toContainEqual({ userId: "exact-organizer-id", role: "admin" }));
    for (const key of [["rosters", "s1"], ["roster", "r1"], ["vendors", "s1"], ["vendor", "v1"]]) expect(queryClient.getQueryState(key)?.isInvalidated).toBe(true);
  });

  it("reports last-admin protection and preserves the role", async () => {
    server.use(http.patch("*/api/v1/sharing-groups/s1/members/o1", () => problem(409, "last_admin")));
    renderApp("/sharing-groups/s1");
    const role = await screen.findByLabelText("Ann 的角色");
    await userEvent.selectOptions(role, "member");
    expect(await screen.findByRole("alert")).toHaveTextContent("不能移除或降級最後一位管理員");
    expect(role).toHaveValue("admin");
  });

  it("updates and removes a member through the exact user ID with empty 204 responses", async () => {
    let roleBody: unknown;
    let removed = false;
    let members = [member, { ...member, id: "o2", displayName: "Bob", role: "member" }];
    server.use(
      http.get("*/api/v1/sharing-groups/s1/members", () => HttpResponse.json(members)),
      http.patch("*/api/v1/sharing-groups/s1/members/o2", async ({ request }) => {
        roleBody = await request.json();
        members = members.map((value) => value.id === "o2" ? { ...value, role: "admin" } : value);
        return new HttpResponse(null, { status: 204 });
      }),
      http.delete("*/api/v1/sharing-groups/s1/members/o2", () => {
        removed = true;
        members = members.filter((value) => value.id !== "o2");
        return new HttpResponse(null, { status: 204 });
      }),
    );
    renderApp("/sharing-groups/s1");
    await userEvent.selectOptions(await screen.findByLabelText("Bob 的角色"), "admin");
    await screen.findByText("Bob · 管理員");
    expect(roleBody).toEqual({ role: "admin" });
    await userEvent.click(screen.getByRole("button", { name: "移除 Bob" }));
    await userEvent.click(screen.getByRole("button", { name: "確定" }));
    await waitFor(() => expect(screen.queryByText("Bob · 管理員")).not.toBeInTheDocument());
    expect(removed).toBe(true);
  });

  it("hides membership management for ordinary members", async () => {
    server.use(http.get("*/api/v1/sharing-groups/s1", () => HttpResponse.json({ ...group, role: "member" })));
    renderApp("/sharing-groups/s1");
    await screen.findByText("午餐同好", { selector: "h1" });
    await screen.findByText("Ann · 管理員");
    expect(screen.queryByRole("button", { name: "加入成員" })).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "儲存名稱" })).not.toBeInTheDocument();
    expect(screen.queryByRole("combobox")).not.toBeInTheDocument();
  });

  it("refreshes revoked administrator permissions after a sharing action is forbidden", async () => {
    let revoked = false;
    server.use(
      http.get("*/api/v1/sharing-groups/s1", () => HttpResponse.json({ ...group, role: revoked ? "member" : "admin" })),
      http.get("*/api/v1/sharing-groups", () => HttpResponse.json([{ ...group, role: revoked ? "member" : "admin" }])),
      http.patch("*/api/v1/sharing-groups/s1", () => {
        revoked = true;
        return problem(403, "forbidden");
      }),
    );
    const { queryClient } = renderApp("/sharing-groups/s1");
    await userEvent.click(await screen.findByRole("button", { name: "儲存名稱" }));
    await waitFor(() => expect(screen.queryByRole("button", { name: "加入成員" })).not.toBeInTheDocument());
    expect(screen.queryByRole("button", { name: "儲存名稱" })).not.toBeInTheDocument();
    expect(queryClient.getQueryData(["sharing-group", "s1"])).toMatchObject({ role: "member" });
  });

  it("refreshes a roster to read-only after a child mutation is forbidden", async () => {
    let revoked = false;
    server.use(
      http.get("*/api/v1/rosters/r1", () => HttpResponse.json({
        id: "r1", name: "共用名單", sharingGroupId: "s1", canManage: !revoked,
        members: [{ id: "m1", name: "小王", note: null, sortOrder: 1 }],
      })),
      http.patch("*/api/v1/rosters/r1/members/m1", () => {
        revoked = true;
        return problem(403, "forbidden");
      }),
    );
    renderApp("/rosters/r1");
    await userEvent.click(await screen.findByRole("button", { name: "編輯 小王" }));
    await userEvent.type(screen.getByLabelText("備註"), "更新");
    await userEvent.click(screen.getByRole("button", { name: "儲存" }));
    expect(await screen.findByText(/唯讀（可選用開團）/)).toBeInTheDocument();
    expect(screen.queryByLabelText("批次新增成員")).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "編輯 小王" })).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "重新命名" })).not.toBeInTheDocument();
  });

  it("replaces stale vendor management with an access error after a missing-resource mutation", async () => {
    let revoked = false;
    server.use(
      http.get("*/api/v1/vendors/v1", () => revoked ? problem(404, "not_found") : HttpResponse.json({
        id: "v1", name: "共用便當", sharingGroupId: "s1", canManage: true,
        menuItems: [{ id: "i1", name: "雞腿飯", price: 100, sortOrder: 1, archived: false }],
      })),
      http.patch("*/api/v1/vendors/v1/menu-items/i1", () => {
        revoked = true;
        return problem(404, "not_found");
      }),
    );
    renderApp("/vendors/v1");
    await userEvent.click(await screen.findByRole("switch", { name: "封存 雞腿飯" }));
    expect(await screen.findByRole("alert")).toHaveTextContent("找不到資料");
    expect(screen.queryByRole("switch")).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "刪除店家" })).not.toBeInTheDocument();
    expect(screen.queryByText("雞腿飯")).not.toBeInTheDocument();
  });

  it("creates rosters in the selected scope, never exposing a scope editor on detail", async () => {
    let posted: unknown;
    const scoped = { id: "r1", name: "共用名單", sharingGroupId: "s1", canManage: true, members: [], updatedAt: "2026-01-01T00:00:00Z" };
    server.use(
      http.get("*/api/v1/rosters", () => HttpResponse.json([])),
      http.post("*/api/v1/rosters", async ({ request }) => { posted = await request.json(); return HttpResponse.json(scoped); }),
      http.get("*/api/v1/rosters/r1", () => HttpResponse.json(scoped)),
    );
    renderApp("/rosters");
    await screen.findByRole("option", { name: /午餐同好/ });
    await userEvent.selectOptions(screen.getByLabelText("資料範圍"), "s1");
    await userEvent.click(screen.getByRole("button", { name: "新增名單" }));
    const dialog = await screen.findByRole("dialog");
    expect(within(dialog).getByText("共用群組：午餐同好")).toBeInTheDocument();
    await userEvent.type(within(dialog).getByLabelText("名單名稱"), "共用名單");
    await userEvent.click(within(dialog).getByRole("button", { name: "建立" }));
    await screen.findByRole("heading", { name: "共用名單" });
    expect(posted).toEqual({ name: "共用名單", members: [], sharingGroupId: "s1" });
    expect(screen.queryByLabelText("資料範圍")).not.toBeInTheDocument();
  });

  it("hides all roster mutations in read-only detail", async () => {
    server.use(http.get("*/api/v1/rosters/r1", () => HttpResponse.json({ id: "r1", name: "共用名單", sharingGroupId: "s1", canManage: false, members: [{ id: "m1", name: "小王", note: "同事", sortOrder: 1 }] })));
    renderApp("/rosters/r1");
    await screen.findByText("小王");
    expect(screen.getByText(/唯讀（可選用開團）/)).toBeInTheDocument();
    expect(screen.queryByLabelText("批次新增成員")).not.toBeInTheDocument();
    for (const name of [/重新命名/, /刪除名單/, /編輯 小王/, /刪除 小王/, /從照片匯入/]) expect(screen.queryByRole("button", { name })).not.toBeInTheDocument();
  });

  it("uses the vendor list scope for creation and isolates personal and shared caches", async () => {
    let posted: unknown;
    const vendor = { id: "v1", name: "共用店家", sharingGroupId: "s1", canManage: true, menuItems: [], updatedAt: "2026-01-01T00:00:00Z" };
    server.use(
      http.get("*/api/v1/vendors", ({ request }) => HttpResponse.json(new URL(request.url).searchParams.has("sharingGroupId") ? [{ ...vendor, itemCount: 0 }] : [{ id: "personal", name: "私人店家", itemCount: 1 }])),
      http.post("*/api/v1/vendors", async ({ request }) => { posted = await request.json(); return HttpResponse.json(vendor); }),
      http.get("*/api/v1/vendors/v1", () => HttpResponse.json(vendor)),
    );
    const { queryClient } = renderApp("/vendors");
    await screen.findByText("私人店家");
    await screen.findByRole("option", { name: /午餐同好/ });
    await userEvent.selectOptions(screen.getByLabelText("資料範圍"), "s1");
    await screen.findByText("共用店家");
    expect(screen.queryByText("私人店家")).not.toBeInTheDocument();
    expect(queryClient.getQueryData(["vendors", "personal"])).toEqual([expect.objectContaining({ id: "personal" })]);
    expect(queryClient.getQueryData(["vendors", "s1"])).toEqual([expect.objectContaining({ id: "v1" })]);
    await userEvent.click(screen.getByRole("button", { name: "新增店家" }));
    const dialog = await screen.findByRole("dialog");
    await userEvent.type(within(dialog).getByLabelText("店家名稱"), "共用店家");
    await userEvent.click(within(dialog).getByRole("button", { name: "建立" }));
    await screen.findByRole("heading", { name: "共用店家" });
    expect(posted).toEqual({ name: "共用店家", sharingGroupId: "s1" });
  });

  it("does not offer create or photo import for a member's directory scope", async () => {
    server.use(
      http.get("*/api/v1/sharing-groups", () => HttpResponse.json([{ ...group, role: "member" }])),
      http.get("*/api/v1/rosters", () => HttpResponse.json([])),
    );
    renderApp("/rosters");
    await screen.findByRole("option", { name: /午餐同好/ });
    await userEvent.selectOptions(screen.getByLabelText("資料範圍"), "s1");
    expect(screen.queryByRole("button", { name: "新增名單" })).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: /照片/ })).not.toBeInTheDocument();
    expect(screen.getByText(/唯讀：可查看並選用開團/)).toBeInTheDocument();
  });

  it("hides vendor, menu, archive and import mutations in read-only detail", async () => {
    server.use(http.get("*/api/v1/vendors/v1", () => HttpResponse.json({ id: "v1", name: "共用便當", phone: "123", address: "台北", note: null, sharingGroupId: "s1", canManage: false, menuItems: [{ id: "i1", name: "雞腿飯", price: 100, sortOrder: 1, archived: true }] })));
    renderApp("/vendors/v1");
    await screen.findByText("雞腿飯");
    expect(screen.getByText("台北")).toBeInTheDocument();
    expect(screen.queryByRole("switch")).not.toBeInTheDocument();
    expect(screen.queryByLabelText("批次新增品項")).not.toBeInTheDocument();
    for (const name of [/儲存/, /刪除/, /編輯/, /匯入/]) expect(screen.queryByRole("button", { name })).not.toBeInTheDocument();
  });

  it("fetches group sources for orders and allows read-only shared sources to be selected", async () => {
    server.use(
      http.get("*/api/v1/rosters", ({ request }) => HttpResponse.json(new URL(request.url).searchParams.get("sharingGroupId") === "s1" ? [{ id: "r1", name: "共享來源名單", memberCount: 2, canManage: false }] : [])),
      http.get("*/api/v1/vendors", ({ request }) => HttpResponse.json(new URL(request.url).searchParams.get("sharingGroupId") === "s1" ? [{ id: "v1", name: "共享來源店家", itemCount: 2, canManage: false }] : [])),
      http.get("*/api/v1/delivery-addresses", () => HttpResponse.json([])),
    );
    renderApp("/group-orders/new");
    const roster = await screen.findByRole("button", { name: /共享來源名單.*午餐同好/ });
    expect(roster).toBeEnabled();
    await userEvent.click(roster);
    await userEvent.click(screen.getByRole("button", { name: "下一步" }));
    const vendor = await screen.findByRole("button", { name: /共享來源店家.*午餐同好/ });
    expect(vendor).toBeEnabled();
    await userEvent.click(vendor);
    await userEvent.click(screen.getByRole("button", { name: "下一步" }));
    expect(await screen.findByRole("button", { name: "開團" })).toBeEnabled();
  });
});
