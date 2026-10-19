import { screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { http, HttpResponse } from "msw";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { renderApp } from "@/test/render";
import { ME, server } from "@/test/server";
import { AUTH_CONFIG, ROSTER_RUN } from "@/features/ai-import/fixtures";

vi.mock("@/shared/lib/compressImage", () => ({
  compressImage: async () => new Blob(["jpegdata"], { type: "image/jpeg" }),
}));

beforeEach(() => {
  server.use(
    http.get("*/api/v1/me", () => HttpResponse.json(ME)),
    http.get("*/api/v1/auth/config", () => HttpResponse.json(AUTH_CONFIG)),
    http.get("*/api/v1/delivery-addresses", () => HttpResponse.json([])),
  );
});

describe("delivery addresses", () => {
  it("creates, edits and deletes a reusable address", async () => {
    let saved = { id: "a1", name: "", address: "", updatedAt: "2026-01-01T00:00:00Z" };
    let exists = false;
    server.use(
      http.get("*/api/v1/delivery-addresses", () => HttpResponse.json(exists ? [saved] : [])),
      http.post("*/api/v1/delivery-addresses", async ({ request }) => {
        const body = await request.json() as { name: string; address: string };
        expect(body).toEqual({ name: "台北教室", address: "台北市 3 樓" });
        saved = { ...saved, ...body };
        exists = true;
        return HttpResponse.json(saved, { status: 201 });
      }),
      http.put("*/api/v1/delivery-addresses/a1", async ({ request }) => {
        saved = { ...saved, ...await request.json() as { name: string; address: string } };
        return HttpResponse.json(saved);
      }),
      http.delete("*/api/v1/delivery-addresses/a1", () => {
        exists = false;
        return new HttpResponse(null, { status: 204 });
      }),
    );
    renderApp("/delivery-addresses");
    await userEvent.click(await screen.findByRole("button", { name: "新增地址" }));
    expect(screen.getByRole("button", { name: "儲存地址" })).toBeDisabled();
    await userEvent.type(screen.getByLabelText("地點名稱"), "台北教室");
    await userEvent.type(screen.getByLabelText("送餐地址"), "台北市 3 樓");
    await userEvent.click(screen.getByRole("button", { name: "儲存地址" }));
    await userEvent.click(await screen.findByRole("button", { name: "編輯 台北教室" }));
    await userEvent.clear(screen.getByLabelText("送餐地址"));
    await userEvent.type(screen.getByLabelText("送餐地址"), "台北市 5 樓");
    await userEvent.click(screen.getByRole("button", { name: "儲存地址" }));
    expect(await within(screen.getByRole("list", { name: "送餐地址清單" })).findByText("台北市 5 樓")).toBeInTheDocument();
    await userEvent.click(screen.getByRole("button", { name: "刪除 台北教室" }));
    await userEvent.click(screen.getByRole("button", { name: "確定刪除 台北教室" }));
    expect(await screen.findByText("還沒有送餐地址，可手動新增或從照片匯入。")).toBeInTheDocument();
  });

  it("uploads a delivery photo, allows correction and saves only after confirmation", async () => {
    const run = {
      ...ROSTER_RUN,
      mode: "deliveryAddress",
      targetRosterId: null,
      proposal: { targets: [{ type: "deliveryAddress", name: "台北教室", address: "", confidence: "low" }], warnings: ["請核對完整地址"] },
    };
    let applied: unknown;
    server.use(
      http.post("*/api/v1/ai/import-runs", async ({ request }) => {
        const form = await request.formData();
        expect(form.get("mode")).toBe("deliveryAddress");
        expect(form.get("targetVendorId")).toBeNull();
        return HttpResponse.json(run, { status: 201 });
      }),
      http.get("*/api/v1/ai/import-runs/run1", () => HttpResponse.json(run)),
      http.post("*/api/v1/ai/import-runs/run1/apply", async ({ request }) => {
        applied = await request.json();
        return HttpResponse.json({ rosterIds: [], vendorIds: [], deliveryAddressIds: ["a1"] });
      }),
    );
    const { router } = renderApp("/delivery-addresses");
    await userEvent.click(await screen.findByRole("button", { name: "📷 從照片匯入地址" }));
    expect(screen.getByRole("heading", { name: "從照片匯入送餐地址" })).toBeInTheDocument();
    await userEvent.upload(screen.getByLabelText("選擇照片檔案"), new File(["x"], "classroom.png", { type: "image/png" }));
    await userEvent.click(screen.getByRole("button", { name: "開始辨識" }));
    expect(await screen.findByRole("heading", { name: "送餐地址匯入確認" })).toBeInTheDocument();
    expect(screen.getByText(/請核對完整地址/)).toBeInTheDocument();
    expect(applied).toBeUndefined();
    expect(screen.getByRole("button", { name: "確認匯入 1 個地址" })).toBeDisabled();
    await userEvent.type(screen.getByLabelText("送餐地址"), "台北市 3 樓");
    await userEvent.click(screen.getByRole("button", { name: "確認匯入 1 個地址" }));
    await waitFor(() => expect(router.state.location.pathname).toBe("/delivery-addresses"));
    expect(applied).toEqual({ accepted: [{ type: "deliveryAddress", name: "台北教室", address: "台北市 3 樓" }] });
  });
});
