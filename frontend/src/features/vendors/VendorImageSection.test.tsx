import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { http, HttpResponse } from "msw";
import { afterAll, afterEach, beforeAll, beforeEach, describe, expect, it, vi } from "vitest";
import { ME, problem, server } from "@/test/server";
import { renderApp } from "@/test/render";
import type { VendorDetail, VendorImage } from "@/shared/api/types";
import { VendorImageSection } from "./VendorImageSection";

const image: VendorImage = { revision: "r1", contentType: "image/jpeg", width: 10, height: 10, sizeBytes: 3, updatedAt: "1" };
const vendor: VendorDetail = { id: "v1", name: "便當", phone: null, address: null, note: null, canManage: true, image: null, menuItems: [], updatedAt: "1" };
const jpg = () => new File([new Uint8Array([255, 216, 255, 217])], "photo.jpg", { type: "image/jpeg" });
const create = vi.fn();
const revoke = vi.fn();
const originalCreate = URL.createObjectURL;
const originalRevoke = URL.revokeObjectURL;
beforeAll(() => { URL.createObjectURL = create; URL.revokeObjectURL = revoke; });
afterAll(() => { URL.createObjectURL = originalCreate; URL.revokeObjectURL = originalRevoke; });
afterEach(() => vi.restoreAllMocks());
beforeEach(() => {
  create.mockReset(); revoke.mockReset();
  create.mockImplementation(() => `blob:test-${create.mock.calls.length}`);
  server.use(
    http.get("*/api/v1/me", () => HttpResponse.json(ME)),
    http.get("*/api/v1/vendors/:id/image", () => new HttpResponse(new Uint8Array([1, 2, 3]), { headers: { "content-type": "image/jpeg" } })),
  );
});

function section(v: VendorDetail = vendor) {
  const qc = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  qc.setQueryData(["me"], ME);
  const wrap = (value: VendorDetail) => <QueryClientProvider client={qc}><VendorImageSection vendor={value} /></QueryClientProvider>;
  const result = render(wrap(v));
  return { ...result, qc, change: (value: VendorDetail) => result.rerender(wrap(value)) };
}
const choose = (file = jpg()) => userEvent.upload(screen.getByLabelText("選擇參考照片檔案"), file);

