import { screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { delay, http, HttpResponse } from "msw";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { renderApp } from "@/test/render";
import { ME, problem, server } from "@/test/server";
import { GROUP, row } from "./fixtures";

const SUMMARY = {
  groupOrderId: "g1",
  status: "open",
  items: [
    { itemId: "i1", name: "排骨飯", price: 100, quantity: 7, subtotal: 700 },
    { itemId: "i2", name: "雞腿飯", price: 110, quantity: 4, subtotal: 440 },
    { itemId: "i3", name: "素食", price: 90, quantity: 0, subtotal: 0 },
  ],
  totalQuantity: 11,
  totalAmount: 1140,
  participants: { total: 3, ordered: 2, notOrdered: 1 },
  settlement: { pickedUp: 0, paid: 1, paidAmount: 520, unpaidAmount: 620 },
};

const ROWS = [
  row({ id: "p1", name: "王小明", amountDue: 100 }),
  row({
    id: "p2",
    name: "陳美麗",
    amountDue: 130,
    paid: true,
    paidAmount: 100,
    balance: 30,
    submissionCount: 2,
    items: [{ itemId: "i2", name: "雞腿飯", quantity: 1, note: "不要辣" }],
  }),
  row({ id: "p3", name: "李大華", amountDue: 0, items: [], submittedAt: null, submissionCount: 0 }),
];

let group = { ...GROUP };

beforeEach(() => {
  group = { ...GROUP };
  server.use(
    http.get("*/api/v1/me", () => HttpResponse.json(ME)),
    http.get("*/api/v1/group-orders/g1", () => HttpResponse.json(group)),
    http.get("*/api/v1/group-orders/g1/summary", () => HttpResponse.json(SUMMARY)),
    http.get("*/api/v1/group-orders/g1/participants", () => HttpResponse.json(ROWS)),
  );
});

describe("GroupOrderDetailPage header", () => {
  it("keeps settings collapsed and opens the link for manual copying on clipboard failure", async () => {
    const user = userEvent.setup();
    vi.spyOn(navigator.clipboard, "writeText").mockRejectedValue(new Error("denied"));
    renderApp("/group-orders/g1");
    await screen.findByText("更多操作");
    expect(screen.getByRole("button", { name: "重新產生連結" })).not.toBeVisible();
    expect(screen.getByRole("textbox", { name: "分享連結" })).not.toBeVisible();
    await user.click(screen.getByRole("button", { name: "複製連結" }));
    const input = screen.getByRole<HTMLInputElement>("textbox", { name: "分享連結" });
    expect(input).toHaveFocus();
    expect(input.selectionStart).toBe(0);
    expect(input.selectionEnd).toBe(input.value.length);
  });

  it("closing switches badge and actions", async () => {
    server.use(
      http.patch("*/api/v1/group-orders/g1", async ({ request }) => {
        expect(await request.json()).toEqual({ status: "closed" });
        group = { ...group, status: "closed" };
        return HttpResponse.json(group);
      }),
    );
    renderApp("/group-orders/g1");
    expect(await screen.findByText("進行中")).toBeInTheDocument();
    await userEvent.click(screen.getByRole("button", { name: "關閉收單" }));
    expect(await screen.findByText("已關閉")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "重新開放" })).toBeInTheDocument();
    await userEvent.click(screen.getByText("更多操作"));
    expect(screen.getByRole("button", { name: "封存" })).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "關閉收單" })).not.toBeInTheDocument();
  });

  it("rotating the link (after confirm) shows the new URL", async () => {
    server.use(
      http.post("*/api/v1/group-orders/g1/share-token/rotate", () =>
        HttpResponse.json({ ...group, shareUrl: "https://x.test/g/new" }),
      ),
    );
    renderApp("/group-orders/g1");
    await userEvent.click(await screen.findByText("更多操作"));
    await userEvent.click(await screen.findByRole("button", { name: "重新產生連結" }));
    expect(screen.getByText(/舊的分享連結會立即失效/)).toBeInTheDocument();
    await userEvent.click(screen.getByRole("button", { name: "確定重新產生連結" }));
    expect(await screen.findByDisplayValue("https://x.test/g/new")).toBeInTheDocument();
  });

  it("archived group is read-only", async () => {
    group = { ...group, status: "archived" };
    renderApp("/group-orders/g1");
    expect(await screen.findByText("此團已封存，內容為唯讀。")).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "重新開放" })).not.toBeInTheDocument();
  });

  it("selected tab lives in ?tab=", async () => {
    const { router } = renderApp("/group-orders/g1");
    await userEvent.click(await screen.findByRole("tab", { name: "明細" }));
    expect(router.state.location.search).toBe("?tab=details");
  });
});

