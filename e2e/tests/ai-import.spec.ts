import { expect, test, type Page } from "@playwright/test";
import { execFileSync } from "node:child_process";
import os from "node:os";
import path from "node:path";
import { createVendor, devLogin, expectNoHorizontalOverflow, writeTestPng } from "../helpers";

/** Long side of a JPEG found inside a multipart body (reads the first SOF0/SOF2 marker). */
function jpegSize(buf: Buffer): { width: number; height: number } | null {
  const start = buf.indexOf(Buffer.from([0xff, 0xd8, 0xff]));
  if (start < 0) return null;
  for (let k = start; k < buf.length - 9; k++) {
    if (buf[k] === 0xff && (buf[k + 1] === 0xc0 || buf[k + 1] === 0xc2)) {
      return { height: buf.readUInt16BE(k + 5), width: buf.readUInt16BE(k + 7) };
    }
  }
  return null;
}

test("flyer upload is downscaled to JPEG and applied to the vendor menu", async ({ page }) => {
  const png = writeTestPng();
  await devLogin(page, "ai-import");
  const vendor = await createVendor(page, "阿財便當", [{ name: "排骨飯", price: 100 }]);

  let part: { type?: string; size: { width: number; height: number } | null } | null = null;
  await page.route("**/api/v1/ai/import-runs", async (route) => {
    const buf = route.request().postDataBuffer();
    if (route.request().method() === "POST" && buf) {
      part = {
        type: buf.toString("latin1").match(/Content-Type: (image\/[\w+]+)/)?.[1],
        size: jpegSize(buf),
      };
    }
    await route.continue();
  });

  await page.goto(`/vendors/${vendor.id}`);
  await page.getByRole("button", { name: /從傳單匯入/ }).click();
  await expect(page.getByText("示範模式").first()).toBeVisible();
  await page.getByLabel("選擇照片檔案").setInputFiles(png);
  await page.getByRole("button", { name: "開始辨識" }).click();
  await expect(page).toHaveURL(/\/ai-import\/[0-9a-f-]+$/, { timeout: 15_000 });

  expect(part).not.toBeNull();
  expect(part!.type).toBe("image/jpeg");
  expect(part!.size).not.toBeNull();
  expect(Math.max(part!.size!.width, part!.size!.height)).toBeLessThanOrEqual(1568);

  // Confirm screen: apply is blocked until the priceless 今日特餐 gets a price.
  const apply = page.getByRole("button", { name: /套用 \d+ 筆變更/ });
  await expect(apply).toBeDisabled();
  await expect(page.getByText("今日特餐").first()).toBeVisible();
  await expectNoHorizontalOverflow(page);

  const priceInputs = page.locator('input[inputmode="numeric"], input[type="number"]');
  const n = await priceInputs.count();
  let filled = false;
  for (let i = 0; i < n; i++) {
    if ((await priceInputs.nth(i).inputValue()) === "") {
      await priceInputs.nth(i).fill("120");
      filled = true;
      break;
    }
  }
  expect(filled, "an empty price input").toBe(true);
  await expect(apply).toBeEnabled();

  await apply.click();
  await expect(page).toHaveURL(new RegExp(`/vendors/${vendor.id}`));
  await expect(page.getByText("今日特餐").first()).toBeVisible();

  const res = await page.request.get(`/api/v1/vendors/${vendor.id}`);
  const menu = (await res.json()).menuItems as { name: string; price: number }[];
  expect(menu.find((m) => m.name === "今日特餐")?.price).toBe(120);
  expect(menu.find((m) => m.name === "排骨飯")?.price).toBe(105);
});

const photoLabel = "確認匯入後，同時儲存菜單照片";

