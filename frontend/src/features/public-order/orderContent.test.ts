import { expect, it } from "vitest";
import { orderContent } from "./lib";

it("compares only positive quantities, IDs and trimmed notes in stable order", () => {
  const lines = [{ itemId: "b", quantity: 2, note: " 不辣 " }, { itemId: "a", quantity: 1, note: null }];
  const baseline = orderContent(lines, " 晚點 ");
  expect(orderContent([{ itemId: "a", quantity: 1, note: "" }, { itemId: "b", quantity: 2, note: "不辣" }, { itemId: "c", quantity: 0 }], "晚點")).toBe(baseline);
  expect(orderContent(lines.map((l) => ({ ...l, quantity: l.quantity + 1 })), "晚點")).not.toBe(baseline);
  expect(orderContent([{ ...lines[0], note: "辣" }, lines[1]], "晚點")).not.toBe(baseline);
  expect(orderContent(lines, "早點")).not.toBe(baseline);
  expect(orderContent(lines, " 晚點 ")).toBe(baseline);
  expect(orderContent([], null)).toBe(orderContent([], " "));
});