describe("StatsTab", () => {
  it("shows totals and per-item rows", async () => {
    renderApp("/group-orders/g1");
    expect(await screen.findByText("共 11 個 · $1,140")).toBeInTheDocument();
    expect(screen.getByText("已訂 2 / 未訂 1")).toBeInTheDocument();
    expect(screen.getByText("×7")).toBeInTheDocument();
    expect(
      within(screen.getByRole("list", { name: "品項統計" })).queryByText("素食"),
    ).not.toBeInTheDocument();
  });

  it("previews and copies the same shop-facing quantities without names or zero-quantity items", async () => {
    const user = userEvent.setup();
    const writeText = vi.spyOn(navigator.clipboard, "writeText");
    renderApp("/group-orders/g1");
    const preview = await screen.findByRole("textbox", { name: "訂單文字" });
    const expected = "您好，好吃便當！\n我們這邊是 台北教室（台北市中正區 3 樓）。\n想向您訂購以下便當：\n\n排骨飯 ×7\n雞腿飯 ×4\n\n共 11 個\n\n再麻煩您協助安排外送，\n請問方便接單嗎？謝謝您！";
    expect(preview).toHaveValue(expected);
    expect(preview).toHaveAttribute("readonly");
    await user.click(screen.getByRole("button", { name: "複製訂單文字" }));
    expect(writeText).toHaveBeenCalledWith(expected);
    expect(await screen.findByText("已複製訂單文字")).toBeInTheDocument();
  });

  it("selects the preview for manual copying when clipboard access fails", async () => {
    const user = userEvent.setup();
    vi.spyOn(navigator.clipboard, "writeText").mockRejectedValue(new Error("denied"));
    renderApp("/group-orders/g1");
    const preview = await screen.findByRole<HTMLTextAreaElement>("textbox", { name: "訂單文字" });
    await user.click(screen.getByRole("button", { name: "複製訂單文字" }));
    expect(await screen.findByText("無法自動複製，已選取文字，請手動複製")).toBeInTheDocument();
    expect(preview).toHaveFocus();
    expect(preview.selectionStart).toBe(0);
    expect(preview.selectionEnd).toBe(preview.value.length);
  });

  it("shows an empty state when nobody has ordered", async () => {
    server.use(
      http.get("*/api/v1/group-orders/g1/summary", () =>
        HttpResponse.json({
          ...SUMMARY,
          items: SUMMARY.items.map((i) => ({ ...i, quantity: 0, subtotal: 0 })),
          totalQuantity: 0,
          totalAmount: 0,
          participants: { ordered: 0, notOrdered: 3 },
        }),
      ),
    );
    renderApp("/group-orders/g1");
    expect(await screen.findByText("還沒有人下單")).toBeInTheDocument();
    expect(screen.getByText(/把訂購頁連結分享給大家/)).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "複製訂單文字" })).not.toBeInTheDocument();
  });

  it("marks an item sold out", async () => {
    let body: unknown;
    server.use(
      http.patch("*/api/v1/group-orders/g1/offered-items/i1", async ({ request }) => {
        body = await request.json();
        return HttpResponse.json(group);
      }),
    );
    renderApp("/group-orders/g1");
    await userEvent.click(await screen.findByRole("switch", { name: "排骨飯 售完" }));
    await screen.findByText("共 11 個 · $1,140");
    expect(body).toEqual({ soldOut: true });
  });

  it("sets a saved delivery address and updates the message preview", async () => {
    server.use(
      http.get("*/api/v1/delivery-addresses", () => HttpResponse.json([{ id: "a2", name: "新教室", address: "新地址 5 樓", updatedAt: "2026-01-01T00:00:00Z" }])),
      http.patch("*/api/v1/group-orders/g1", async ({ request }) => {
        expect(await request.json()).toEqual({ deliveryAddressId: "a2" });
        group = { ...group, deliveryAddress: "新教室（新地址 5 樓）" };
        return HttpResponse.json(group);
      }),
    );
    renderApp("/group-orders/g1");
    await userEvent.click(await screen.findByText("更多操作"));
    await userEvent.click(await screen.findByRole("button", { name: "設定送餐地址" }));
    await screen.findByRole("option", { name: "新教室 — 新地址 5 樓" });
    await userEvent.selectOptions(screen.getByLabelText("送餐地址（選填）"), "a2");
    await userEvent.click(screen.getByRole("button", { name: "儲存送餐地址" }));
    expect(await screen.findByText("送餐地址：新教室（新地址 5 樓）")).toBeInTheDocument();
    expect(screen.getByRole<HTMLTextAreaElement>("textbox", { name: "訂單文字" }).value).toContain("我們這邊是 新教室（新地址 5 樓）");
  });
});