// Only the harness's disposable database; never inspect or mutate backend/data.
function pendingPhoto(runId: string, expire = false): { count: number; hex: string | null } {
  return JSON.parse(execFileSync("python3", ["-c", `
import sqlite3, sys, json
db = sqlite3.connect(sys.argv[1], timeout=10)
if sys.argv[3] == 'expire':
    db.execute("UPDATE ai_import_photos SET expires_at='2000-01-01T00:00:00Z' WHERE run_id=?", (sys.argv[2],))
    db.commit()
rows = db.execute('SELECT hex(data) FROM ai_import_photos WHERE run_id=?', (sys.argv[2],)).fetchall()
print(json.dumps({'count': len(rows), 'hex': rows[0][0] if rows else None}))
`, path.join(os.tmpdir(), "bento-e2e-3990", "e2e.db"), runId, expire ? "expire" : "read"], { encoding: "utf8" }));
}

async function canvasFile(page: Page, type: "image/webp" | "image/png") {
  const base64 = await page.evaluate((mime) => {
    const canvas = document.createElement("canvas");
    canvas.width = 96;
    canvas.height = 64;
    const ctx = canvas.getContext("2d")!;
    // Most pixels remain transparent: the input must be flattened white, not black.
    ctx.fillStyle = "#dc2626";
    ctx.fillRect(32, 16, 32, 32);
    return canvas.toDataURL(mime).split(",")[1];
  }, type);
  const buffer = Buffer.from(base64, "base64");
  if (type === "image/webp") expect(buffer.subarray(8, 12).toString()).toBe("WEBP");
  else expect(buffer.subarray(1, 4).toString()).toBe("PNG");
  return { name: type === "image/webp" ? "actual-input.webp" : "transparent.png", mimeType: type, buffer };
}

async function readyToApply(page: Page) {
  await expect(page.getByRole("heading", { name: "AI 匯入確認" })).toBeVisible();
  for (const input of await page.locator('input[inputmode="numeric"], input[type="number"]').all()) {
    if ((await input.inputValue()) === "") await input.fill("120");
  }
  await expect(page.getByRole("button", { name: /套用 \d+ 筆變更/ })).toBeEnabled();
}

async function startPhoto(page: Page, vendorId?: string, savePhoto = true, type: "image/webp" | "image/png" = "image/png") {
  await page.goto(vendorId ? `/vendors/${vendorId}` : "/vendors");
  await page.getByRole("button", { name: vendorId ? /從傳單匯入/ : "從傳單建立店家" }).click();
  await expect(page.getByLabel(photoLabel)).toBeChecked();
  if (!savePhoto) await page.getByLabel(photoLabel).uncheck();
  const file = await canvasFile(page, type);
  await page.getByLabel("選擇照片檔案").setInputFiles(file);
  let request: Buffer | null = null;
  await page.route("**/api/v1/ai/import-runs", async (route) => {
    request = route.request().postDataBuffer();
    await route.continue();
  }, { times: 1 });
  const response = page.waitForResponse((r) => r.url().endsWith("/api/v1/ai/import-runs") && r.request().method() === "POST");
  await page.getByRole("button", { name: "開始辨識" }).click();
  const res = await response;
  expect(res.ok(), await res.text()).toBeTruthy();
  const run = await res.json();
  expect(run.hasPendingPhoto).toBe(savePhoto);
  expect(run.pendingPhotoExpiresAt !== null).toBe(savePhoto);
  expect(run).not.toHaveProperty("data");
  expect(run).not.toHaveProperty("image");
  expect(request!.toString("latin1")).toContain("Content-Type: image/jpeg");
  expect(jpegSize(request!)).toEqual({ width: 96, height: 64 });
  await expect(page).toHaveURL(new RegExp(`/ai-import/${run.id}$`));
  return run;
}

async function detail(page: Page, id: string) {
  const res = await page.request.get(`/api/v1/vendors/${id}`);
  expect(res.ok()).toBeTruthy();
  return res.json();
}

async function seedPhoto(page: Page, id: string) {
  const res = await page.request.put(`/api/v1/vendors/${id}/image`, {
    multipart: { image: { name: "old.png", mimeType: "image/png", buffer: (await canvasFile(page, "image/png")).buffer } },
  });
  expect(res.ok(), await res.text()).toBeTruthy();
  return res.json();
}

