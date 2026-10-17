import { describe, expect, it } from "vitest";
import { ApiError } from "./client";
import { errorMessage, fieldError, loginErrorMessage } from "./errors";

const err = (status: number, code: string, extra: object = {}) =>
  new ApiError({ type: "about:blank", title: "x", status, detail: "English detail", code, ...extra });

describe("errorMessage", () => {
  it("maps known codes to zh-TW", () => {
    expect(errorMessage(err(409, "vendor_name_taken"))).toBe("已有同名店家");
    expect(errorMessage(err(409, "name_taken"))).toBe("已有同名項目");
  });
  it("maps generic invalid_* using the field", () => {
    expect(errorMessage(err(422, "invalid_phone", { field: "phone" }))).toBe("電話格式不正確");
  });
  it("treats non-ApiError TypeError as network failure", () => {
    expect(errorMessage(new TypeError("Failed to fetch"))).toContain("網路");
  });
  it("never leaks English detail for unknown codes", () => {
    expect(errorMessage(err(400, "weird"))).not.toContain("English");
  });
});

describe("fieldError", () => {
  it("returns the field when the form has it", () => {
    expect(fieldError(err(409, "name_taken", { field: "name" }), ["name"])?.field).toBe("name");
    expect(fieldError(err(409, "name_taken", { field: "other" }), ["name"])).toBeNull();
  });
});

describe("loginErrorMessage", () => {
  it("has a fallback", () => {
    expect(loginErrorMessage("access_denied")).toContain("取消");
    expect(loginErrorMessage("zzz")).toContain("錯誤");
  });
});