describe("ChecklistTab", () => {
  it("optimistically flips 繳費 and rolls back with a toast on 500", async () => {
    server.use(
      http.put("*/api/v1/group-orders/g1/participants/p1/settlement", async () => {
        await delay(100);
        return problem(500, "internal_error");
      }),
    );
    renderApp("/group-orders/g1?tab=checklist");
    const sw = await screen.findByRole("switch", { name: "王小明 繳費" });
    expect(sw).toHaveAttribute("aria-checked", "false");
    await userEvent.click(sw);
    expect(screen.getByRole("switch", { name: "王小明 繳費" })).toHaveAttribute(
      "aria-checked",
      "true",
    );
    expect(await screen.findByText("伺服器發生錯誤，請稍後再試")).toBeInTheDocument();
    expect(screen.getByRole("switch", { name: "王小明 繳費" })).toHaveAttribute(
      "aria-checked",
      "false",
    );
  });

  it("shows the 尚差 tag, footer figures and a muted not-ordered section", async () => {
    renderApp("/group-orders/g1?tab=checklist");
    expect(await screen.findByText("尚差 $30")).toBeInTheDocument();
    expect(await screen.findByText("已收 $520 / 應收 $1,140")).toBeInTheDocument();
    expect(screen.getByText("未繳 $620")).toBeInTheDocument();
    const muted = screen.getByRole("region", { name: "未訂餐" });
    expect(within(muted).getByText("李大華")).toBeInTheDocument();
    expect(screen.queryByRole("switch", { name: "李大華 繳費" })).not.toBeInTheDocument();
  });

  it("filters by 有改單", async () => {
    renderApp("/group-orders/g1?tab=checklist");
    await screen.findByText("王小明");
    await userEvent.click(screen.getByRole("button", { name: "有改單" }));
    expect(screen.queryByText("王小明")).not.toBeInTheDocument();
    expect(screen.getByText("陳美麗")).toBeInTheDocument();
  });
});

describe("DetailsTab + history sheet", () => {
  const HISTORY = [
    {
      id: "s3",
      submittedAt: "2026-01-01T02:15:00Z",
      submittedBy: "participant",
      cancelled: false,
      total: 110,
      note: null,
      lines: [{ itemId: "i2", name: "雞腿飯", quantity: 1, unitPrice: 110, note: null }],
      client: { ip: "203.0.113.8", uaSummary: "Android · Chrome 129", deviceShort: "7c2" },
      effective: true,
      sharedDeviceWith: ["王小明"],
    },
    {
      id: "s2",
      submittedAt: "2026-01-01T02:10:00Z",
      submittedBy: "participant",
      cancelled: true,
      total: 0,
      note: null,
      lines: [],
      client: { ip: null, uaSummary: null, deviceShort: "7c2" },
      effective: false,
      sharedDeviceWith: [],
    },
  ];

  it("lists orders with notes, badges, and opens the history sheet", async () => {
    server.use(
      http.get("*/api/v1/group-orders/g1/participants/p2/submissions", () =>
        HttpResponse.json(HISTORY),
      ),
    );
    renderApp("/group-orders/g1?tab=details");
    expect(await screen.findByText("雞腿飯 ×1（不要辣）")).toBeInTheDocument();
    expect(screen.getByText("改過 2 次")).toBeInTheDocument();
    await userEvent.click(screen.getByRole("button", { name: /陳美麗/ }));
    expect(await screen.findByText("目前有效")).toBeInTheDocument();
    expect(screen.getByText("取消訂單")).toBeInTheDocument();
    expect(screen.getByText("⚠ 此裝置也替 王小明 送出過訂單")).toBeInTheDocument();
    expect(screen.getAllByTestId("device-short")[0]).toHaveTextContent("#7c2");
    expect(screen.getByRole("button", { name: "代為修改訂單" })).toBeInTheDocument();
  });

  it.each([null, "2000-01-01T11:00:00+08:00"])("organizer can edit with deadline %s", async (deadlineAt) => {
    group = { ...group, deadlineAt };
    let body: Record<string, unknown> | undefined;
    server.use(
      http.get("*/api/v1/group-orders/g1/participants/p2/submissions", () =>
        HttpResponse.json(HISTORY),
      ),
      http.post("*/api/v1/group-orders/g1/participants/p2/submissions", async ({ request }) => {
        body = (await request.json()) as Record<string, unknown>;
        return HttpResponse.json({ order: null });
      }),
    );
    renderApp("/group-orders/g1?tab=details");
    if (deadlineAt) expect(await screen.findByText("已截止")).toBeInTheDocument();
    await userEvent.click(await screen.findByRole("button", { name: /陳美麗/ }));
    await userEvent.click(await screen.findByRole("button", { name: "代為修改訂單" }));
    await userEvent.click(screen.getByRole("button", { name: "增加 雞腿飯" }));
    await userEvent.click(screen.getByRole("button", { name: "儲存訂單" }));
    await screen.findByText("目前有效");
    expect(body).toMatchObject({ lines: [{ itemId: "i2", quantity: 2 }] });
    expect(typeof body?.idempotencyKey).toBe("string");
  });

  it("adding a participant shows name_taken on the field", async () => {
    server.use(
      http.post("*/api/v1/group-orders/g1/participants", () =>
        problem(409, "name_taken", { field: "name" }),
      ),
    );
    renderApp("/group-orders/g1?tab=details");
    await userEvent.click(await screen.findByRole("button", { name: "＋ 臨時加入成員" }));
    await userEvent.type(screen.getByLabelText("成員姓名"), "王小明");
    await userEvent.click(screen.getByRole("button", { name: "加入" }));
    expect(await screen.findByText("已有同名項目")).toBeInTheDocument();
  });
});
