import { act, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { http, HttpResponse } from "msw";
import { beforeEach, expect, it } from "vitest";
import { renderApp } from "@/test/render";
import { problem, server } from "@/test/server";
import { effectiveOrderKey, orderSubmissionKey, publicGroupKey } from "./hooks";

function deferred() {
  let resolve!: () => void;
  const promise = new Promise<void>((done) => { resolve = done; });
  return { promise, resolve };
}

const snapshot = (quantity: number) => ({ order: {
  submissionId: `s${quantity}`, submittedAt: "2026-05-01T02:02:00Z", cancelled: false, note: null,
  total: quantity * 100,
  lines: [{ itemId: "i1", name: "排骨飯", quantity, unitPrice: 100, subtotal: quantity * 100, note: null }],
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
