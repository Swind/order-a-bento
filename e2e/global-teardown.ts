import fs from "node:fs";
import os from "node:os";
import path from "node:path";

export default async function globalTeardown() {
  fs.rmSync(path.join(os.tmpdir(), "bento-e2e-3990"), { recursive: true, force: true });
}
