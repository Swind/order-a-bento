import { screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { http, HttpResponse } from "msw";
import { beforeEach, describe, expect, it } from "vitest";
import { renderApp } from "@/test/render";
import { ME, problem, server } from "@/test/server";

const detail = {
  canManage: true,
  sharingGroupId: null,
  sharingGroupName: null,
  id: "g1",
  title: "T",
  status: "open",
  deadlineAt: null,
  note: null,
  shareUrl: "https://x.test/g/tok",
  vendor: { name: "好吃便當", phone: null, address: null },
  participants: [],
  offeredItems: [],
  createdAt: "2026-01-01T00:00:00Z",
  closedAt: null,
};

beforeEach(() =>
  server.use(
    http.get("*/api/v1/me", () => HttpResponse.json(ME)),
    http.get("*/api/v1/delivery-addresses", () => HttpResponse.json([{ id: "a1", name: "台北教室", address: "台北市 3 樓", updatedAt: "2026-01-01T00:00:00Z" }])),
    http.get("*/api/v1/rosters", () =>
      HttpResponse.json([
        { id: "r0", name: "空名單", memberCount: 0, updatedAt: "2026-01-01T00:00:00Z" },
        { id: "r1", name: "業務部", memberCount: 5, updatedAt: "2026-01-01T00:00:00Z" },
      ]),
    ),
    http.get("*/api/v1/vendors", () =>
      HttpResponse.json([
        { id: "v0", name: "空店", itemCount: 0, updatedAt: "2026-01-01T00:00:00Z" },
        { id: "v1", name: "好吃便當", itemCount: 3, updatedAt: "2026-01-01T00:00:00Z" },
      ]),
    ),
  ),
);

describe("NewGroupOrderPage", () => {
  it("disables empty rosters and vendors", async () => {
    renderApp("/group-orders/new");
    expect(await screen.findByRole("button", { name: /空名單/ })).toBeDisabled();
    await userEvent.click(screen.getByRole("button", { name: /業務部/ }));
    await userEvent.click(screen.getByRole("button", { name: "下一步" }));
    expect(await screen.findByRole("button", { name: /空店/ })).toBeDisabled();
    expect(screen.getByRole("button", { name: "下一步" })).toBeDisabled();
  });

  it("walks the 3 steps, posts the body and navigates to the group", async () => {
    let posted: Record<string, unknown> | undefined;
    server.use(
      http.post("*/api/v1/group-orders", async ({ request }) => {
        posted = (await request.json()) as Record<string, unknown>;
        return HttpResponse.json(detail, { status: 201 });
      }),
      http.get("*/api/v1/group-orders/g1", () => HttpResponse.json(detail)),
    );
    const { router } = renderApp("/group-orders/new");
    await userEvent.click(await screen.findByRole("button", { name: /業務部/ }));
    await userEvent.click(screen.getByRole("button", { name: "下一步" }));
    await userEvent.click(await screen.findByRole("button", { name: /好吃便當/ }));
    await userEvent.click(screen.getByRole("button", { name: "下一步" }));
    const title = await screen.findByLabelText("標題");
    await screen.findByRole("option", { name: "台北教室 — 台北市 3 樓" });
    await userEvent.selectOptions(screen.getByLabelText("送餐地址（選填）"), "a1");
    expect((title as HTMLInputElement).value).toMatch(/^\d+\/\d+ 午餐 · 好吃便當$/);
    await userEvent.clear(title);
    await userEvent.type(title, "週五午餐");
    await userEvent.click(screen.getByRole("button", { name: "1 小時後" }));
    const deadline = (screen.getByLabelText("截止時間（選填）") as HTMLInputElement).value;
    await userEvent.type(screen.getByLabelText("備註（選填）"), "保留備註");
    expect(screen.getByRole("region", { name: "開團選擇摘要" })).toHaveTextContent("名單：業務部 · 5 人");
    expect(screen.getByRole("region", { name: "開團選擇摘要" })).toHaveTextContent("店家：好吃便當");
    expect(screen.getByRole("region", { name: "開團選擇摘要" })).toHaveTextContent("開團歸屬：個人");
    await userEvent.click(screen.getByRole("button", { name: "修改名單" }));
    await userEvent.click(screen.getByRole("button", { name: "下一步" }));
    await userEvent.click(screen.getByRole("button", { name: "下一步" }));
    await userEvent.click(screen.getByRole("button", { name: "修改店家" }));
    await userEvent.click(screen.getByRole("button", { name: "下一步" }));
    expect(screen.getByLabelText("標題")).toHaveValue("週五午餐");
    expect(screen.getByLabelText("截止時間（選填）")).toHaveValue(deadline);
    expect(screen.getByLabelText("備註（選填）")).toHaveValue("保留備註");
    expect(screen.getByLabelText("送餐地址（選填）")).toHaveValue("a1");
    await userEvent.click(screen.getByRole("button", { name: "開團" }));
    expect(await screen.findByText("分享連結")).toBeInTheDocument();
    expect(router.state.location.pathname).toBe("/group-orders/g1");
    expect(posted).toMatchObject({ title: "週五午餐", rosterId: "r1", vendorId: "v1", deliveryAddressId: "a1" });
    expect(posted).not.toHaveProperty("sharingGroupId");
    const delta = new Date(posted!.deadlineAt as string).getTime() - Date.now();
    expect(delta).toBeGreaterThan(55 * 60_000);
    expect(delta).toBeLessThan(65 * 60_000);
  });

  it("shows server field errors for the title", async () => {
    server.use(
      http.post("*/api/v1/group-orders", () => problem(422, "invalid_title", { field: "title" })),
    );
    renderApp("/group-orders/new");
    await userEvent.click(await screen.findByRole("button", { name: /業務部/ }));
    await userEvent.click(screen.getByRole("button", { name: "下一步" }));
    await userEvent.click(await screen.findByRole("button", { name: /好吃便當/ }));
    await userEvent.click(screen.getByRole("button", { name: "下一步" }));
    await userEvent.click(await screen.findByRole("button", { name: "開團" }));
    expect(await screen.findByText("標題格式不正確")).toBeInTheDocument();
  });

  it.each(["", "s1"])("keeps ownership %s independent of shared directory sources", async (ownership) => {
    let posted: Record<string, unknown> | undefined;
    server.use(
      http.get("*/api/v1/sharing-groups", () => HttpResponse.json([{ id: "s1", name: "共同教室", role: "member" }])),
      http.get("*/api/v1/rosters", ({ request }) => HttpResponse.json(
        new URL(request.url).searchParams.has("sharingGroupId")
          ? [{ id: "shared-r", name: "共用名單", memberCount: 2 }] : [],
      )),
      http.get("*/api/v1/vendors", ({ request }) => HttpResponse.json(
        new URL(request.url).searchParams.has("sharingGroupId")
          ? [] : [{ id: "personal-v", name: "私人店家", itemCount: 1 }],
      )),
      http.post("*/api/v1/group-orders", async ({ request }) => {
        posted = await request.json() as Record<string, unknown>;
        return HttpResponse.json(detail, { status: 201 });
      }),
      http.get("*/api/v1/group-orders/g1", () => HttpResponse.json(detail)),
    );
    renderApp("/group-orders/new");
    expect(await screen.findByLabelText("開團歸屬")).toHaveValue("");
    await screen.findByRole("option", { name: "共同教室（成員）" });
    await userEvent.selectOptions(screen.getByLabelText("開團歸屬"), ownership);
    await userEvent.click(await screen.findByRole("button", { name: /共用名單/ }));
    await userEvent.click(screen.getByRole("button", { name: "下一步" }));
    await userEvent.click(await screen.findByRole("button", { name: /私人店家/ }));
    await userEvent.click(screen.getByRole("button", { name: "下一步" }));
    expect(screen.getByRole("region", { name: "開團選擇摘要" })).toHaveTextContent(`開團歸屬：${ownership ? "共用群組 · 共同教室" : "個人"}`);
    await userEvent.click(screen.getByRole("button", { name: "開團" }));
    await screen.findByText("分享連結");
    expect(posted).toMatchObject({ rosterId: "shared-r", vendorId: "personal-v" });
    if (ownership) expect(posted).toHaveProperty("sharingGroupId", ownership);
    else expect(posted).not.toHaveProperty("sharingGroupId");
  });

  it("refreshes lost sharing access without silently changing ownership", async () => {
    let accessible = true;
    server.use(
      http.get("*/api/v1/sharing-groups", () => HttpResponse.json(accessible ? [{ id: "s1", name: "共同教室", role: "member" }] : [])),
      http.get("*/api/v1/rosters", ({ request }) => HttpResponse.json(
        new URL(request.url).searchParams.has("sharingGroupId") ? [] : [{ id: "r1", name: "業務部", memberCount: 5 }],
      )),
      http.get("*/api/v1/vendors", ({ request }) => HttpResponse.json(
        new URL(request.url).searchParams.has("sharingGroupId") ? [] : [{ id: "v1", name: "好吃便當", itemCount: 3 }],
      )),
      http.post("*/api/v1/group-orders", () => {
        accessible = false;
        return problem(404, "not_found");
      }),
    );
    renderApp("/group-orders/new");
    await screen.findByRole("option", { name: "共同教室（成員）" });
    await userEvent.selectOptions(screen.getByLabelText("開團歸屬"), "s1");
    await userEvent.click(await screen.findByRole("button", { name: /業務部/ }));
    await userEvent.click(screen.getByRole("button", { name: "下一步" }));
    await userEvent.click(await screen.findByRole("button", { name: /好吃便當/ }));
    await userEvent.click(screen.getByRole("button", { name: "下一步" }));
    await userEvent.click(screen.getByRole("button", { name: "開團" }));
    expect(await screen.findByText("共用群組或選用資料已無法存取，請確認開團歸屬並重新選擇資料")).toBeInTheDocument();
    expect(await screen.findByRole("option", { name: /原共用群組/ })).toBeDisabled();
    expect(screen.getByLabelText("開團歸屬")).toHaveValue("s1");
    expect(screen.getByRole("button", { name: "下一步" })).toBeDisabled();
  });

  it("dashboard links to the wizard", async () => {
    renderApp("/");
    expect(await screen.findByRole("link", { name: /開團$/ })).toHaveAttribute("href", "/group-orders/new");
  });
});