for (const source of ["new WebP", "existing transparent PNG"] as const) {
  test(`AI default photo survives refresh and atomically saves ${source}`, async ({ page, context }) => {
    await devLogin(page, `ai-photo-${source.split(" ")[0]}`);
    const vendor = source.startsWith("existing") ? await createVendor(page, "既有 AI 照片店", [{ name: "排骨飯", price: 100 }]) : null;
    const old = vendor ? await seedPhoto(page, vendor.id) : null;
    const imageMutations: string[] = [];
    page.on("request", (request) => {
      if (/\/vendors\/[^/]+\/image$/.test(request.url()) && request.method() !== "GET") imageMutations.push(request.method());
    });
    const run = await startPhoto(page, vendor?.id, true, vendor ? "image/png" : "image/webp");
    const pending = pendingPhoto(run.id);
    expect(pending.count).toBe(1);
    expect(pending.hex?.startsWith("FFD8FF")).toBe(true);
    const outsider = await context.browser()!.newContext();
    const denied = await outsider.request.get(new URL(`/api/v1/ai/import-runs/${run.id}`, page.url()).href);
    expect(denied.status()).toBe(401);
    await outsider.close();
    await page.reload();
    await expect(page.getByLabel(photoLabel)).toBeChecked();
    await readyToApply(page);
    if (vendor) {
      expect((await detail(page, vendor.id)).image.revision).toBe(old.revision);
      await expect(page.getByText("將取代目前參考照片，可取消勾選。")).toBeVisible();
    }
    const label = page.getByLabel(photoLabel).locator("..");
    const box = await label.boundingBox();
    expect(box!.height).toBeGreaterThanOrEqual(44);
    await label.scrollIntoViewIfNeeded();
    await expectNoHorizontalOverflow(page);
    await page.screenshot({ path: test.info().outputPath(`ai-${vendor ? "existing" : "new"}-confirm.png`), fullPage: false });
    const applyResponse = page.waitForResponse((r) => r.url().endsWith(`/${run.id}/apply`));
    await page.getByRole("button", { name: /套用 \d+ 筆變更/ }).click();
    const applied = await applyResponse;
    expect(applied.request().postDataJSON().savePhoto).toBe(true);
    expect(applied.ok(), await applied.text()).toBeTruthy();
    const id = (await applied.json()).vendorIds[0];
    if (vendor) expect(id).toBe(vendor.id);
    await expect(page).toHaveURL(new RegExp(`/vendors/${id}$`));
    await page.reload();
    const saved = await detail(page, id);
    expect(saved.image).toMatchObject({ contentType: "image/jpeg", width: 96, height: 64 });
    if (old) expect(saved.image.revision).not.toBe(old.revision);
    const bytes = await (await page.request.get(`/api/v1/vendors/${id}/image`)).body();
    // Compare with normalized server pending bytes, not the pre-normalization Canvas JPEG.
    expect(bytes.toString("hex").toUpperCase()).toBe(pending.hex);
    expect(bytes.length).toBe(saved.image.sizeBytes);
    expect(imageMutations, "AI apply must not use a second image mutation").toEqual([]);
    expect(pendingPhoto(run.id).count).toBe(0);
    const img = page.getByAltText("店家參考照片", { exact: true });
    await expect(img).toHaveJSProperty("naturalWidth", 96);
    const pixel = await img.evaluate((el) => {
      const c = document.createElement("canvas"); c.width = 96; c.height = 64;
      const ctx = c.getContext("2d")!; ctx.drawImage(el as HTMLImageElement, 0, 0);
      return [...ctx.getImageData(4, 4, 1, 1).data];
    });
    expect(pixel.slice(0, 3).every((v) => v >= 245), `white background: ${pixel}`).toBe(true);
    await img.scrollIntoViewIfNeeded();
    await page.screenshot({ path: test.info().outputPath(`ai-${vendor ? "existing" : "new"}-saved.png`), fullPage: false });
  });
}

