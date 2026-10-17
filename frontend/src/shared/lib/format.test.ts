import { describe, expect, it } from "vitest";
import { formatMoney, formatUpdatedAt, safeReturnTo } from "./format";

describe("formatMoney", () => {
  it("formats integers with a dollar sign and thousands separators", () => {
    expect(formatMoney(0)).toBe("$0");
    expect(formatMoney(85)).toBe("$85");
    expect(formatMoney(1234)).toBe("$1,234");
    expect(formatMoney(100000)).toBe("$100,000");
  });
});

describe("safeReturnTo", () => {
  it("accepts same-origin paths", () => {
    expect(safeReturnTo("/rosters?x=1")).toBe("/rosters?x=1");
  });
  it("rejects everything else", () => {
    for (const bad of [null, undefined, "", "//evil.com", "https://evil.com", "evil", "/\\evil.com"]) {
      expect(safeReturnTo(bad)).toBe("/");
    }
  });
});

describe("formatUpdatedAt", () => {
  const now = new Date("2026-01-10T12:00:00Z");
  it("handles just now, recent and old timestamps", () => {
    expect(formatUpdatedAt("2026-01-10T11:59:50Z", now)).toBe("剛剛");
    expect(formatUpdatedAt("2026-01-10T10:00:00Z", now)).toContain("2");
    expect(formatUpdatedAt("2025-06-01T00:00:00Z", now)).toMatch(/2025/);
    expect(formatUpdatedAt("garbage", now)).toBe("");
  });
});
