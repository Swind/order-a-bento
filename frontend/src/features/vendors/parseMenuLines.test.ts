import { describe, expect, it } from "vitest";
import { parseMenuLines, parsePrice } from "./parseMenuLines";

describe("parsePrice", () => {
  it("accepts common notations", () => {
    for (const s of ["85", "$85", "NT$85", "nt$ 85", "85元", "NT$85元", "８５", "1,200"]) {
      expect(parsePrice(s), s).not.toBeNull();
    }
    expect(parsePrice("1,200")).toBe(1200);
    expect(parsePrice("８５")).toBe(85);
  });
  it("rejects garbage and decimals", () => {
    for (const s of ["abc", "8.5", "-5", "", "$"]) expect(parsePrice(s), s).toBeNull();
  });
});

describe("parseMenuLines", () => {
  it("parses space separated lines", () => {
    expect(parseMenuLines("雞腿飯 85\n排骨便當 $90\n  控肉飯  NT$95  ")).toEqual({
      items: [
        { name: "雞腿飯", price: 85 },
        { name: "排骨便當", price: 90 },
        { name: "控肉飯", price: 95 },
      ],
      errors: [],
    });
  });
  it("parses comma separated lines with description", () => {
    const r = parseMenuLines("雞腿飯,85,附滷蛋\n排骨飯，90元\n魚排\t100\t加辣, 不辣");
    expect(r.errors).toEqual([]);
    expect(r.items).toEqual([
      { name: "雞腿飯", price: 85, description: "附滷蛋" },
      { name: "排骨飯", price: 90 },
      { name: "魚排", price: 100, description: "加辣, 不辣" },
    ]);
  });
  it("keeps names with inner spaces", () => {
    expect(parseMenuLines("紅燒 牛肉 麵 150").items[0]).toEqual({ name: "紅燒 牛肉 麵", price: 150 });
  });
  it("reports errors with 1-based line numbers and skips blank lines", () => {
    const r = parseMenuLines("雞腿飯 85\n\n只有品名\n便當 abc\n,50\n超貴 100001\n免費 0\n便當,");
    expect(r.items).toEqual([{ name: "雞腿飯", price: 85 }]);
    expect(r.errors.map((e) => e.line)).toEqual([3, 4, 5, 6, 7, 8]);
    expect(r.errors.every((e) => e.reason.length > 0)).toBe(true);
  });
});
