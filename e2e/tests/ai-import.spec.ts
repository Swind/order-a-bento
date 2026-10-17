import { expect, test } from "@playwright/test";
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
