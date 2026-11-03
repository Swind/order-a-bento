import { describe, expect, it } from "vitest";
import { orderTotal, parseCents } from "./money";

describe("fixed-cent money", () => {
  it("parses two decimal places without float multiplication", () => {
    expect(parseCents("7.5")).toBe(750);
    expect(parseCents("7.50")).toBe(750);
    expect(parseCents("0.29")).toBe(29);
    expect(parseCents("-22.5")).toBe(-2250);
    for (const raw of ["7.501", "NaN", "Infinity", "7e2", "", "999999999999999999999"]) expect(parseCents(raw)).toBeNull();
  });
  it("computes exact quantity totals including binary-float traps", () => {
    expect(orderTotal([{ price: 7.5, quantity: 3 }])).toBe(22.5);
    expect(orderTotal([{ price: 0.1, quantity: 1 }, { price: 0.2, quantity: 1 }])).toBe(0.3);
    expect(orderTotal([{ price: 0.29, quantity: 3 }])).toBe(0.87);
  });
  it("rejects overflow and invalid quantities or precision", () => {
    expect(() => orderTotal([{ price: 1, quantity: Number.MAX_SAFE_INTEGER }])).toThrow();
    expect(() => orderTotal([{ price: 7.501, quantity: 1 }])).toThrow();
    expect(() => orderTotal([{ price: 7.5, quantity: 1.5 }])).toThrow();
  });
});
