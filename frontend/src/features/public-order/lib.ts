/** Case-insensitive, full/half-width-insensitive form used for name search. */
export function normalizeSearch(s: string): string {
  return s.normalize("NFKC").toLowerCase().replace(/\s+/g, " ").trim();
}

export function matchesName(name: string, query: string): boolean {
  const q = normalizeSearch(query);
  return q === "" || normalizeSearch(name).includes(q);
}

export interface PendingKey {
  key: string;
  fingerprint: string;
}

/**
 * Idempotency rule: retrying the *same payload* after a failure/timeout reuses the previous key;
 * a changed payload (different fingerprint) or no previous key yields a fresh key. The caller
 * discards the stored value after a success, so the next press always gets a new key.
 */
export function resolveIdempotencyKey(
  prev: PendingKey | null,
  fingerprint: string,
  generate: () => string = () => crypto.randomUUID(),
): PendingKey {
  if (prev && prev.fingerprint === fingerprint) return prev;
  return { key: generate(), fingerprint };
}

const lastKey = (token: string) => `bento:last:${token}`;

export function readLastParticipant(token: string): string | null {
  try {
    return localStorage.getItem(lastKey(token));
  } catch {
    return null;
  }
}

export function writeLastParticipant(token: string, pid: string): void {
  try {
    localStorage.setItem(lastKey(token), pid);
  } catch {
    /* storage unavailable: ordering still works */
  }
}

export const MAX_QTY = 20;
export const ITEM_NOTE_MAX = 50;
export const ORDER_NOTE_MAX = 200;

/** Match submission semantics; display snapshots and line ordering are not edits. */
export function orderContent(lines: readonly { itemId: string; quantity: number; note?: string | null }[], note?: string | null) {
  return JSON.stringify([
    lines.filter((line) => line.quantity > 0)
      .map((line) => ({ itemId: line.itemId, quantity: line.quantity, note: line.note?.trim() || "" }))
      .sort((a, b) => a.itemId.localeCompare(b.itemId)),
    note?.trim() || "",
  ]);
}