describe("vendor reference photo", () => {
  it("previews/cancels/reselects the same file and revokes all object URLs", async () => {
    const { unmount } = section();
    expect(screen.getByText("尚未上傳參考照片")).toBeVisible();
    const file = jpg();
    await choose(file);
    await userEvent.click(screen.getByRole("button", { name: "待上傳照片預覽" }));
    expect(await screen.findByRole("dialog")).toBeVisible();
    await userEvent.keyboard("{Escape}");
    await waitFor(() => expect(screen.queryByRole("dialog")).not.toBeInTheDocument());
    await userEvent.click(screen.getByRole("button", { name: "取消上傳" }));
    expect(revoke).toHaveBeenCalledWith("blob:test-1");
    await choose(file);
    expect(await screen.findByAltText("待上傳照片")).toBeVisible();
    await choose(new File(["png"], "other.png", { type: "image/png" }));
    expect(revoke).toHaveBeenCalledWith("blob:test-2");
    unmount();
    expect(revoke).toHaveBeenCalledWith("blob:test-3");
  });

  it("rejects empty/oversized/WebP locally without uploading", async () => {
    const put = vi.fn();
    server.use(http.put("*/api/v1/vendors/:id/image", put));
    section();
    const input = screen.getByLabelText("選擇參考照片檔案");
    for (const file of [new File([], "empty.png", { type: "image/png" }), new File(["webp"], "x.webp", { type: "image/webp" })]) {
      fireEvent.change(input, { target: { files: [file] } });
      expect(await screen.findByRole("alert")).toHaveTextContent("JPEG 或 PNG");
    }
    fireEvent.change(input, { target: { files: [new File([new Uint8Array(5 * 1024 * 1024 + 1)], "large.jpg", { type: "image/jpeg" })] } });
    expect(screen.getByRole("alert")).toHaveTextContent("5 MiB");
    expect(put).not.toHaveBeenCalled();
  });

  it("uploads original bytes in one typed PUT, keeps old photo while busy, invalidates detail and lists", async () => {
    let release!: () => void;
    const waiting = new Promise<void>((resolve) => { release = resolve; });
    const fetchSpy = vi.spyOn(globalThis, "fetch");
    const put = vi.fn(async ({ request }: { request: Request }) => {
      expect(request.headers.get("content-type")).toMatch(/^multipart\/form-data; boundary=/);
      const form = await request.formData();
      const keys: string[] = [];
      form.forEach((_value, key) => keys.push(key));
      expect(keys).toEqual(["image"]);
      expect(new Uint8Array(await (form.get("image") as File).arrayBuffer())).toEqual(new Uint8Array([255, 216, 255, 217]));
      await waiting;
      return HttpResponse.json({ ...image, revision: "r2" });
    });
    server.use(http.put("*/api/v1/vendors/:id/image", put));
    const { qc } = section({ ...vendor, image });
    const invalidate = vi.spyOn(qc, "invalidateQueries");
    await screen.findByAltText("店家參考照片");
    await choose();
    expect(put).not.toHaveBeenCalled();
    await userEvent.click(screen.getByRole("button", { name: "確認替換照片" }));
    await waitFor(() => expect(put).toHaveBeenCalledTimes(1));
    expect((fetchSpy.mock.calls.find(([req]) => (req as Request).method === "PUT")![0] as Request).credentials).toBe("include");
    expect(screen.getByRole("button", { name: "上傳中…" })).toBeDisabled();
    expect(screen.getByRole("button", { name: "刪除照片" })).toBeDisabled();
    expect(screen.getByAltText("店家參考照片")).toBeVisible();
    await act(async () => release());
    await waitFor(() => expect(screen.queryByAltText("待上傳照片")).not.toBeInTheDocument());
    expect(invalidate).toHaveBeenCalledWith({ queryKey: ["vendor", "v1"] });
    expect(invalidate).toHaveBeenCalledWith({ queryKey: ["vendors"] });
  });

  it("does not flatten APNG before the server rejects it; retains draft and old image for retry", async () => {
    const put = vi.fn(() => problem(422, "invalid_image"));
    server.use(http.put("*/api/v1/vendors/:id/image", put));
    section({ ...vendor, image });
    await screen.findByAltText("店家參考照片");
    await choose(new File(["original APNG bytes"], "animation.png", { type: "image/png" }));
    await userEvent.click(screen.getByRole("button", { name: "確認替換照片" }));
    expect(await screen.findByRole("alert")).toHaveTextContent("不支援動畫與 WebP");
    expect(screen.getByAltText("店家參考照片")).toBeVisible();
    expect(screen.getByAltText("待上傳照片")).toBeVisible();
    server.use(http.put("*/api/v1/vendors/:id/image", () => HttpResponse.json(image)));
    await userEvent.click(screen.getByRole("button", { name: "確認替換照片" }));
    await waitFor(() => expect(screen.queryByAltText("待上傳照片")).not.toBeInTheDocument());
  });

  it.each([
    [413, "image_too_large", "5 MiB"],
    [429, "rate_limited", "請稍候再試"],
    [0, "network", "網路連線失敗"],
  ] as const)("preserves selection and original photo after upload error %s, allowing retry", async (status, code, message) => {
    server.use(http.put("*/api/v1/vendors/:id/image", () => status ? problem(status, code) : HttpResponse.error()));
    section({ ...vendor, image });
    await screen.findByAltText("店家參考照片");
    await choose();
    await userEvent.click(screen.getByRole("button", { name: "確認替換照片" }));
    expect(await screen.findByRole("alert")).toHaveTextContent(message);
    expect(screen.getByAltText("店家參考照片")).toBeVisible();
    expect(screen.getByAltText("待上傳照片")).toBeVisible();
    expect(screen.getByRole("button", { name: "確認替換照片" })).toBeEnabled();
  });

  it("confirms deletion, preserves photo on failure, and invalidates on success", async () => {
    const del = vi.fn(() => problem(500, "internal_error"));
    server.use(http.delete("*/api/v1/vendors/:id/image", del));
    const { qc } = section({ ...vendor, image });
    const invalidate = vi.spyOn(qc, "invalidateQueries");
    await screen.findByAltText("店家參考照片");
    await userEvent.click(screen.getByRole("button", { name: "刪除照片" }));
    await userEvent.click(screen.getByRole("button", { name: "取消" }));
    expect(del).not.toHaveBeenCalled();
    await userEvent.click(screen.getByRole("button", { name: "刪除照片" }));
    await userEvent.click(screen.getByRole("button", { name: "確定" }));
    expect(await screen.findByRole("alert")).toHaveTextContent("伺服器");
    expect(screen.getByAltText("店家參考照片")).toBeVisible();
    server.use(http.delete("*/api/v1/vendors/:id/image", () => new HttpResponse(null, { status: 204 })));
    await userEvent.click(screen.getByRole("button", { name: "刪除照片" }));
    await userEvent.click(screen.getByRole("button", { name: "確定" }));
    await waitFor(() => expect(invalidate).toHaveBeenCalledWith({ queryKey: ["vendors"] }));
  });

  it("read-only members may enlarge photos but cannot change them; loading failure is retryable", async () => {
    server.use(http.get("*/api/v1/vendors/:id/image", () => HttpResponse.error()));
    section({ ...vendor, image, canManage: false });
    expect(await screen.findByRole("alert")).toHaveTextContent("網路連線失敗");
    expect(screen.queryByLabelText("選擇參考照片檔案")).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "刪除照片" })).not.toBeInTheDocument();
    server.use(http.get("*/api/v1/vendors/:id/image", () => new HttpResponse("jpeg")));
    await userEvent.click(screen.getByRole("button", { name: "重新載入照片" }));
    await userEvent.click(await screen.findByRole("button", { name: "參考照片預覽" }));
    expect(await screen.findByRole("dialog")).toBeVisible();
  });

  it.each([401, 403, 404])("withdraws loaded image on mutation %s and invalidates parent", async (status) => {
    server.use(http.delete("*/api/v1/vendors/:id/image", () => problem(status, "not_found")));
    const { qc } = section({ ...vendor, image });
    // A slow parent/access refetch must not delay withdrawing private bytes.
    const invalidate = vi.spyOn(qc, "invalidateQueries").mockImplementation(() => new Promise<void>(() => {}));
    await screen.findByAltText("店家參考照片");
    await userEvent.click(screen.getByRole("button", { name: "刪除照片" }));
    await userEvent.click(screen.getByRole("button", { name: "確定" }));
    await waitFor(() => expect(screen.queryByAltText("店家參考照片")).not.toBeInTheDocument());
    expect(revoke).toHaveBeenCalled();
    expect(invalidate).toHaveBeenCalled();
  });

  it.each([401, 403, 404])("rejects image GET %s without creating an object URL, and rechecks access", async (status) => {
    server.use(http.get("*/api/v1/vendors/:id/image", () => problem(status, "not_found")));
    const { qc } = section({ ...vendor, image, canManage: false });
    const invalidate = vi.spyOn(qc, "invalidateQueries");
    await screen.findByRole("alert");
    expect(screen.queryByAltText("店家參考照片")).not.toBeInTheDocument();
    expect(create).not.toHaveBeenCalled();
    expect(invalidate).toHaveBeenCalledWith({ queryKey: ["vendor", "v1"], exact: true });
  });

  it("aborts fetch and upload on unmount; changing vendor/account revokes old images", async () => {
    const fetchSpy = vi.spyOn(globalThis, "fetch");
    server.use(http.get("*/api/v1/vendors/:id/image", ({ request }) => {
      expect(request.method).toBe("GET");
      return new HttpResponse("jpeg");
    }));
    const view = section({ ...vendor, image });
    await screen.findByAltText("店家參考照片");
    const readSignal = (fetchSpy.mock.calls.find(([req]) => (req as Request).url.endsWith("/image"))![0] as Request).signal;
    view.change({ ...vendor, id: "v2", image: null });
    expect(readSignal?.aborted).toBe(true);
    expect(revoke).toHaveBeenCalledWith("blob:test-1");
    await choose();
    act(() => view.qc.setQueryData(["me"], { ...ME, id: "other" }));
    await waitFor(() => expect(screen.queryByAltText("待上傳照片")).not.toBeInTheDocument());
    server.use(http.put("*/api/v1/vendors/:id/image", () => {
      return new Promise<Response>(() => {});
    }));
    await choose();
    await userEvent.click(screen.getByRole("button", { name: "確認上傳照片" }));
    await waitFor(() => expect(fetchSpy.mock.calls.some(([req]) => (req as Request).method === "PUT")).toBe(true));
    const uploadSignal = (fetchSpy.mock.calls.find(([req]) => (req as Request).method === "PUT")![0] as Request).signal;
    view.unmount();
    expect(uploadSignal?.aborted).toBe(true);
  });

  it("reloads by revision, never refetches when just opening the preview", async () => {
    const get = vi.fn(() => new HttpResponse("jpeg"));
    server.use(http.get("*/api/v1/vendors/:id/image", get));
    const view = section({ ...vendor, image });
    await screen.findByAltText("店家參考照片");
    await userEvent.click(screen.getByRole("button", { name: "參考照片預覽" }));
    await userEvent.keyboard("{Escape}");
    expect(get).toHaveBeenCalledTimes(1);
    view.change({ ...vendor, image: { ...image, revision: "r2" } });
    await waitFor(() => expect(get).toHaveBeenCalledTimes(2));
    expect(revoke).toHaveBeenCalledWith("blob:test-1");
  });

  it("preserves unsaved vendor fields after upload AND deletion change updatedAt", async () => {
    let current = { ...vendor };
    server.use(
      http.get("*/api/v1/vendors/v1", () => HttpResponse.json(current)),
      http.get("*/api/v1/auth/config", () => HttpResponse.json({ aiEnabled: false })),
      http.put("*/api/v1/vendors/v1/image", () => { current = { ...current, image, updatedAt: "2" }; return HttpResponse.json(image); }),
      http.delete("*/api/v1/vendors/v1/image", () => { current = { ...current, image: null, updatedAt: "3" }; return new HttpResponse(null, { status: 204 }); }),
    );
    renderApp("/vendors/v1");
    const name = await screen.findByLabelText("店家名稱");
    await userEvent.type(name, "未儲存");
    await choose();
    await userEvent.click(screen.getByRole("button", { name: "確認上傳照片" }));
    await screen.findByAltText("店家參考照片");
    expect(name).toHaveValue("便當未儲存");
    await userEvent.click(screen.getByRole("button", { name: "刪除照片" }));
    await userEvent.click(within(screen.getByRole("group", { name: "確認刪除" })).getByRole("button", { name: "確定" }));
    await screen.findByText("尚未上傳參考照片");
    expect(screen.getByLabelText("店家名稱")).toHaveValue("便當未儲存");
  });
});
