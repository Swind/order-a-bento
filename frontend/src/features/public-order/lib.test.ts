import { describe, expect, it } from "vitest";
import { lineShareUrl } from "@/features/group-orders/share";
import { formatCountdown } from "@/shared/lib/datetime";
import { matchesName, normalizeSearch, resolveIdempotencyKey } from "./lib";

describe("name search", () => {
  it("is case-insensitive and full/half-width insensitive", () => {
    expect(normalizeSearch("ＡＭＹ")).toBe("amy");
    expect(matchesName("Amy Lee", "ａｍｙ")).toBe(true);
    expect(matchesName("amy", "AM")).toBe(true);
    expect(matchesName("王小明", "小")).toBe(true);
    expect(matchesName("王小明", "李")).toBe(false);
    expect(matchesName("王小明", "  ")).toBe(true);
  });
});

describe("resolveIdempotencyKey", () => {
  let n = 0;
  const gen = () => `key-${++n}`;
  it("reuses the key for the same payload and renews it when the payload changes", () => {
    const first = resolveIdempotencyKey(null, "a", gen);
    expect(resolveIdempotencyKey(first, "a", gen)).toBe(first);
    const second = resolveIdempotencyKey(first, "b", gen);
    expect(second.key).not.toBe(first.key);
  });
  it("generates a fresh key once the caller discards the previous one (after success)", () => {
    const first = resolveIdempotencyKey(null, "a", gen);
    expect(resolveIdempotencyKey(null, "a", gen).key).not.toBe(first.key);
  });
});

describe("formatCountdown", () => {
  const now = new Date("2026-05-01T10:00:00Z");
  const at = (min: number) => new Date(now.getTime() + min * 60_000).toISOString();
  it("formats minutes, hours and expiry", () => {
    expect(formatCountdown(at(42), now)).toBe("剩 42 分");
    expect(formatCountdown(at(65), now)).toBe("剩 1 小時 5 分");
    expect(formatCountdown(at(120), now)).toBe("剩 2 小時");
    expect(formatCountdown(at(0), now)).toBe("已截止");
    expect(formatCountdown(at(-5), now)).toBe("已截止");
  });
});

describe("lineShareUrl", () => {
  it("encodes title and link", () => {
    expect(lineShareUrl("週五 午餐", "https://x.test/g/abc")).toBe(
      `https://line.me/R/share?text=${encodeURIComponent("週五 午餐\nhttps://x.test/g/abc")}`,
    );
  });
});
