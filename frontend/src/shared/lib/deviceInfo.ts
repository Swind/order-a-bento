import type { ClientHints } from "@/shared/api/types";

const DEVICE_KEY = "bento:device";
let memoryId: string | null = null;

/** Stable per-browser random id; falls back to an in-memory id when storage is unavailable. */
export function getDeviceId(): string {
  try {
    const existing = localStorage.getItem(DEVICE_KEY);
    if (existing) return existing;
    const id = crypto.randomUUID();
    localStorage.setItem(DEVICE_KEY, id);
    return id;
  } catch {
    memoryId ??= crypto.randomUUID();
    return memoryId;
  }
}

interface UaDataLike {
  getHighEntropyValues?: (hints: string[]) => Promise<{ model?: string }>;
}

async function getDeviceModel(): Promise<string | undefined> {
  try {
    const uaData = (navigator as unknown as { userAgentData?: UaDataLike }).userAgentData;
    const values = await uaData?.getHighEntropyValues?.(["model"]);
    return values?.model || undefined;
  } catch {
    return undefined;
  }
}

/** Forgeable device hints attached to each submission. Never throws. */
export async function collectClientInfo(): Promise<ClientHints> {
  const info: ClientHints = {};
  try {
    info.deviceId = getDeviceId();
  } catch {
    /* ignore */
  }
  info.deviceModel = await getDeviceModel();
  try {
    info.screen = `${screen.width}x${screen.height}`;
  } catch {
    /* ignore */
  }
  try {
    info.language = navigator.language;
  } catch {
    /* ignore */
  }
  try {
    info.timezone = Intl.DateTimeFormat().resolvedOptions().timeZone;
  } catch {
    /* ignore */
  }
  return info;
}
