import { act, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { http, HttpResponse } from "msw";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { renderApp } from "@/test/render";
import { problem, server } from "@/test/server";
import { effectiveOrderKey, publicGroupKey, publicOrdersKey } from "./hooks";

const token = "overview";
const url = `*/api/v1/public/g/${token}`;
const base = {
  title: "午餐", status: "open", acceptingOrders: true, deadlineAt: null as string | null, note: null,
  vendor: { name: "店家", phone: null, address: null },
  participants: [{ id: "p1", name: "小明", hasOrdered: false }],
  menu: [{ id: "i1", name: "排骨飯", price: 100, description: null, category: null, soldOut: false }],
};
const saved = { order: {
  submissionId: "s1", submittedAt: "2026-05-01T02:02:00Z", cancelled: false, note: "私密整單備註", total: 100,
  lines: [{ itemId: "i1", name: "排骨飯", quantity: 1, unitPrice: 100, subtotal: 100, note: "私密餐點備註" }],
} };
const snapshot = (quantity = 1) => ({
  status: "open", acceptingOrders: true, deadlineAt: null as string | null,
  orders: [{ participantId: "p1", name: "小明", submittedAt: "2026-05-01T02:02:00Z",
    lines: [{ itemId: "i1", name: "排骨飯", quantity }] }],
});
let group = base;
let overview = snapshot();
let reads = 0;
let groupReads = 0;
let posts: Record<string, unknown>[] = [];

function deferred() {
  let resolve!: () => void;
  const promise = new Promise<void>((done) => { resolve = done; });
  return { promise, resolve };
}
const ownTab = () => screen.getByRole("tab", { name: "我要點餐" });
const overviewTab = () => screen.getByRole("tab", { name: "大家訂了什麼" });
const openOverview = async () => { await userEvent.click(await screen.findByRole("tab", { name: "大家訂了什麼" })); };

beforeEach(() => {
  localStorage.clear();
  group = base;
  overview = snapshot();
  reads = groupReads = 0;
  posts = [];
  server.use(
    http.get(url, () => { groupReads++; return HttpResponse.json(group); }),
    http.get(`${url}/orders`, () => { reads++; return HttpResponse.json(overview); }),
    http.get(`${url}/participants/p1/order`, () => HttpResponse.json({ order: null })),
    http.post(`${url}/participants/p1/submissions`, async ({ request }) => {
      posts.push(await request.json() as Record<string, unknown>);
      return problem(500, "internal_error");
    }),
  );
});
afterEach(() => { vi.useRealTimers(); vi.restoreAllMocks(); vi.unstubAllGlobals(); });

it("needs no identity, renders only safe fields, handles empty data and refreshes group on every entry/manual read", async () => {
  // Extra wire fields must not accidentally be rendered by the public projection component.
  server.use(http.get(`${url}/orders`, () => {
    reads++;
    return HttpResponse.json({ ...overview, note: "私密整單備註", client: "私密裝置", paid: "私密付款", price: 999,
      orders: overview.orders.map((order) => ({ ...order, note: "私密餐點備註", total: 999 })) });
  }));
  renderApp(`/g/${token}`);
  await screen.findByRole("button", { name: "小明" });
  expect(reads).toBe(0);
  await openOverview();
  const panel = screen.getByRole("tabpanel", { name: "大家訂了什麼" });
  expect(await within(panel).findByText("排骨飯 ×1")).toBeInTheDocument();
  expect(panel).toHaveTextContent("持有此訂購連結的人都能查看已送出的姓名與餐點");
  expect(panel).toHaveTextContent("資料最後成功更新");
  expect(panel).not.toHaveTextContent(/私密|999|\$|訂單備註/);
  expect(within(panel).queryByRole("button", { name: "小明" })).not.toBeInTheDocument();
  expect(within(panel).getAllByRole("button")).toHaveLength(1);
  expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
  await userEvent.click(ownTab());
  await openOverview();
  await waitFor(() => expect(reads).toBe(2));
  overview = { ...snapshot(), orders: [] };
  await userEvent.click(screen.getByRole("button", { name: "重新整理" }));
  expect(await screen.findByText("目前還沒有人訂餐")).toBeInTheDocument();
  expect(reads).toBe(3);
  await waitFor(() => expect(groupReads).toBe(4));
});

it("keeps draft, expanded notes and retry idempotency across tabs, cleans up the bar and does not steal tab focus", async () => {
  const barObservers: { disconnect: ReturnType<typeof vi.fn> }[] = [];
  vi.stubGlobal("ResizeObserver", class {
    disconnect = vi.fn();
    observe(element: Element) {
      if (element.getAttribute("aria-label") === "訂單送出操作") barObservers.push(this);
    }
  });
  localStorage.setItem(`bento:last:${token}`, "p1");
  renderApp(`/g/${token}`);
  await userEvent.click(await screen.findByRole("button", { name: "增加 排骨飯" }));
  await userEvent.click(screen.getByRole("button", { name: "＋備註" }));
  await userEvent.type(screen.getByLabelText("排骨飯 備註"), "不辣");
  await userEvent.type(screen.getByLabelText("訂單備註（選填）"), "晚點拿");
  await userEvent.click(screen.getByRole("button", { name: "送出訂單" }));
  await waitFor(() => expect(posts).toHaveLength(1));
  await waitFor(() => expect(screen.getByRole("button", { name: "送出訂單" })).toBeEnabled());
  const note = screen.getByLabelText("排骨飯 備註");
  expect(document.documentElement.style.getPropertyValue("--order-bar-h")).toMatch(/px$/);
  expect(barObservers).toHaveLength(1);
  await openOverview();
  await screen.findByText("排骨飯 ×1");
  expect(document.documentElement.style.getPropertyValue("--order-bar-h")).toBe("");
  expect(document.querySelector('[aria-label="訂單送出操作"]')).toBeNull();
  expect(barObservers[0].disconnect).toHaveBeenCalledTimes(1);
  expect(document.querySelector('[data-slot="tabs-content"][hidden]')).toHaveAttribute("inert");
  expect(overviewTab()).toHaveFocus();
  await userEvent.click(ownTab());
  expect(ownTab()).toHaveFocus();
  expect(screen.getByLabelText("排骨飯 備註")).toBe(note);
  expect(note).toHaveValue("不辣");
  expect(screen.getByLabelText("訂單備註（選填）")).toHaveValue("晚點拿");
  expect(screen.getByRole("group", { name: "排骨飯 數量" })).toHaveTextContent("1");
  expect(document.documentElement.style.getPropertyValue("--order-bar-h")).toMatch(/px$/);
  expect(barObservers).toHaveLength(2);
  await userEvent.click(screen.getByRole("button", { name: "送出訂單" }));
  await waitFor(() => expect(posts).toHaveLength(2));
  expect(posts[1].idempotencyKey).toBe(posts[0].idempotencyKey);
});

it("shows loading and stale successful data/time after 5xx, not a false empty state", async () => {
  renderApp(`/g/${token}`);
  await openOverview();
  await screen.findByText("排骨飯 ×1");
  const time = screen.getByText(/資料最後成功更新/).querySelector("time")!.dateTime;
  const pending = deferred();
  server.use(http.get(`${url}/orders`, async () => { await pending.promise; return problem(500, "internal_error"); }));
  await userEvent.click(screen.getByRole("button", { name: "重新整理" }));
  expect(screen.getByRole("status")).toHaveTextContent("正在更新，暫顯示上次資料");
  expect(screen.getByText("排骨飯 ×1")).toBeInTheDocument();
  await act(async () => pending.resolve());
  expect(await screen.findByRole("alert", {}, { timeout: 2500 })).toHaveTextContent("上次成功取得的舊資料");
  expect(screen.getByText(/資料最後成功更新/).querySelector("time")!.dateTime).toBe(time);
  expect(screen.queryByText("目前還沒有人訂餐")).not.toBeInTheDocument();
  server.use(http.get(`${url}/orders`, () => HttpResponse.json({ ...snapshot(), orders: [] })));
  await userEvent.click(screen.getByRole("button", { name: "重新整理" }));
  expect(await screen.findByText("目前還沒有人訂餐")).toBeInTheDocument();
});

it("polls visible active overview every 30s, pauses hidden/inactive and refreshes on visibility return", async () => {
  const visibility = vi.spyOn(document, "visibilityState", "get").mockReturnValue("visible");
  vi.useFakeTimers({ toFake: ["setInterval", "clearInterval", "Date"] });
  renderApp(`/g/${token}`);
  await openOverview();
  await screen.findByText("排骨飯 ×1");
  await act(async () => { vi.advanceTimersByTime(29_999); });
  expect(reads).toBe(1);
  await act(async () => { vi.advanceTimersByTime(1); });
  await waitFor(() => expect(reads).toBe(2));
  await waitFor(() => expect(screen.getByRole("button", { name: "重新整理" })).toBeEnabled());
  visibility.mockReturnValue("hidden");
  act(() => document.dispatchEvent(new Event("visibilitychange")));
  await act(async () => { vi.advanceTimersByTime(90_000); });
  expect(reads).toBe(2);
  visibility.mockReturnValue("visible");
  act(() => document.dispatchEvent(new Event("visibilitychange")));
  await waitFor(() => expect(reads).toBe(3));
  await userEvent.click(ownTab());
  await act(async () => { vi.advanceTimersByTime(90_000); });
  expect(reads).toBe(3);
});

it.each(["cancel", "identity"])("removes hidden %s portal without moving focus back into the hidden panel", async (dialog) => {
  group = { ...base, participants: [{ id: "p1", name: "小明", hasOrdered: true }] };
  server.use(http.get(`${url}/participants/p1/order`, () => HttpResponse.json(saved)));
  if (dialog === "cancel") localStorage.setItem(`bento:last:${token}`, "p1");
  renderApp(`/g/${token}`);
  const tab = await screen.findByRole("tab", { name: "大家訂了什麼" });
  await userEvent.click(await screen.findByRole("button", { name: dialog === "cancel" ? "取消我的訂單" : /小明/ }));
  await screen.findByRole("dialog");
  // A real modal blocks the tab; simulate an external tab state transition for portal cleanup.
  act(() => tab.click());
  await screen.findByText("排骨飯 ×1");
  await waitFor(() => expect(document.querySelector('[data-slot="dialog-overlay"]')).toBeNull());
  expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
  expect(document.activeElement?.closest('[data-slot="tabs-content"][hidden]')).toBeNull();
  await userEvent.click(ownTab());
  expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
});

it("keeps deadline protection alive in a hidden own panel and never reopens on a stale open response", async () => {
  localStorage.setItem(`bento:last:${token}`, "p1");
  vi.useFakeTimers({ toFake: ["setInterval", "clearInterval", "Date"] });
  group = { ...base, deadlineAt: new Date(Date.now() + 40_000).toISOString() };
  renderApp(`/g/${token}`);
  await userEvent.click(await screen.findByRole("button", { name: "增加 排骨飯" }));
  await openOverview();
  await screen.findByText("排骨飯 ×1");
  await act(async () => { vi.advanceTimersByTime(60_000); });
  await waitFor(() => expect(screen.getByRole("status")).toHaveTextContent("已截止"));
  group = base;
  await userEvent.click(screen.getByRole("button", { name: "重新整理" }));
  await userEvent.click(ownTab());
  expect(await screen.findByText("你沒有訂餐")).toBeInTheDocument();
  expect(screen.queryByRole("button", { name: "增加 排骨飯" })).not.toBeInTheDocument();
  expect(document.documentElement.style.getPropertyValue("--order-bar-h")).toBe("");
});

it.each(["group", "overview", "effective", "identity", "readonly", "submit"].flatMap((entry) =>
  [401, 403, 404].map((status) => ({ entry, status }))))("latches $status from $entry and clears token cache", async ({ entry, status }) => {
  if (entry !== "identity") localStorage.setItem(`bento:last:${token}`, "p1");
  if (entry === "identity") group = { ...base, participants: [{ id: "p1", name: "小明", hasOrdered: true }] };
  if (entry === "readonly") group = { ...base, acceptingOrders: false, status: "closed" };
  const { queryClient } = renderApp(`/g/${token}`);
  await screen.findByRole("tab", { name: "大家訂了什麼" });
  const failure = () => problem(status, "not_found");
  if (entry === "group") {
    server.use(http.get(url, failure));
    await act(() => queryClient.invalidateQueries({ queryKey: publicGroupKey(token) }));
  } else if (entry === "overview") {
    server.use(http.get(`${url}/orders`, failure));
    await openOverview();
  } else if (entry === "submit") {
    server.use(http.post(`${url}/participants/p1/submissions`, failure));
    await userEvent.click(await screen.findByRole("button", { name: "增加 排骨飯" }));
    await userEvent.click(screen.getByRole("button", { name: "送出訂單" }));
  } else {
    server.use(http.get(`${url}/participants/p1/order`, failure));
    if (entry === "identity") await userEvent.click(screen.getByRole("button", { name: /小明/ }));
    else await act(() => queryClient.invalidateQueries({ queryKey: effectiveOrderKey(token, "p1") }));
  }
  await screen.findByText("請向開團的人確認連結及存取權限。");
  await waitFor(() => expect(queryClient.getQueryData(publicGroupKey(token))).toBeUndefined());
  expect(screen.queryByRole("tab")).not.toBeInTheDocument();
  expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
  expect(document.documentElement.style.getPropertyValue("--order-bar-h")).toBe("");
  act(() => {
    queryClient.setQueryData(publicGroupKey(token), base);
    queryClient.setQueryData(publicOrdersKey(token), snapshot());
  });
  expect(screen.queryByRole("tab")).not.toBeInTheDocument();
});

it.each([401, 403, 404])("blocks both tabs on overview %s without retrying the 4xx", async (status) => {
  let failures = 0;
  server.use(http.get(`${url}/orders`, () => { failures++; return problem(status, "not_found"); }));
  renderApp(`/g/${token}`);
  await openOverview();
  expect(await screen.findByRole("alert")).toHaveTextContent(status === 404 ? "這個訂購連結無效或已失效" : "你目前無權限查看或使用這個訂購連結");
  expect(screen.queryByRole("tab")).not.toBeInTheDocument();
  expect(failures).toBe(1);
});

it("uses overview metadata to close writes even when group refresh fails, without overwriting the draft from overview", async () => {
  localStorage.setItem(`bento:last:${token}`, "p1");
  const { queryClient } = renderApp(`/g/${token}`);
  await userEvent.click(await screen.findByRole("button", { name: "增加 排骨飯" }));
  overview = { ...snapshot(), acceptingOrders: false, status: "closed" };
  server.use(http.get(url, () => problem(500, "internal_error")));
  await openOverview();
  await screen.findByText("排骨飯 ×1");
  expect(screen.getByRole("status")).toHaveTextContent("已關閉");
  await userEvent.click(ownTab());
  expect(await screen.findByText("你沒有訂餐")).toBeInTheDocument();
  expect(queryClient.getQueryData(effectiveOrderKey(token, "p1"))).toEqual({ order: null });
  expect(screen.queryByRole("button", { name: "送出訂單" })).not.toBeInTheDocument();
});

it("resets tab, identity and draft on token change", async () => {
  localStorage.setItem(`bento:last:${token}`, "p1");
  server.use(http.get("*/api/v1/public/g/other", () => HttpResponse.json({ ...base, title: "另一團" })));
  const { router } = renderApp(`/g/${token}`);
  await userEvent.click(await screen.findByRole("button", { name: "增加 排骨飯" }));
  await openOverview();
  await screen.findByText("排骨飯 ×1");
  await act(() => router.navigate("/g/other"));
  await screen.findByRole("heading", { name: "另一團" });
  expect(ownTab()).toHaveAttribute("aria-selected", "true");
  expect(screen.getByRole("button", { name: "小明" })).toBeInTheDocument();
  expect(screen.queryByRole("region", { name: "訂單送出操作" })).not.toBeInTheDocument();
});
