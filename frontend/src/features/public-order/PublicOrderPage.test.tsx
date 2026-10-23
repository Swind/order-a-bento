import { act, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { delay, http, HttpResponse } from "msw";
import { beforeEach, describe, expect, it, vi } from "vitest";
import * as deviceInfo from "@/shared/lib/deviceInfo";
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
const order = (lines: { itemId: string; name: string; quantity: number; note?: string | null }[], cancelled = false, note: string | null = null) => ({
  order: {
    submissionId: "s1",
    submittedAt: "2026-05-01T02:02:00Z",
    cancelled,
    note,
    total: lines.reduce((s, l) => s + l.quantity * 100, 0),
    lines: lines.map((l) => ({ ...l, unitPrice: 100, subtotal: l.quantity * 100, note: l.note ?? null })),
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
        note: l.note,
      }));
      const next = order(lines, lines.length === 0, body.note ?? null);
      orders[params.pid as string] = next;
      return HttpResponse.json(next);
    }),
  );
});

describe("PublicOrderPage", () => {
  it("opens the identity dialog immediately during loading and treats cancellation neutrally", async () => {
    server.use(http.get(`*/api/v1/public/g/${TOKEN}/participants/p2/order`, async () => {
      await delay(150);
      return HttpResponse.json(order([], true));
    }));
    renderApp(`/g/${TOKEN}`);
    await userEvent.click(await screen.findByRole("button", { name: /李大華/ }));
    const dialog = screen.getByRole("dialog", { name: "確認身分" });
    expect(dialog).toHaveTextContent("正在查詢 李大華 的訂單");
    expect(within(dialog).getByRole("button", { name: "是我" })).toBeDisabled();
    expect(await within(dialog).findByText(/你選擇了 李大華/)).toBeInTheDocument();
    expect(dialog).not.toHaveTextContent("已送出訂單");
    await userEvent.click(within(dialog).getByRole("button", { name: "Close" }));
    expect(localStorage.getItem(`bento:last:${TOKEN}`)).toBeNull();
  });
  it("derives dirty state, restores unchanged state, and preserves notes after failure and success", async () => {
    localStorage.setItem(`bento:last:${TOKEN}`, "p1");
    renderApp(`/g/${TOKEN}`);
    await screen.findByRole("button", { name: "訂單已送出" });
    await userEvent.click(screen.getByRole("button", { name: "增加 排骨飯" }));
    expect(screen.getByRole("status")).toHaveTextContent("有尚未送出的修改");
    await userEvent.click(screen.getByRole("button", { name: "減少 排骨飯" }));
    expect(screen.getByRole("button", { name: "訂單已送出" })).toBeDisabled();
    await userEvent.click(screen.getByRole("button", { name: "＋備註" }));
    await userEvent.type(screen.getByLabelText("排骨飯 備註"), " 不辣 ");
    await userEvent.type(screen.getByLabelText("訂單備註（選填）"), " 晚點拿 ");
    submitResponse = () => problem(500, "internal_error");
    await userEvent.click(screen.getByRole("button", { name: "儲存修改" }));
    await screen.findByText("伺服器發生錯誤，請稍後再試");
    expect(screen.getByLabelText("排骨飯 備註")).toHaveValue(" 不辣 ");
    expect(screen.getByRole("status")).toHaveTextContent("有尚未送出的修改");
    submitResponse = null;
    await userEvent.click(screen.getByRole("button", { name: "儲存修改" }));
    await screen.findByRole("button", { name: "訂單已送出" });
    expect(screen.getByLabelText("排骨飯 備註")).toHaveValue("不辣");
    expect(screen.getByLabelText("訂單備註（選填）")).toHaveValue("晚點拿");
  });

  it("locks edits and duplicate submits during client preparation and recovers preparation failure", async () => {
    localStorage.setItem(`bento:last:${TOKEN}`, "p3");
    let reject!: (error: Error) => void;
    const spy = vi.spyOn(deviceInfo, "collectClientInfo").mockImplementationOnce(() => new Promise((_, fail) => { reject = fail; }));
    renderApp(`/g/${TOKEN}`);
    await userEvent.click(await screen.findByRole("button", { name: "增加 排骨飯" }));
    await userEvent.click(screen.getByRole("button", { name: "送出訂單" }));
    expect(screen.getByRole("button", { name: "送出中…" })).toBeDisabled();
    expect(screen.getByRole("button", { name: "增加 排骨飯" })).toBeDisabled();
    expect(screen.getByLabelText("訂單備註（選填）")).toBeDisabled();
    await userEvent.click(screen.getByRole("button", { name: "送出中…" }));
    expect(spy).toHaveBeenCalledTimes(1);
    expect(submitted).toHaveLength(0);
    await act(async () => reject(new Error("準備失敗")));
    await waitFor(() => expect(screen.getByRole("button", { name: "送出訂單" })).toBeEnabled());
    spy.mockRestore();
    await userEvent.click(screen.getByRole("button", { name: "送出訂單" }));
    await screen.findByRole("button", { name: "訂單已送出" });
    expect(submitted).toHaveLength(1);
  });

  it("requeries persisted order on cutoff and never shows unsaved draft or stale cached snapshot", async () => {
    localStorage.setItem(`bento:last:${TOKEN}`, "p1");
    const { queryClient } = renderApp(`/g/${TOKEN}`);
    await userEvent.click(await screen.findByRole("button", { name: "增加 雞腿飯" }));
    const persisted = order([{ itemId: "removed", name: "已下架餐點", quantity: 2, note: "快照備註" }], false, "快照訂單備註");
    persisted.order.lines[0].unitPrice = 777;
    persisted.order.lines[0].subtotal = 1554;
    persisted.order.total = 1554;
    orders.p1 = persisted;
    group = { ...base, deadlineAt: "2000-01-01T00:00:00Z" };
    await act(() => queryClient.invalidateQueries({ queryKey: ["public-group", TOKEN] }));
    expect(await screen.findByText("總額 $1,554")).toBeInTheDocument();
    expect(screen.getByText("已下架餐點 ×2")).toBeInTheDocument();
    expect(screen.getByText("備註：快照備註")).toBeInTheDocument();
    expect(screen.getByText("訂單備註：快照訂單備註")).toBeInTheDocument();
    expect(screen.queryByText("雞腿飯")).not.toBeInTheDocument();
    expect(document.documentElement.style.getPropertyValue("--order-bar-h")).toBe("");
  });

  it("blocks stale cutoff summaries on read failure and supports retry", async () => {
    localStorage.setItem(`bento:last:${TOKEN}`, "p1");
    const { queryClient } = renderApp(`/g/${TOKEN}`);
    await screen.findByRole("button", { name: "訂單已送出" });
    server.use(http.get(`*/api/v1/public/g/${TOKEN}/participants/p1/order`, () => problem(500, "internal_error")));
    group = { ...base, status: "closed", acceptingOrders: false };
    await act(() => queryClient.invalidateQueries({ queryKey: ["public-group", TOKEN] }));
    expect(await screen.findByRole("alert")).toHaveTextContent("無法查詢已送出的訂單");
    expect(screen.queryByText("排骨飯 ×1")).not.toBeInTheDocument();
    server.use(http.get(`*/api/v1/public/g/${TOKEN}/participants/p1/order`, () => HttpResponse.json(orders.p1)));
    await userEvent.click(screen.getByRole("button", { name: "重試" }));
    expect(await screen.findByText("總額 $100")).toBeInTheDocument();
  });

  it.each(["none", "cancelled", "archived"])("shows readonly %s state without menu controls", async (state) => {
    localStorage.setItem(`bento:last:${TOKEN}`, "p1");
    group = { ...base, status: state === "archived" ? "archived" : "closed", acceptingOrders: false };
    if (state === "none") orders.p1 = { order: null };
    if (state === "cancelled") orders.p1 = order([], true);
    renderApp(`/g/${TOKEN}`);
    expect(await screen.findByText(state === "none" ? "你沒有訂餐" : state === "cancelled" ? /你的訂單已取消/ : "總額 $100")).toBeInTheDocument();
    expect(screen.queryByRole("textbox")).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: /增加/ })).not.toBeInTheDocument();
  });

  it("identity dialog retries failed reads, focuses cancel and returns focus on Escape", async () => {
    server.use(http.get(`*/api/v1/public/g/${TOKEN}/participants/p2/order`, () => problem(500, "internal_error")));
    renderApp(`/g/${TOKEN}`);
    const origin = await screen.findByRole("button", { name: /李大華/ });
    await userEvent.click(origin);
    const dialog = await screen.findByRole("dialog", { name: "確認身分" });
    await waitFor(() => expect(within(dialog).getByRole("button", { name: "選錯了" })).toHaveFocus());
    await within(dialog).findByRole("button", { name: "重試" });
    expect(within(dialog).getByRole("button", { name: "是我" })).toBeDisabled();
    expect(dialog).not.toHaveTextContent("已送出訂單");
    server.use(http.get(`*/api/v1/public/g/${TOKEN}/participants/p2/order`, () => HttpResponse.json({ order: null })));
    await userEvent.click(within(dialog).getByRole("button", { name: "重試" }));
    expect(await within(dialog).findByText(/你選擇了 李大華/)).toBeInTheDocument();
    await userEvent.keyboard("{Escape}");
    await waitFor(() => expect(origin).toHaveFocus());
    expect(localStorage.getItem(`bento:last:${TOKEN}`)).toBeNull();
  });

  it("shows a busy state while submitting, then keeps a saved order summary", async () => {
    localStorage.setItem(`bento:last:${TOKEN}`, "p3");
    server.use(http.post(`*/api/v1/public/g/${TOKEN}/participants/p3/submissions`, async () => {
      await delay(200);
      const next = order([{ itemId: "i1", name: "排骨飯", quantity: 1 }]);
      orders.p3 = next;
      return HttpResponse.json(next);
    }));
    renderApp(`/g/${TOKEN}`);
    await userEvent.click(await screen.findByRole("button", { name: "增加 排骨飯" }));
    await userEvent.click(screen.getByRole("button", { name: "送出訂單" }));
    const pending = await screen.findByRole("button", { name: "送出中…" });
    expect(pending).toBeDisabled();
    expect(pending).toHaveAttribute("aria-busy", "true");
    expect(await screen.findByText("排骨飯 ×1")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "訂單已送出" })).toBeDisabled();
  });

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
    expect(screen.getByRole("button", { name: "訂單已送出" })).toBeDisabled();
    await userEvent.click(screen.getByRole("button", { name: "增加 雞腿飯" }));
    expect(screen.getByLabelText("目前選餐合計")).toHaveTextContent("共 2 個$210");
    expect(screen.getByRole("button", { name: "增加 素食便當" })).toBeDisabled();
    await userEvent.click(screen.getByRole("button", { name: "儲存修改" }));
    await waitFor(() => expect(submitted).toHaveLength(1));
    expect(submitted[0].body.lines).toEqual([
      { itemId: "i1", quantity: 1 },
      { itemId: "i2", quantity: 1 },
    ]);
    expect(submitted[0].body.idempotencyKey).toMatch(/^[0-9a-f-]{36}$/);
    expect(submitted[0].body.client.deviceId).toBeTruthy();
    expect(await screen.findByText("已更新訂單")).toBeInTheDocument();
    expect(screen.getByText("排骨飯 ×1、雞腿飯 ×1")).toBeInTheDocument();
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
    await userEvent.click(screen.getByRole("button", { name: "儲存修改" }));
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
    expect(await screen.findByRole("button", { name: "訂單已送出" })).toBeDisabled();
    expect(screen.getByRole("heading", { name: "李大華" })).toHaveFocus();
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
    expect(screen.queryByRole("button", { name: "增加 雞腿飯" })).not.toBeInTheDocument();
  });

  it("an expired open group is read-only even with stale acceptingOrders", async () => {
    group = { ...base, deadlineAt: "2000-01-01T11:00:00+08:00", acceptingOrders: true };
    localStorage.setItem(`bento:last:${TOKEN}`, "p1");
    renderApp(`/g/${TOKEN}`);
    expect(await screen.findByRole("status")).toHaveTextContent("已截止");
    await screen.findByText(/已於 .* 送出/);
    expect(screen.queryByRole("button", { name: "增加 雞腿飯" })).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: /送出訂單|更新訂單|取消我的訂單/ })).not.toBeInTheDocument();
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
    await userEvent.click(screen.getByRole("button", { name: "增加 雞腿飯" }));
    await userEvent.click(screen.getByRole("button", { name: "送出訂單" }));
    expect(await screen.findByRole("button", { name: "訂單已送出" })).toBeDisabled();
    expect(screen.getByText("雞腿飯 ×1")).toBeInTheDocument();
  });

  it("409 deadline_passed switches to read-only", async () => {
    localStorage.setItem(`bento:last:${TOKEN}`, "p3");
    submitResponse = () => problem(409, "deadline_passed");
    renderApp(`/g/${TOKEN}`);
    await userEvent.click(await screen.findByRole("button", { name: "增加 排骨飯" }));
    await userEvent.click(screen.getByRole("button", { name: "送出訂單" }));
    expect(await screen.findByRole("status")).toHaveTextContent("已截止");
    expect(screen.queryByRole("button", { name: "送出訂單" })).not.toBeInTheDocument();
    expect(await screen.findByText("你沒有訂餐")).toBeInTheDocument();
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

  it("preserves quantity and both notes through sold_out, failed group refresh and successful retry", async () => {
    localStorage.setItem(`bento:last:${TOKEN}`, "p3");
    let failGroup = false;
    server.use(http.get(`*/api/v1/public/g/${TOKEN}`, () => failGroup ? problem(500, "internal_error") : HttpResponse.json(group)));
    renderApp(`/g/${TOKEN}`);
    await userEvent.click(await screen.findByRole("button", { name: "增加 排骨飯" }));
    await userEvent.click(screen.getByRole("button", { name: "增加 排骨飯" }));
    await userEvent.click(screen.getByRole("button", { name: "＋備註" }));
    const itemNote = screen.getByLabelText("排骨飯 備註");
    const note = screen.getByLabelText("訂單備註（選填）");
    await userEvent.type(itemNote, "不要辣，保留草稿");
    await userEvent.type(note, "晚點拿，保留訂單備註");
    submitResponse = () => {
      failGroup = true;
      return problem(409, "sold_out");
    };
    await userEvent.click(screen.getByRole("button", { name: "送出訂單" }));
    await screen.findByText(/團單資料更新失敗，保留目前選餐/);
    expect(screen.getByLabelText("排骨飯 備註")).toBe(itemNote);
    expect(screen.getByLabelText("訂單備註（選填）")).toBe(note);
    expect(itemNote).toHaveValue("不要辣，保留草稿");
    expect(note).toHaveValue("晚點拿，保留訂單備註");
    expect(screen.getByRole("group", { name: "排骨飯 數量" })).toHaveTextContent("2");
    group = { ...base, menu: base.menu.map((m) => m.id === "i1" ? { ...m, soldOut: true } : m) };
    failGroup = false;
    await userEvent.click(screen.getByRole("button", { name: "重試" }));
    await screen.findByText("此品項已售完，請改選其他品項");
    expect(screen.queryByText(/團單資料更新失敗/)).not.toBeInTheDocument();
    expect(screen.getByLabelText("排骨飯 備註")).toBe(itemNote);
    expect(screen.getByLabelText("訂單備註（選填）")).toBe(note);
    expect(itemNote).toHaveValue("不要辣，保留草稿");
    expect(note).toHaveValue("晚點拿，保留訂單備註");
    expect(screen.getByRole("group", { name: "排骨飯 數量" })).toHaveTextContent("2");
    expect(screen.getByRole("button", { name: "送出訂單" })).toBeDisabled();
    await userEvent.click(screen.getByRole("button", { name: "減少 排骨飯" }));
    expect(screen.getByRole("group", { name: "排骨飯 數量" })).toHaveTextContent("1");
  });

  it.each([403, 404])("blocks cached ordering after an authoritative background %s", async (status) => {
    localStorage.setItem(`bento:last:${TOKEN}`, "p1");
    const { queryClient } = renderApp(`/g/${TOKEN}`);
    await userEvent.click(await screen.findByRole("button", { name: "增加 排骨飯" }));
    expect(screen.getByRole("button", { name: "儲存修改" })).toBeEnabled();
    server.use(http.get(`*/api/v1/public/g/${TOKEN}`, () => problem(status, status === 404 ? "not_found" : "forbidden")));
    await act(() => queryClient.invalidateQueries({ queryKey: ["public-group", TOKEN] }));
    expect(await screen.findByRole("alert")).toHaveTextContent(status === 404 ? "這個訂購連結無效或已失效" : "你目前無權限查看或使用這個訂購連結");
    // Cached data still exists: authorization, not the absence of a cache, must block the UI.
    expect(queryClient.getQueryData(["public-group", TOKEN])).toEqual(base);
    expect(screen.getByText("請向開團的人確認連結及存取權限。")).toBeInTheDocument();
    expect(screen.queryByText(/保留目前選餐/)).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: /儲存修改|送出訂單|取消我的訂單|確定取消/ })).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: /增加|減少/ })).not.toBeInTheDocument();
    expect(screen.queryByRole("textbox")).not.toBeInTheDocument();
    expect(document.documentElement.style.getPropertyValue("--order-bar-h")).toBe("");
    expect(submitted).toHaveLength(0);
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
