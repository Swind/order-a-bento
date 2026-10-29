// Manual integration check: node vendor-image-persistence.mjs /absolute/path/to/new/server
// Uses only disposable /tmp/opencode data; never backend/data or a deployed volume.
import assert from "node:assert/strict";
import { spawn } from "node:child_process";
import { once } from "node:events";
import fs from "node:fs/promises";
import path from "node:path";
import { setTimeout } from "node:timers/promises";
import { request } from "@playwright/test";

const binary = path.resolve(process.argv[2]);
const root = await fs.mkdtemp("/tmp/opencode/vendor-image-persistence-");
const source = path.join(root, "source");
const restored = path.join(root, "restored");
await fs.mkdir(source);
await fs.mkdir(restored);
const baseURL = "http://localhost:3991";
const api = await request.newContext({ baseURL, extraHTTPHeaders: { Origin: baseURL } });
let server;
async function start(directory) {
  const log = await fs.open(path.join(root, "server.log"), "a");
  server = spawn(binary, [], {
    env: { ...process.env, PORT: "3991", APP_ENV: "development", DEV_LOGIN: "true",
      DATABASE_URL: `sqlite://${directory}/bento.db?mode=rwc`, APP_BASE_URL: baseURL,
      OIDC_ISSUER_URL: "", OIDC_CLIENT_ID: "", ADMIN_EMAIL: "", ADMIN_PASSWORD: "",
      PASSWORD_LOGIN_ENABLED: "false", AI_API_KEY: "", ANTHROPIC_API_KEY: "", AI_DEMO: "false" },
    stdio: ["ignore", log.fd, log.fd],
  });
  await log.close();
  for (let i = 0; i < 100; i++) {
    assert.equal(server.exitCode, null, "server exited during startup");
    try { if ((await api.get("/healthz")).ok()) return; } catch {}
    await setTimeout(100);
  }
  throw new Error("server failed to start");
}
async function stop() {
  const exited = once(server, "exit");
  server.kill("SIGTERM");
  await exited;
  server = undefined;
}
async function json(response, status = 200) {
  assert.equal(response.status(), status, await response.text());
  return response.json();
}
try {
  await start(source);
  assert.equal((await api.post("/auth/dev-login", { data: { email: "persistence@example.tw" } })).status(), 204);
  const vendor = await json(await api.post("/api/v1/vendors", {
    data: { name: "Persistent private menu", menuItems: [{ name: "Lunch", price: 100 }] },
  }), 201);
  const endpoint = `/api/v1/vendors/${vendor.id}/image`;
  const png = await fs.readFile(new URL("../eval/cases/03-vendor-flyer/image.png", import.meta.url));
  const metadata = await json(await api.put(endpoint, {
    multipart: { image: { name: "menu.png", mimeType: "image/png", buffer: png } },
  }));
  const before = await api.get(endpoint);
  assert.equal(before.status(), 200);
  const bytes = await before.body();
  assert.equal(bytes.length, metadata.sizeBytes);
  const check = async () => {
    const detail = await json(await api.get(`/api/v1/vendors/${vendor.id}`));
    assert.deepEqual(detail.image, metadata);
    const read = await api.get(endpoint);
    assert.equal(read.status(), 200);
    assert.deepEqual(await read.body(), bytes);
    assert.equal(read.headers()["cache-control"], "private, no-store");
  };
  await stop();
  await start(source);
  await check();
  await stop();
  // Same deployment procedure: stop, archive entire directory (including any WAL/SHM), restore elsewhere.
  async function tar(args) {
    const child = spawn("tar", args, { stdio: "inherit" });
    assert.equal((await once(child, "exit"))[0], 0);
  }
  const archive = path.join(root, "backup.tgz");
  await tar(["czf", archive, "-C", source, "."]);
  await tar(["xzf", archive, "-C", restored]);
  await start(restored);
  await check();
  // Validate release assets really match the newly built frontend.
  const html = await fs.readFile(new URL("../frontend/dist/index.html", import.meta.url), "utf8");
  assert.equal(await (await api.get("/")).text(), html);
  const asset = html.match(/src="([^"]+\.js)"/)[1];
  const built = await fs.readFile(new URL(`../frontend/dist${asset}`, import.meta.url));
  assert.deepEqual(await (await api.get(asset)).body(), built);
  console.log(JSON.stringify({ root, vendorId: vendor.id, metadata, result: "restart, archive/restore, embedded assets passed" }, null, 2));
} finally {
  if (server) await stop();
  await api.dispose();
}
