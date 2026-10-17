import { describe, expect, it } from "vitest";
import { scaledSize } from "./compressImage";

describe("scaledSize", () => {
  it("scales landscape by width", () => {
    expect(scaledSize(4032, 3024)).toEqual({ width: 1568, height: 1176 });
  });
  it("scales portrait by height", () => {
    expect(scaledSize(3024, 4032)).toEqual({ width: 1176, height: 1568 });
  });
  it("never upscales", () => {
    expect(scaledSize(800, 600)).toEqual({ width: 800, height: 600 });
    expect(scaledSize(1568, 1000)).toEqual({ width: 1568, height: 1000 });
  });
  it("keeps at least 1px on extreme panoramas", () => {
    expect(scaledSize(20000, 1)).toEqual({ width: 1568, height: 1 });
  });
});
