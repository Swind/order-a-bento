import { act, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { http, HttpResponse } from "msw";
import { beforeEach, expect, it } from "vitest";
import { renderApp } from "@/test/render";
import { problem, server } from "@/test/server";
import { effectiveOrderKey, orderSubmissionKey, publicGroupKey, publicOrdersKey } from "./hooks";

function deferred() {
  let resolve!: () => void;
  const promise = new Promise<void>((done) => { resolve = done; });
  return { promise, resolve };
}

const snapshot = (quantity: number) => ({ order: {
  submissionId: `s${quantity}`, submittedAt: "2026-05-01T02:02:00Z", cancelled: quantity === 0, note: null,
  total: quantity * 100,
  lines: quantity ? [{ itemId: "i1", name: "排骨飯", quantity, unitPrice: 100, subtotal: quantity * 100, note: null }] : [],
} });

beforeEach(() => localStorage.clear());

it("exits mutation loading when post-success cutoff revalidation fails and retries the persisted snapshot", async () => {
  localStorage.setItem("bento:last:retry", "p1");
  const post = deferred();
  let postStarted = false;
  let closed = false;
  let failReads = false;
  let saved = snapshot(1);
  server.use(
    http.get("*/api/v1/public/g/retry", () => HttpResponse.json({
      title: "retry", status: closed ? "closed" : "open", acceptingOrders: !closed, deadlineAt: null, note: null,
      vendor: { name: "店家", phone: null, address: null },
      participants: [{ id: "p1", name: "王小明", hasOrdered: true }],
      menu: [{ id: "i1", name: "排骨飯", price: 100, description: null, category: null, soldOut: false }],
    })),
    http.get("*/api/v1/public/g/retry/participants/p1/order", () => failReads ? problem(500, "internal_error") : HttpResponse.json(saved)),
    http.post("*/api/v1/public/g/retry/participants/p1/submissions", async () => {
      postStarted = true;
      await post.promise;
      saved = snapshot(2);
      failReads = true;
      return HttpResponse.json(saved);
    }),
  );
  const { queryClient } = renderApp("/g/retry");
  await userEvent.click(await screen.findByRole("button", { name: "修改訂單" }));
  await userEvent.click(await screen.findByRole("button", { name: "增加 排骨飯" }));
  await userEvent.click(screen.getByRole("button", { name: "儲存修改" }));
  await waitFor(() => expect(postStarted).toBe(true));
  closed = true;
  await act(() => queryClient.invalidateQueries({ queryKey: publicGroupKey("retry") }));
  await act(async () => post.resolve());
  await waitFor(() => expect(queryClient.isMutating({ mutationKey: orderSubmissionKey("retry", "p1") })).toBe(0));
  expect(await screen.findByRole("alert")).toHaveTextContent("無法查詢已送出的訂單");
  expect(queryClient.getQueryData(effectiveOrderKey("retry", "p1"))).toEqual(snapshot(2));
  expect(screen.queryByRole("region", { name: "已送出訂單" })).not.toBeInTheDocument();
  expect(screen.queryByText("正在查詢已送出的訂單")).not.toBeInTheDocument();
  failReads = false;
  await userEvent.click(screen.getByRole("button", { name: "重試" }));
  expect(await screen.findByText("總額 $200")).toBeInTheDocument();
  expect(queryClient.getQueryState(effectiveOrderKey("retry", "p1"))?.fetchStatus).toBe("idle");
  expect(screen.queryByRole("alert")).not.toBeInTheDocument();
});

it.each(["read-first", "write-first"])("cutoff revalidates the last persisted submission with %s response ordering", async (ordering) => {
  localStorage.setItem("bento:last:race", "p1");
  const post = deferred();
  const oldGet = deferred();
  const freshGet = deferred();
  let closed = false;
  let saved = snapshot(1);
  let postStarted = false;
  let oldGetStarted = false;
  let oldGetReturned = false;
  let oldGetAborted = false;
  let freshGetStarted = false;
  server.use(
    http.get("*/api/v1/public/g/race", () => HttpResponse.json({
      title: "race", status: "open", acceptingOrders: true, deadlineAt: closed ? "2000-01-01T00:00:00Z" : null, note: null,
      vendor: { name: "店家", phone: null, address: null },
      participants: [{ id: "p1", name: "王小明", hasOrdered: true }],
      menu: [{ id: "i1", name: "排骨飯", price: 100, description: null, category: null, soldOut: false }],
    })),
    http.get("*/api/v1/public/g/race/participants/p1/order", async ({ request }) => {
      const captured = saved; // A GET's snapshot is fixed when it starts, not when it returns.
      if (closed && !oldGetStarted) {
        oldGetStarted = true;
        request.signal.addEventListener("abort", () => { oldGetAborted = true; });
        await oldGet.promise;
        oldGetReturned = true;
      } else if (closed) {
        freshGetStarted = true;
        await freshGet.promise;
      }
      return HttpResponse.json(captured);
    }),
    http.post("*/api/v1/public/g/race/participants/p1/submissions", async () => {
      postStarted = true;
      await post.promise;
      saved = snapshot(2);
      return HttpResponse.json(saved);
    }),
  );

  const { queryClient } = renderApp("/g/race");
  await userEvent.click(await screen.findByRole("button", { name: "修改訂單" }));
  await userEvent.click(await screen.findByRole("button", { name: "增加 排骨飯" }));
  await userEvent.click(screen.getByRole("button", { name: "儲存修改" }));
  await waitFor(() => expect(postStarted).toBe(true));
  closed = true;
  await act(() => queryClient.invalidateQueries({ queryKey: publicGroupKey("race") }));
  await waitFor(() => expect(oldGetStarted).toBe(true));
  if (ordering === "read-first") {
    await act(async () => oldGet.resolve());
    await waitFor(() => expect(queryClient.getQueryData(effectiveOrderKey("race", "p1"))).toEqual(snapshot(1)));
    expect(screen.queryByRole("region", { name: "已送出訂單" })).not.toBeInTheDocument();
  }

  await act(async () => post.resolve());
  await waitFor(() => expect(freshGetStarted).toBe(true));
  expect(queryClient.getQueryData(effectiveOrderKey("race", "p1"))).toEqual(snapshot(2));
  // Even the POST result isn't presented as a revalidated readonly snapshot yet.
  expect(screen.queryByRole("region", { name: "已送出訂單" })).not.toBeInTheDocument();
  if (ordering === "write-first") {
    await waitFor(() => expect(oldGetAborted).toBe(true));
  }
  await act(async () => freshGet.resolve());
  expect(await screen.findByText("總額 $200")).toBeInTheDocument();
  if (ordering === "write-first") {
    // The obsolete response arrives last, after the revalidated summary is already visible.
    await act(async () => oldGet.resolve());
    await waitFor(() => expect(oldGetReturned).toBe(true));
  }
  expect(screen.getByText("排骨飯 ×2")).toBeInTheDocument();
  expect(queryClient.getQueryData(effectiveOrderKey("race", "p1"))).toEqual(snapshot(2));
  expect(queryClient.isMutating({ mutationKey: orderSubmissionKey("race", "p1") })).toBe(0);
});

const overviewGroup = {
  title: "overview race", status: "open", acceptingOrders: true, deadlineAt: null, note: null,
  vendor: { name: "店家", phone: null, address: null },
  participants: [{ id: "p1", name: "王小明", hasOrdered: true }],
  menu: [{ id: "i1", name: "排骨飯", price: 100, description: null, category: null, soldOut: false }],
};
const overviewSnapshot = (quantity: number) => ({ status: "open", acceptingOrders: true, deadlineAt: null,
  orders: quantity ? [{ participantId: "p1", name: "王小明", submittedAt: "2026-05-01T02:02:00Z",
    lines: [{ itemId: "i1", name: "排骨飯", quantity }] }] : [],
});

it.each(["success", "error", "null"])("keeps local editing locked through POST %s and deferred settlement, then uses latest persisted data", async (outcome) => {
  const token = "editing-race";
  const url = `*/api/v1/public/g/${token}`;
  localStorage.setItem(`bento:last:${token}`, "p1");
  const post = deferred();
  const settlement = deferred();
  let postStarted = false;
  let settled = false;
  let readStarted = false;
  const submittedSnapshot = { order: {
    ...snapshot(2).order, note: "本機整單草稿",
    lines: snapshot(2).order.lines.map((line) => ({ ...line, note: "本機品項草稿" })),
  } };
  server.use(
    http.get(url, () => HttpResponse.json(overviewGroup)),
    http.get(`${url}/participants/p1/order`, async () => {
      if (!settled) return HttpResponse.json(snapshot(1));
      readStarted = true;
      await settlement.promise;
      return HttpResponse.json(snapshot(5));
    }),
    http.post(`${url}/participants/p1/submissions`, async () => {
      postStarted = true;
      await post.promise;
      settled = true;
      return outcome === "error" ? problem(500, "internal_error") : HttpResponse.json(outcome === "null" ? { order: null } : submittedSnapshot);
    }),
  );
  const { queryClient } = renderApp(`/g/${token}`);
  await userEvent.click(await screen.findByRole("button", { name: "修改訂單" }));
  await userEvent.click(screen.getByRole("button", { name: "增加 排骨飯" }));
  await userEvent.click(screen.getByRole("button", { name: "＋備註" }));
  await userEvent.type(screen.getByLabelText("排骨飯 備註"), "本機品項草稿");
  await userEvent.type(screen.getByLabelText("訂單備註（選填）"), "本機整單草稿");
  const note = screen.getByLabelText("排骨飯 備註");
  await userEvent.click(screen.getByRole("button", { name: "儲存修改" }));
  await waitFor(() => expect(postStarted).toBe(true));
  expect(screen.queryByRole("region", { name: "已送出訂單" })).not.toBeInTheDocument();
  await act(async () => post.resolve());
  await waitFor(() => expect(readStarted).toBe(true));
  // POST onSuccess has rendered its cache value, but mutateAsync is still awaiting GET settlement.
  expect(queryClient.getQueryData(effectiveOrderKey(token, "p1"))).toEqual(outcome === "success" ? submittedSnapshot : snapshot(1));
  expect(queryClient.isMutating({ mutationKey: orderSubmissionKey(token, "p1") })).toBe(1);
  expect(screen.queryByRole("button", { name: "修改訂單" })).not.toBeInTheDocument();
  expect(screen.getByRole("button", { name: "送出中…" })).toBeDisabled();
  expect(screen.getByRole("status")).toHaveTextContent("處理訂單中…");
  expect(screen.getByRole("button", { name: "放棄修改" })).toBeDisabled();
  expect(screen.getByRole("button", { name: "取消我的訂單" })).toBeDisabled();
  expect(screen.getByRole("group", { name: "排骨飯 數量" })).toHaveTextContent("2");
  expect(screen.getByLabelText("排骨飯 備註")).toBe(note);
  expect(note).toHaveValue("本機品項草稿");
  expect(note).toBeDisabled();
  expect(screen.getByLabelText("訂單備註（選填）")).toHaveValue("本機整單草稿");
  await act(async () => settlement.resolve());
  await waitFor(() => expect(queryClient.isMutating()).toBe(0));
  expect(queryClient.getQueryData(effectiveOrderKey(token, "p1"))).toEqual(snapshot(5));
  if (outcome !== "success") {
    expect(await screen.findByRole("alert")).toHaveTextContent(outcome === "null" ? "發生未預期的錯誤，請稍後再試" : "伺服器發生錯誤");
    expect(screen.getByLabelText("排骨飯 備註")).toBe(note);
    expect(note).toHaveValue("本機品項草稿");
    expect(screen.getByLabelText("訂單備註（選填）")).toHaveValue("本機整單草稿");
    expect(screen.getByRole("group", { name: "排骨飯 數量" })).toHaveTextContent("2");
    // Discard is read-only: do not put the pre-edit snapshot back into the server cache.
    await userEvent.click(screen.getByRole("button", { name: "放棄修改" }));
  }
  expect(await screen.findByText("總額 $500")).toBeInTheDocument();
  expect(screen.getByText("排骨飯 ×5")).toBeInTheDocument();
  expect(document.documentElement.style.getPropertyValue("--order-bar-h")).toBe("");
  await waitFor(() => expect(screen.getByRole("status")).toHaveFocus());
  await userEvent.click(screen.getByRole("button", { name: "修改訂單" }));
  expect(screen.getByRole("group", { name: "排骨飯 數量" })).toHaveTextContent("5");
  expect(screen.queryByLabelText("排骨飯 備註")).not.toBeInTheDocument();
  expect(screen.getByLabelText("訂單備註（選填）")).toHaveValue("");
  expect(queryClient.getQueryData(effectiveOrderKey(token, "p1"))).toEqual(snapshot(5));
});

it("discards into the current refetched order without a write, invalidation or stale cache overwrite", async () => {
  const token = "discard-race";
  const url = `*/api/v1/public/g/${token}`;
  localStorage.setItem(`bento:last:${token}`, "p1");
  const refresh = deferred();
  let reads = 0;
  let started = false;
  let posts = 0;
  server.use(
    http.get(url, () => HttpResponse.json(overviewGroup)),
    http.get(`${url}/participants/p1/order`, async () => {
      if (++reads === 1) return HttpResponse.json(snapshot(1));
      started = true;
      await refresh.promise;
      return HttpResponse.json(snapshot(7));
    }),
    http.post(`${url}/participants/p1/submissions`, () => { posts++; return HttpResponse.json(snapshot(99)); }),
  );
  const { queryClient } = renderApp(`/g/${token}`);
  await userEvent.click(await screen.findByRole("button", { name: "修改訂單" }));
  await userEvent.click(screen.getByRole("button", { name: "增加 排骨飯" }));
  await userEvent.type(screen.getByLabelText("訂單備註（選填）"), "不可覆蓋新快照");
  let refetch!: Promise<void>;
  act(() => { refetch = queryClient.invalidateQueries({ queryKey: effectiveOrderKey(token, "p1") }); });
  await waitFor(() => expect(started).toBe(true));
  expect(screen.getByRole("group", { name: "排骨飯 數量" })).toHaveTextContent("2");
  await act(async () => { refresh.resolve(); await refetch; });
  expect(screen.getByLabelText("訂單備註（選填）")).toHaveValue("不可覆蓋新快照");
  await userEvent.click(screen.getByRole("button", { name: "放棄修改" }));
  expect(await screen.findByText("總額 $700")).toBeInTheDocument();
  expect(queryClient.getQueryData(effectiveOrderKey(token, "p1"))).toEqual(snapshot(7));
  await userEvent.click(screen.getByRole("button", { name: "修改訂單" }));
  expect(screen.getByRole("group", { name: "排骨飯 數量" })).toHaveTextContent("7");
  expect(screen.getByLabelText("訂單備註（選填）")).toHaveValue("");
  expect(reads).toBe(2);
  expect(posts).toBe(0);
});

it.each([0, 8])("does not expose cancellation completion from summary until settlement returns latest quantity %s", async (quantity) => {
  const token = "summary-cancel-race";
  const url = `*/api/v1/public/g/${token}`;
  localStorage.setItem(`bento:last:${token}`, "p1");
  const post = deferred();
  const settlement = deferred();
  let started = false;
  let posted = false;
  let reading = false;
  server.use(
    http.get(url, () => HttpResponse.json(overviewGroup)),
    http.get(`${url}/participants/p1/order`, async () => {
      if (!posted) return HttpResponse.json(snapshot(1));
      reading = true;
      await settlement.promise;
      return HttpResponse.json(snapshot(quantity));
    }),
    http.post(`${url}/participants/p1/submissions`, async () => {
      started = true;
      await post.promise;
      posted = true;
      return HttpResponse.json(snapshot(0));
    }),
  );
  const { queryClient } = renderApp(`/g/${token}`);
  await userEvent.click(await screen.findByRole("button", { name: "取消我的訂單" }));
  await userEvent.click(screen.getByRole("button", { name: "確定取消" }));
  await waitFor(() => expect(started).toBe(true));
  await act(async () => post.resolve());
  await waitFor(() => expect(reading).toBe(true));
  expect(queryClient.getQueryData(effectiveOrderKey(token, "p1"))).toEqual(snapshot(0));
  expect(screen.getByRole("dialog", { name: "確認取消訂單" })).toBeInTheDocument();
  expect(screen.getByRole("button", { name: "取消中…" })).toBeDisabled();
  expect(screen.getByRole("button", { name: "保留" })).toBeDisabled();
  expect(screen.queryByText("你的訂單已取消")).not.toBeInTheDocument();
  expect(screen.queryByRole("button", { name: "重新訂餐", hidden: true })).not.toBeInTheDocument();
  expect(screen.getByRole("status", { hidden: true })).toHaveTextContent("處理訂單中…");
  expect(screen.getByRole("status", { hidden: true })).not.toHaveClass("bg-green-50");
  expect(screen.getByRole("status", { hidden: true }).querySelector("svg")).toBeNull();
  await act(async () => settlement.resolve());
  await screen.findByRole("button", { name: quantity ? "修改訂單" : "重新訂餐" });
  expect(screen.getByRole("status")).toHaveTextContent(quantity ? "訂單已送出" : "你的訂單已取消");
  await waitFor(() => expect(screen.getByRole("status")).toHaveFocus());
  if (quantity) expect(screen.getByText("總額 $800")).toBeInTheDocument();
  expect(screen.queryByRole("region", { name: "訂單送出操作" })).not.toBeInTheDocument();
});

it("keeps the confirmed POST snapshot with a visible refresh error when open settlement GET fails", async () => {
  const token = "open-refresh-error";
  const url = `*/api/v1/public/g/${token}`;
  localStorage.setItem(`bento:last:${token}`, "p1");
  const post = deferred();
  let started = false;
  let saved = snapshot(1);
  let failReads = false;
  server.use(
    http.get(url, () => HttpResponse.json(overviewGroup)),
    http.get(`${url}/participants/p1/order`, () => failReads ? problem(500, "internal_error") : HttpResponse.json(saved)),
    http.post(`${url}/participants/p1/submissions`, async () => {
      started = true;
      await post.promise;
      saved = snapshot(4);
      failReads = true;
      return HttpResponse.json(saved);
    }),
  );
  renderApp(`/g/${token}`);
  await userEvent.click(await screen.findByRole("button", { name: "修改訂單" }));
  await userEvent.click(screen.getByRole("button", { name: "增加 排骨飯" }));
  await userEvent.click(screen.getByRole("button", { name: "儲存修改" }));
  await waitFor(() => expect(started).toBe(true));
  await act(async () => post.resolve());
  await screen.findByRole("button", { name: "修改訂單" });
  expect(screen.getByRole("alert")).toHaveTextContent("伺服器發生錯誤");
  expect(screen.getByText("總額 $400")).toBeInTheDocument();
  expect(screen.getByText("排骨飯 ×4")).toBeInTheDocument();
  expect(screen.queryByRole("region", { name: "訂單送出操作" })).not.toBeInTheDocument();
  failReads = false;
  saved = snapshot(6);
  await userEvent.click(screen.getByRole("button", { name: "重試" }));
  expect(await screen.findByText("總額 $600")).toBeInTheDocument();
  expect(screen.queryByRole("alert")).not.toBeInTheDocument();
});

it("does not revive ordering or cache after a late successful POST following revocation", async () => {
  const token = "late-post-revoked";
  const url = `*/api/v1/public/g/${token}`;
  localStorage.setItem(`bento:last:${token}`, "p1");
  const post = deferred();
  let started = false;
  let returned = false;
  let denied = false;
  server.use(
    http.get(url, () => denied ? problem(403, "forbidden") : HttpResponse.json(overviewGroup)),
    http.get(`${url}/participants/p1/order`, () => HttpResponse.json(snapshot(1))),
    http.post(`${url}/participants/p1/submissions`, async () => {
      started = true;
      await post.promise;
      returned = true;
      return HttpResponse.json(snapshot(8));
    }),
  );
  const { queryClient } = renderApp(`/g/${token}`);
  await userEvent.click(await screen.findByRole("button", { name: "修改訂單" }));
  await userEvent.click(screen.getByRole("button", { name: "增加 排骨飯" }));
  await userEvent.click(screen.getByRole("button", { name: "儲存修改" }));
  await waitFor(() => expect(started).toBe(true));
  denied = true;
  await act(() => queryClient.invalidateQueries({ queryKey: publicGroupKey(token) }));
  await screen.findByText("請向開團的人確認連結及存取權限。");
  await act(async () => post.resolve());
  await waitFor(() => expect(returned).toBe(true));
  await waitFor(() => expect(queryClient.isMutating()).toBe(0));
  expect(screen.queryByRole("tab")).not.toBeInTheDocument();
  expect(screen.queryByRole("region", { name: "已送出訂單" })).not.toBeInTheDocument();
  expect(queryClient.getQueryData(effectiveOrderKey(token, "p1"))).toBeUndefined();
  expect(queryClient.getQueryData(publicGroupKey(token))).toBeUndefined();
  expect(document.documentElement.style.getPropertyValue("--order-bar-h")).toBe("");
});

it.each(["success", "failure", "cancel"])("cancels a pre-settlement overview GET after POST %s and rejects its late snapshot", async (outcome) => {
  localStorage.setItem("bento:last:overview-race", "p1");
  const post = deferred();
  const oldGet = deferred();
  let postStarted = false;
  let oldStarted = false;
  let oldAborted = false;
  let oldReturned = false;
  let quantity = 1;
  let reads = 0;
  const url = "*/api/v1/public/g/overview-race";
  server.use(
    http.get(url, () => HttpResponse.json(overviewGroup)),
    http.get(`${url}/participants/p1/order`, () => HttpResponse.json(snapshot(quantity))),
    http.get(`${url}/orders`, async ({ request }) => {
      reads++;
      const captured = overviewSnapshot(quantity);
      if (reads === 1) {
        oldStarted = true;
        request.signal.addEventListener("abort", () => { oldAborted = true; });
        await oldGet.promise;
        oldReturned = true;
      }
      return HttpResponse.json(captured);
    }),
    http.post(`${url}/participants/p1/submissions`, async () => {
      postStarted = true;
      await post.promise;
      if (outcome === "failure") return problem(500, "internal_error");
      quantity = outcome === "cancel" ? 0 : 2;
      return HttpResponse.json(snapshot(quantity));
    }),
  );
  const { queryClient } = renderApp("/g/overview-race");
  await userEvent.click(await screen.findByRole("button", { name: "修改訂單" }));
  await userEvent.click(await screen.findByRole("button", { name: "增加 排骨飯" }));
  if (outcome === "cancel") {
    await userEvent.click(screen.getByRole("button", { name: "取消我的訂單" }));
    await userEvent.click(screen.getByRole("button", { name: "確定取消" }));
  } else await userEvent.click(screen.getByRole("button", { name: "儲存修改" }));
  await waitFor(() => expect(postStarted).toBe(true));
  act(() => screen.getByRole("tab", { name: "大家訂了什麼", hidden: true }).click());
  await waitFor(() => expect(oldStarted).toBe(true));
  expect(document.querySelector('[data-slot="dialog-overlay"]')).toBeNull();
  const activeTab = screen.getByRole("tab", { name: "大家訂了什麼" });
  activeTab.focus();
  await act(async () => post.resolve());
  await waitFor(() => expect(oldAborted).toBe(true));
  const panel = screen.getByRole("tabpanel", { name: "大家訂了什麼" });
  expect(await within(panel).findByText(outcome === "cancel" ? "目前還沒有人訂餐" : `排骨飯 ×${quantity}`)).toBeInTheDocument();
  await waitFor(() => expect(queryClient.isMutating()).toBe(0));
  await act(async () => oldGet.resolve());
  await waitFor(() => expect(oldReturned).toBe(true));
  expect(queryClient.getQueryData(publicOrdersKey("overview-race"))).toEqual(overviewSnapshot(quantity));
  expect(reads).toBe(2);
  expect(activeTab).toHaveFocus();
  if (outcome === "failure") {
    await userEvent.click(screen.getByRole("tab", { name: "我要點餐" }));
    expect(screen.getByRole("group", { name: "排骨飯 數量" })).toHaveTextContent("2");
  } else {
    expect(document.activeElement?.closest('[data-slot="tabs-content"][hidden]')).toBeNull();
    await userEvent.click(screen.getByRole("tab", { name: "我要點餐" }));
    expect(screen.getByRole("tab", { name: "我要點餐" })).toHaveFocus();
    expect(screen.getByRole("button", { name: outcome === "cancel" ? "重新訂餐" : "修改訂單" })).toBeInTheDocument();
    expect(screen.queryByRole("region", { name: "訂單送出操作" })).not.toBeInTheDocument();
    expect(document.documentElement.style.getPropertyValue("--order-bar-h")).toBe("");
  }
});

it.each([false, true])("aborts pending overview on revocation (leave tab first: %s) and rejects late success", async (leaveTab) => {
  const pending = deferred();
  let started = false;
  let aborted = false;
  let returned = false;
  let failGroup = false;
  const url = "*/api/v1/public/g/revoked-race";
  server.use(
    http.get(url, () => failGroup ? problem(403, "forbidden") : HttpResponse.json(overviewGroup)),
    http.get(`${url}/orders`, async ({ request }) => {
      started = true;
      request.signal.addEventListener("abort", () => { aborted = true; });
      await pending.promise;
      returned = true;
      return HttpResponse.json(overviewSnapshot(1));
    }),
  );
  const { queryClient } = renderApp("/g/revoked-race");
  await userEvent.click(await screen.findByRole("tab", { name: "大家訂了什麼" }));
  await waitFor(() => expect(started).toBe(true));
  if (leaveTab) {
    await userEvent.click(screen.getByRole("tab", { name: "我要點餐" }));
    await waitFor(() => expect(aborted).toBe(true));
  }
  failGroup = true;
  await act(() => queryClient.invalidateQueries({ queryKey: publicGroupKey("revoked-race") }));
  await screen.findByText("請向開團的人確認連結及存取權限。");
  await waitFor(() => expect(aborted).toBe(true));
  await act(async () => pending.resolve());
  await waitFor(() => expect(returned).toBe(true));
  expect(screen.queryByRole("tab")).not.toBeInTheDocument();
  expect(queryClient.getQueryData(publicOrdersKey("revoked-race"))).toBeUndefined();
  expect(queryClient.getQueryData(publicGroupKey("revoked-race"))).toBeUndefined();
});
