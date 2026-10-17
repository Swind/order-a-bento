import { screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { http, HttpResponse } from "msw";
import { beforeEach, describe, expect, it } from "vitest";
import { renderApp } from "@/test/render";
import { problem, server } from "@/test/server";

const TOKEN = "tok";
const base = {
  title: "週五午餐",
  status: "open",
  acceptingOrders: true,
  deadlineAt: null as string | null,
  note: null,
  vendor: { name: "好吃便當", phone: "02-1234-5678", address: null },
  participants: [
    { id: "p1", name: "王小明", hasOrdered: true },
    { id: "p2", name: "李大華", hasOrdered: true },
    { id: "p3", name: "陳美麗", hasOrdered: false },
  ],
  menu: [
    { id: "i1", name: "排骨飯", price: 100, description: null, category: null, soldOut: false },
    { id: "i2", name: "雞腿飯", price: 110, description: null, category: null, soldOut: false },
    { id: "i3", name: "素食便當", price: 90, description: null, category: null, soldOut: true },
  ],
};
const order = (lines: { itemId: string; name: string; quantity: number }[], cancelled = false) => ({
  order: {
    submissionId: "s1",
    submittedAt: "2026-05-01T02:02:00Z",
    cancelled,
    note: null,
    total: lines.reduce((s, l) => s + l.quantity * 100, 0),
    lines: lines.map((l) => ({ ...l, unitPrice: 100, subtotal: l.quantity * 100, note: null })),
  },
});

let group = base;
let orders: Record<string, unknown> = {};
let submitted: { pid: string; body: Record<string, any> }[] = [];
let submitResponse: (() => Response) | null = null;

beforeEach(() => {
  localStorage.clear();
  group = base;
  orders = { p1: order([{ itemId: "i1", name: "排骨飯", quantity: 1 }]), p2: order([{ itemId: "i2", name: "雞腿飯", quantity: 1 }]), p3: { order: null } };
  submitted = [];
  submitResponse = null;
  server.use(
    http.get(`*/api/v1/public/g/${TOKEN}`, () => HttpResponse.json(group)),
    http.get(`*/api/v1/public/g/${TOKEN}/participants/:pid/order`, ({ params }) =>
      HttpResponse.json(orders[params.pid as string]),
    ),
    http.post(`*/api/v1/public/g/${TOKEN}/participants/:pid/submissions`, async ({ request, params }) => {
      const body = (await request.json()) as Record<string, any>;
      submitted.push({ pid: params.pid as string, body });
      if (submitResponse) return submitResponse();
      const lines = (body.lines as any[]).map((l) => ({
        itemId: l.itemId,
        name: group.menu.find((m) => m.id === l.itemId)!.name,
        quantity: l.quantity,
      }));
      const next = order(lines, lines.length === 0);
      orders[params.pid as string] = next;
      return HttpResponse.json(next);
    }),
  );
});

describe("PublicOrderPage", () => {
  it("shows a friendly message for an unknown link", async () => {
    server.use(http.get(`*/api/v1/public/g/${TOKEN}`, () => problem(404, "not_found")));
    renderApp(`/g/${TOKEN}`);
    expect(await screen.findByText("這個訂購連結無效或已失效")).toBeInTheDocument();
  });

  it("filters participants by width/case-insensitive search and shows privacy footer", async () => {
    renderApp(`/g/${TOKEN}`);
    await screen.findByText("陳美麗");
    expect(screen.getByText("為核對訂單，系統會記錄 IP 與裝置資訊")).toBeInTheDocument();
    expect(screen.getByRole("link", { name: "02-1234-5678" })).toHaveAttribute("href", "tel:02-1234-5678");
    await userEvent.type(screen.getByLabelText("搜尋名字"), "美");
    expect(screen.queryByText("王小明")).not.toBeInTheDocument();
    expect(screen.getByText("陳美麗")).toBeInTheDocument();
  });

  it("prefills the existing order, submits a changed order and shows the result", async () => {
    localStorage.setItem(`bento:last:${TOKEN}`, "p1");
    renderApp(`/g/${TOKEN}`);
    expect(await screen.findByText(/已於 .* 送出/)).toBeInTheDocument();
    expect(screen.getByRole("group", { name: "排骨飯 數量" })).toHaveTextContent("1");
    expect(screen.getByRole("button", { name: "更新訂單（取代上一次）" })).toBeInTheDocument();
    await userEvent.click(screen.getByRole("button", { name: "增加 雞腿飯" }));
    expect(screen.getByText("共 2 個 $210")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "增加 素食便當" })).toBeDisabled();
    await userEvent.click(screen.getByRole("button", { name: "更新訂單（取代上一次）" }));
    await waitFor(() => expect(submitted).toHaveLength(1));
    expect(submitted[0].body.lines).toEqual([
      { itemId: "i1", quantity: 1 },
      { itemId: "i2", quantity: 1 },
    ]);
    expect(submitted[0].body.idempotencyKey).toMatch(/^[0-9a-f-]{36}$/);
    expect(submitted[0].body.client.deviceId).toBeTruthy();
    expect(await screen.findByText("已更新訂單")).toBeInTheDocument();
  });

  it("reuses the idempotency key on retry of the same payload, renews it after success", async () => {
    localStorage.setItem(`bento:last:${TOKEN}`, "p3");
    renderApp(`/g/${TOKEN}`);
    await userEvent.click(await screen.findByRole("button", { name: "增加 排骨飯" }));
    submitResponse = () => problem(500, "internal_error");
    await userEvent.click(screen.getByRole("button", { name: "送出訂單" }));
    await waitFor(() => expect(submitted).toHaveLength(1));
    await screen.findByText("伺服器發生錯誤，請稍後再試");
    submitResponse = null;
    await waitFor(() => expect(screen.getByRole("button", { name: "送出訂單" })).toBeEnabled());
    await userEvent.click(screen.getByRole("button", { name: "送出訂單" }));
    await waitFor(() => expect(submitted).toHaveLength(2));
    expect(submitted[1].body.idempotencyKey).toBe(submitted[0].body.idempotencyKey);
    await screen.findByText(/已於 .* 送出/);
    await userEvent.click(screen.getByRole("button", { name: "增加 排骨飯" }));
    await userEvent.click(screen.getByRole("button", { name: "更新訂單（取代上一次）" }));
    await waitFor(() => expect(submitted).toHaveLength(3));
    expect(submitted[2].body.idempotencyKey).not.toBe(submitted[1].body.idempotencyKey);
  });

  it("asks for confirmation before opening someone else's existing order", async () => {
    renderApp(`/g/${TOKEN}`);
    await userEvent.click(await screen.findByRole("button", { name: /李大華/ }));
    expect(await screen.findByText(/李大華 已於 .* 送出訂單，你確定是本人嗎？/)).toBeInTheDocument();
    await userEvent.click(screen.getByRole("button", { name: "選錯了" }));
    expect(screen.queryByText(/你確定是本人嗎/)).not.toBeInTheDocument();
    await userEvent.click(screen.getByRole("button", { name: /李大華/ }));
    await userEvent.click(await screen.findByRole("button", { name: "是我" }));
    expect(await screen.findByRole("button", { name: "更新訂單（取代上一次）" })).toBeInTheDocument();
    expect(localStorage.getItem(`bento:last:${TOKEN}`)).toBe("p2");
  });

  it("remembered participant skips step 1; 換人 returns to the list", async () => {
    localStorage.setItem(`bento:last:${TOKEN}`, "p3");
    renderApp(`/g/${TOKEN}`);
    expect(await screen.findByRole("button", { name: "送出訂單" })).toBeInTheDocument();
    expect(screen.queryByLabelText("搜尋名字")).not.toBeInTheDocument();
    await userEvent.click(screen.getByRole("button", { name: "換人" }));
    expect(await screen.findByLabelText("搜尋名字")).toBeInTheDocument();
  });

  it("closed group is read-only with a banner", async () => {
    group = { ...base, status: "closed", acceptingOrders: false };
    localStorage.setItem(`bento:last:${TOKEN}`, "p1");
    renderApp(`/g/${TOKEN}`);
    expect(await screen.findByRole("status")).toHaveTextContent("已關閉");
    await screen.findByText(/已於 .* 送出/);
    expect(screen.queryByRole("button", { name: /送出訂單|更新訂單/ })).not.toBeInTheDocument();
    expect(screen.getByRole("button", { name: "增加 雞腿飯" })).toBeDisabled();
  });

  it("cancel sends empty lines after inline confirmation", async () => {
    localStorage.setItem(`bento:last:${TOKEN}`, "p1");
    renderApp(`/g/${TOKEN}`);
    await userEvent.click(await screen.findByRole("button", { name: "取消我的訂單" }));
    await userEvent.click(screen.getByRole("button", { name: "確定取消" }));
    await waitFor(() => expect(submitted).toHaveLength(1));
    expect(submitted[0].body.lines).toEqual([]);
    expect(await screen.findByText("已取消你的訂單")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "送出訂單" })).toBeInTheDocument();
  });

  it("409 deadline_passed switches to read-only", async () => {
    localStorage.setItem(`bento:last:${TOKEN}`, "p3");
    submitResponse = () => problem(409, "deadline_passed");
    renderApp(`/g/${TOKEN}`);
    await userEvent.click(await screen.findByRole("button", { name: "增加 排骨飯" }));
    await userEvent.click(screen.getByRole("button", { name: "送出訂單" }));
    expect(await screen.findByRole("status")).toHaveTextContent("已截止");
    expect(screen.queryByRole("button", { name: "送出訂單" })).not.toBeInTheDocument();
  });

  it("sold_out refetches the menu and highlights the item", async () => {
    localStorage.setItem(`bento:last:${TOKEN}`, "p3");
    renderApp(`/g/${TOKEN}`);
    await userEvent.click(await screen.findByRole("button", { name: "增加 排骨飯" }));
    group = { ...base, menu: base.menu.map((m) => (m.id === "i1" ? { ...m, soldOut: true } : m)) };
    submitResponse = () => problem(409, "sold_out", { field: "offeredItemId" });
    await userEvent.click(screen.getByRole("button", { name: "送出訂單" }));
    expect(await screen.findByText("此品項已售完，請改選其他品項")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "送出訂單" })).toBeDisabled();
  });

  it("shows an empty state when the roster has no participants", async () => {
    group = { ...base, participants: [] };
    renderApp(`/g/${TOKEN}`);
    expect(await screen.findByText("名單裡還沒有人")).toBeInTheDocument();
    expect(screen.getByText("為核對訂單，系統會記錄 IP 與裝置資訊")).toBeInTheDocument();
  });

  it("shows an empty state when the menu has no items", async () => {
    localStorage.setItem(`bento:last:${TOKEN}`, "p3");
    group = { ...base, menu: [] };
    renderApp(`/g/${TOKEN}`);
    expect(await screen.findByText("這個團還沒有菜單品項")).toBeInTheDocument();
  });

  it("reserves bottom space for the sticky bar via --order-bar-h", async () => {
    localStorage.setItem(`bento:last:${TOKEN}`, "p3");
    renderApp(`/g/${TOKEN}`);
    await screen.findByRole("button", { name: "送出訂單" });
    expect(document.documentElement.style.getPropertyValue("--order-bar-h")).toMatch(/px$/);
  });
});