test("AI cancelling photo, start opt-out and discard preserve the existing image and clear pending", async ({ page }) => {
  await devLogin(page, "ai-photo-cancel");
  const vendor = await createVendor(page, "取消保存店", [{ name: "排骨飯", price: 100 }]);
  const old = await seedPhoto(page, vendor.id);
  const run = await startPhoto(page, vendor.id);
  await readyToApply(page);
  await page.getByLabel(photoLabel).uncheck();
  await page.getByRole("button", { name: /套用 \d+ 筆變更/ }).click();
  await expect(page).toHaveURL(new RegExp(`/vendors/${vendor.id}$`));
  expect((await detail(page, vendor.id)).image.revision).toBe(old.revision);
  expect(pendingPhoto(run.id).count).toBe(0);
  const optedOut = await startPhoto(page, vendor.id, false);
  expect(pendingPhoto(optedOut.id).count).toBe(0);
  await expect(page.getByLabel(photoLabel)).not.toBeChecked();
  const discarded = await startPhoto(page, vendor.id);
  await page.getByRole("button", { name: "放棄", exact: true }).click();
  await page.getByRole("button", { name: "確定放棄", exact: true }).click();
  await expect(page).toHaveURL(new RegExp(`/vendors/${vendor.id}$`));
  expect(pendingPhoto(discarded.id).count).toBe(0);
  const fetched = await (await page.request.get(`/api/v1/ai/import-runs/${discarded.id}`)).json();
  expect(fetched).toMatchObject({ status: "discarded", hasPendingPhoto: false, pendingPhotoExpiresAt: null });
  expect((await detail(page, vendor.id)).image.revision).toBe(old.revision);
});

test("AI expired server pending rejects photo without false success and permits menu only", async ({ page }) => {
  await devLogin(page, "ai-photo-expiry");
  const vendor = await createVendor(page, "到期照片店", [{ name: "排骨飯", price: 100 }]);
  const old = await seedPhoto(page, vendor.id);
  const run = await startPhoto(page, vendor.id);
  await readyToApply(page);
  pendingPhoto(run.id, true);
  const response = page.waitForResponse((r) => r.url().endsWith(`/${run.id}/apply`));
  await page.getByRole("button", { name: /套用 \d+ 筆變更/ }).click();
  expect((await response).status()).toBe(422);
  await expect(page.getByText(/沒有待存照片（未暫存或已到期）/)).toBeVisible();
  await expect(page.getByRole("button", { name: /套用 \d+ 筆變更/ })).toBeDisabled();
  expect((await detail(page, vendor.id)).menuItems).toHaveLength(1);
  expect((await detail(page, vendor.id)).image.revision).toBe(old.revision);
  await page.getByLabel(photoLabel).uncheck();
  await page.getByRole("button", { name: /套用 \d+ 筆變更/ }).click();
  await expect(page).toHaveURL(new RegExp(`/vendors/${vendor.id}$`));
  expect((await detail(page, vendor.id)).menuItems.length).toBeGreaterThan(1);
  expect((await detail(page, vendor.id)).image.revision).toBe(old.revision);
  expect(pendingPhoto(run.id).count).toBe(0);
});

test("AI lost committed response is verified once without duplicate menu or photo writes", async ({ page }) => {
  await devLogin(page, "ai-photo-lost-response");
  const run = await startPhoto(page);
  await readyToApply(page);
  let calls = 0;
  let vendorId = "";
  await page.route(`**/api/v1/ai/import-runs/${run.id}/apply`, async (route) => {
    calls++;
    const response = await route.fetch();
    expect(response.ok()).toBeTruthy();
    vendorId = (await response.json()).vendorIds[0];
    await route.abort("failed");
  });
  await page.getByRole("button", { name: /套用 \d+ 筆變更/ }).click();
  await expect(page.getByRole("heading", { name: "已套用", exact: true })).toBeVisible();
  expect(calls).toBe(1);
  const saved = await detail(page, vendorId);
  expect(saved.image).not.toBeNull();
  expect(new Set(saved.menuItems.map((item: { name: string }) => item.name)).size).toBe(saved.menuItems.length);
  expect(pendingPhoto(run.id).count).toBe(0);
  await page.reload();
  await expect(page.getByRole("heading", { name: "已套用", exact: true })).toBeVisible();
  expect(calls).toBe(1);
  expect((await detail(page, vendorId)).image.revision).toBe(saved.image.revision);
});

