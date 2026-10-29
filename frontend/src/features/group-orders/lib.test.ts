import { describe, expect, it } from "vitest";
import { row } from "./fixtures";
import {
  balanceTag,
  buildConfirmationText,
  buildOrderText,
  deviceColor,
  filterChecklist,
  parseTab,
  splitParticipants,
  toggleStatus,
} from "./lib";

describe("buildOrderText", () => {
  it("lists only item quantities and their total with clear line breaks, skipping zero quantities", () => {
    expect(
      buildOrderText({
        vendorName: "好吃便當",
        deliveryAddress: "台北教室（台北市中正區 3 樓）",
        items: [
          { name: "排骨飯", quantity: 7 },
          { name: "素食", quantity: 0 },
          { name: "雞腿飯", quantity: 4 },
        ],
        totalQuantity: 11,
      }),
    ).toBe("您好，好吃便當！\n我們這邊是 台北教室（台北市中正區 3 樓）。\n想向您訂購以下便當：\n\n排骨飯 ×7\n雞腿飯 ×4\n\n共 11 個\n\n再麻煩您協助安排外送，\n請問方便接單嗎？謝謝您！");
  });
  it("does not invent a delivery address when none is configured", () => {
    const text = buildOrderText({ vendorName: "便當店", items: [{ name: "排骨飯", quantity: 1 }], totalQuantity: 1 });
    expect(text).toBe("您好，便當店！\n想向您訂購以下便當：\n\n排骨飯 ×1\n\n共 1 個\n\n請問方便接單嗎？謝謝您！");
  });
});

describe("balanceTag", () => {
  it("formats owe / refund and hides null or zero", () => {
    expect(balanceTag(30)).toEqual({ tone: "owe", text: "尚差 $30" });
    expect(balanceTag(-1200)).toEqual({ tone: "refund", text: "應退 $1,200" });
    expect(balanceTag(0)).toBeNull();
    expect(balanceTag(null)).toBeNull();
  });
});

describe("buildConfirmationText", () => {
  it("includes only current orders with quantities and item/order notes, excluding non-orderers and cancellations", () => {
    const text = buildConfirmationText({
      title: "週五午餐",
      vendorName: "好吃便當",
      participants: [
        row({ name: "小明", items: [{ itemId: "i1", name: "排骨飯", quantity: 2, note: "少飯" }, { itemId: "i2", name: "雞腿飯", quantity: 1 }], orderNote: "分開裝" }),
        row({ name: "小美", items: [], submittedAt: null, submissionCount: 0 }),
        row({ name: "小華", items: [], submissionCount: 2, orderNote: "取消的舊備註" }),
      ],
    });
    expect(text).toBe("【週五午餐】訂餐確認\n店家：好吃便當\n\n小明：排骨飯 ×2（少飯）、雞腿飯 ×1（訂單備註：分開裝）\n\n已訂 1 人，共 3 個\n\n請大家確認自己的餐點與數量是否正確，謝謝！");
  });

  it("omits the non-orderer line when everyone ordered", () => {
    expect(buildConfirmationText({ title: "午餐", vendorName: "店家", participants: [row({})] })).not.toContain("未訂餐");
  });

  it.each([
    { participants: [] },
    { participants: [row({ items: [], submittedAt: null, submissionCount: 0 }), row({ items: [], submissionCount: 2 })] },
  ])("returns no message when there are no effective orders (%#)", ({ participants }) => {
    expect(buildConfirmationText({ title: "午餐", vendorName: "店家", participants })).toBe("");
  });
});

describe("checklist filters", () => {
  const rows = [
    row({ id: "a", pickedUp: true, paid: true }),
    row({ id: "b", submissionCount: 3 }),
    row({ id: "c", items: [], amountDue: 0 }),
  ];
  it("splits ordered from not-ordered", () => {
    const { ordered, notOrdered } = splitParticipants(rows);
    expect(ordered.map((r) => r.id)).toEqual(["a", "b"]);
    expect(notOrdered.map((r) => r.id)).toEqual(["c"]);
  });
  it("filters by pickup, payment and changed orders", () => {
    const { ordered } = splitParticipants(rows);
    const ids = (f: Parameters<typeof filterChecklist>[1]) =>
      filterChecklist(ordered, f).map((r) => r.id);
    expect(ids("all")).toEqual(["a", "b"]);
    expect(ids("unpicked")).toEqual(["b"]);
    expect(ids("unpaid")).toEqual(["b"]);
    expect(ids("changed")).toEqual(["b"]);
  });
});

describe("deviceColor", () => {
  it("is stable per device and differs between devices", () => {
    expect(deviceColor("7c2")).toBe(deviceColor("7c2"));
    expect(deviceColor("7c2")).toMatch(/^hsl\(\d+ /);
    expect(deviceColor("7c2")).not.toBe(deviceColor("a3f"));
  });
});

describe("status chips / tabs", () => {
  it("toggles statuses but never empties the selection", () => {
    expect(toggleStatus(["open", "closed"], "archived")).toEqual(["open", "closed", "archived"]);
    expect(toggleStatus(["open", "closed"], "closed")).toEqual(["open"]);
    expect(toggleStatus(["open"], "open")).toEqual(["open"]);
  });
  it("falls back to the stats tab", () => {
    expect(parseTab("checklist")).toBe("checklist");
    expect(parseTab("nope")).toBe("stats");
    expect(parseTab(null)).toBe("stats");
  });
});
