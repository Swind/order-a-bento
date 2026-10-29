import { describe, expect, it, vi } from "vitest";
import { compressImage, scaledSize } from "./compressImage";

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

describe("compressImage", () => {
  it.each(["image/png", "image/webp"])("paints white before drawing %s and encodes JPEG", async (type) => {
    const calls: string[] = [];
    const close = vi.fn();
    const ctx = {
      fillStyle: "",
      fillRect: vi.fn(() => { calls.push(`fill:${ctx.fillStyle}`); }),
      drawImage: vi.fn(() => { calls.push("draw"); }),
    };
    vi.stubGlobal("createImageBitmap", vi.fn(async () => ({ width: 2, height: 1, close })));
    const context = vi.spyOn(HTMLCanvasElement.prototype, "getContext").mockReturnValue(ctx as unknown as CanvasRenderingContext2D);
    const blob = vi.spyOn(HTMLCanvasElement.prototype, "toBlob").mockImplementation((cb, mime) => {
      expect(mime).toBe("image/jpeg");
      cb(new Blob(["jpeg"], { type: mime }));
    });
    try {
      const result = await compressImage(new Blob(["transparent"], { type }));
      expect(calls).toEqual(["fill:#ffffff", "draw"]);
      expect(ctx.fillRect).toHaveBeenCalledWith(0, 0, 2, 1);
      expect(result.type).toBe("image/jpeg");
      expect(close).toHaveBeenCalledOnce();
    } finally {
      context.mockRestore();
      blob.mockRestore();
      vi.unstubAllGlobals();
    }
  });
});
