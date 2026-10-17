import { expect, test, type Page } from "@playwright/test";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import zlib from "node:zlib";
import { randomUUID } from "node:crypto";

/** Dev login; the email is suffixed with the project name so projects never share organizer data. */
export async function devLogin(page: Page, name: string) {
  const email = `${name}.${test.info().project.name}@example.tw`;
  await page.goto("/login");
  await page.fill("#dev-email", email);
  await page.locator("form:has(#dev-email) button[type=submit]").click();
  await expect(page).toHaveURL(/\/$/);
}

export async function expectNoHorizontalOverflow(page: Page) {
  const [scrollWidth, innerWidth] = await page.evaluate(() => [
    document.documentElement.scrollWidth,
    window.innerWidth,
  ]);
  expect(scrollWidth, "horizontal overflow").toBeLessThanOrEqual(innerWidth);
}

async function postJson<T>(page: Page, url: string, data: unknown): Promise<T> {
  const res = await page.request.post(url, { data });
  expect(res.ok(), `${url} -> ${res.status()} ${await res.text()}`).toBeTruthy();
  return (await res.json()) as T;
}

export const createRoster = (page: Page, name: string, members: string[]) =>
  postJson<{ id: string }>(page, "/api/v1/rosters", { name, members: members.map((n) => ({ name: n })) });

export const createVendor = (
  page: Page,
  name: string,
  menuItems: { name: string; price: number }[],
) => postJson<{ id: string }>(page, "/api/v1/vendors", { name, menuItems });

export const createGroupOrder = (page: Page, title: string, rosterId: string, vendorId: string) =>
  postJson<{ id: string; shareUrl: string }>(page, "/api/v1/group-orders", { title, rosterId, vendorId });

export interface PublicGroup {
  participants: { id: string; name: string; hasOrdered: boolean }[];
  menu: { id: string; name: string }[];
}

export async function getPublicGroup(page: Page, shareUrl: string): Promise<PublicGroup> {
  const token = shareUrl.split("/g/")[1];
  const res = await page.request.get(`/api/v1/public/g/${token}`);
  expect(res.ok()).toBeTruthy();
  return (await res.json()) as PublicGroup;
}

/** Public submission via the API; `lines` use item ids. */
export async function submitOrder(
  page: Page,
  shareUrl: string,
  participantId: string,
  lines: { itemId: string; quantity: number }[],
  deviceId: string,
) {
  const token = shareUrl.split("/g/")[1];
  const res = await page.request.post(`/api/v1/public/g/${token}/participants/${participantId}/submissions`, {
    data: { lines, idempotencyKey: randomUUID(), client: { deviceId } },
  });
  expect(res.ok(), `submit -> ${res.status()} ${await res.text()}`).toBeTruthy();
}

function crc32(buf: Buffer): number {
  let c = ~0;
  for (let i = 0; i < buf.length; i++) {
    c ^= buf[i];
    for (let k = 0; k < 8; k++) c = (c >>> 1) ^ (0xedb88320 & -(c & 1));
  }
  return ~c >>> 0;
}

function chunk(type: string, data: Buffer): Buffer {
  const len = Buffer.alloc(4);
  len.writeUInt32BE(data.length);
  const td = Buffer.concat([Buffer.from(type, "ascii"), data]);
  const crc = Buffer.alloc(4);
  crc.writeUInt32BE(crc32(td));
  return Buffer.concat([len, td, crc]);
}

/** Writes a noisy-gradient RGB PNG (default 2400x1600) to a temp file and returns its path. */
export function writeTestPng(width = 2400, height = 1600): string {
  const stride = width * 3 + 1;
  const raw = Buffer.alloc(stride * height);
  let seed = 12345;
  for (let y = 0; y < height; y++) {
    const o = y * stride;
    raw[o] = 0;
    for (let x = 0; x < width; x++) {
      seed = (seed * 1103515245 + 12345) & 0x7fffffff;
      const n = (seed >> 16) & 0x3f;
      raw[o + 1 + x * 3] = ((x * 255) / width + n) & 255;
      raw[o + 2 + x * 3] = ((y * 255) / height + n) & 255;
      raw[o + 3 + x * 3] = (((x + y) * 255) / (width + height) + n) & 255;
    }
  }
  const ihdr = Buffer.alloc(13);
  ihdr.writeUInt32BE(width, 0);
  ihdr.writeUInt32BE(height, 4);
  ihdr[8] = 8; // bit depth
  ihdr[9] = 2; // RGB
  const png = Buffer.concat([
    Buffer.from([0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a]),
    chunk("IHDR", ihdr),
    chunk("IDAT", zlib.deflateSync(raw, { level: 3 })),
    chunk("IEND", Buffer.alloc(0)),
  ]);
  // Lives under the per-run temp dir, which global teardown removes.
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), "bento-e2e-3990", "img-"));
  const file = path.join(dir, "flyer.png");
  fs.writeFileSync(file, png);
  return file;
}
