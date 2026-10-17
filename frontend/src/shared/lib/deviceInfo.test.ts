import { afterEach, describe, expect, it, vi } from "vitest";
import { collectClientInfo, getDeviceId } from "./deviceInfo";

afterEach(() => vi.restoreAllMocks());

describe("deviceInfo", () => {
  it("persists a device id in localStorage", () => {
    localStorage.removeItem("bento:device");
    const id = getDeviceId();
    expect(localStorage.getItem("bento:device")).toBe(id);
    expect(getDeviceId()).toBe(id);
  });

  it("falls back to a stable in-memory id when localStorage throws", async () => {
    vi.spyOn(Storage.prototype, "getItem").mockImplementation(() => {
      throw new Error("denied");
    });
    vi.spyOn(Storage.prototype, "setItem").mockImplementation(() => {
      throw new Error("denied");
    });
    const a = getDeviceId();
    expect(a).toBeTruthy();
    expect(getDeviceId()).toBe(a);
    const info = await collectClientInfo();
    expect(info.deviceId).toBe(a);
    expect(info.screen).toMatch(/^\d+x\d+$/);
    expect(info.timezone).toBeTruthy();
    expect(info.deviceModel).toBeUndefined();
  });

  it("reads the model from userAgentData and never throws if it rejects", async () => {
    const nav = navigator as unknown as { userAgentData?: unknown };
    nav.userAgentData = { getHighEntropyValues: async () => ({ model: "Pixel 9" }) };
    expect((await collectClientInfo()).deviceModel).toBe("Pixel 9");
    nav.userAgentData = {
      getHighEntropyValues: async () => {
        throw new Error("nope");
      },
    };
    expect((await collectClientInfo()).deviceModel).toBeUndefined();
    delete nav.userAgentData;
  });
});