test("AI failed apply keeps draft, pending and old image instead of claiming success", async ({ page }) => {
  await devLogin(page, "ai-photo-failed");
  const vendor = await createVendor(page, "失敗保留店", [{ name: "排骨飯", price: 100 }]);
  const old = await seedPhoto(page, vendor.id);
  const run = await startPhoto(page, vendor.id);
  await readyToApply(page);
  await page.route(`**/api/v1/ai/import-runs/${run.id}/apply`, (route) => route.fulfill({ status: 500, contentType: "application/problem+json", body: JSON.stringify({ code: "internal_error", status: 500, title: "E2E failure" }) }));
  const response = page.waitForResponse((r) => r.url().endsWith(`/${run.id}/apply`));
  await page.getByRole("button", { name: /套用 \d+ 筆變更/ }).click();
  expect((await response).status()).toBe(500);
  await expect(page.getByRole("button", { name: /套用 \d+ 筆變更/ })).toBeEnabled();
  await expect(page.getByRole("heading", { name: "已套用", exact: true })).toHaveCount(0);
  await expect(page.getByLabel(photoLabel)).toBeChecked();
  expect((await detail(page, vendor.id)).menuItems).toHaveLength(1);
  expect((await detail(page, vendor.id)).image.revision).toBe(old.revision);
  expect(pendingPhoto(run.id).count).toBe(1);
});

test("AI unverifiable apply result blocks resubmission and preserves the confirmation draft", async ({ page }) => {
  await devLogin(page, "ai-photo-unverified");
  const run = await startPhoto(page);
  await readyToApply(page);
  const failure = { status: 503, contentType: "application/problem+json", body: JSON.stringify({ code: "internal_error", status: 503, title: "E2E unavailable" }) };
  await page.route(`**/api/v1/ai/import-runs/${run.id}/apply`, (route) => route.fulfill(failure));
  await page.route(`**/api/v1/ai/import-runs/${run.id}`, (route) => route.fulfill(failure));
  await page.getByRole("button", { name: /套用 \d+ 筆變更/ }).click();
  await expect(page.getByText(/目前無法確認匯入結果/)).toBeVisible({ timeout: 15_000 });
  await expect(page.getByRole("heading", { name: "已套用", exact: true })).toHaveCount(0);
  await expect(page.getByRole("button", { name: /套用 \d+ 筆變更/ })).toBeDisabled();
  await expect(page.getByRole("button", { name: "重試查詢" })).toBeVisible();
  await expect(page.getByLabel(photoLabel)).toBeChecked();
  await expect(page.locator('input[inputmode="numeric"]').last()).toHaveValue("120");
  expect(pendingPhoto(run.id).count).toBe(1);
});

for (const mode of ["roster", "deliveryAddress"] as const) {
  test(`AI ${mode} does not offer or retain a photo`, async ({ page }) => {
    await devLogin(page, `ai-no-photo-${mode}`);
    await page.goto(mode === "roster" ? "/rosters" : "/delivery-addresses");
    await page.getByRole("button", { name: /從照片/ }).click();
    await expect(page.getByLabel(photoLabel)).toHaveCount(0);
    await page.getByLabel("選擇照片檔案").setInputFiles(await canvasFile(page, "image/png"));
    let upload: Buffer | null = null;
    await page.route("**/api/v1/ai/import-runs", async (route) => {
      upload = route.request().postDataBuffer();
      await route.continue();
    }, { times: 1 });
    const response = page.waitForResponse((r) => r.url().endsWith("/api/v1/ai/import-runs") && r.request().method() === "POST");
    await page.getByRole("button", { name: "開始辨識" }).click();
    const res = await response;
    expect(res.ok(), await res.text()).toBeTruthy();
    expect(upload!.toString("latin1")).not.toContain('name="savePhoto"');
    const run = await res.json();
    expect(run).toMatchObject({ hasPendingPhoto: false, pendingPhotoExpiresAt: null });
    expect(pendingPhoto(run.id).count).toBe(0);
    await expect(page).toHaveURL(new RegExp(`/ai-import/${run.id}$`));
    await expect(page.getByLabel(photoLabel)).toHaveCount(0);
  });
}
