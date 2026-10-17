/** `1234` -> `$1,234`. */
export function formatMoney(amount: number): string {
  return `$${Math.trunc(amount).toLocaleString("en-US")}`;
}

/** Relative time for recent timestamps (zh-TW), local date otherwise. */
export function formatUpdatedAt(iso: string, now: Date = new Date()): string {
  const t = new Date(iso);
  if (Number.isNaN(t.getTime())) return "";
  const diffSec = Math.round((t.getTime() - now.getTime()) / 1000);
  const abs = Math.abs(diffSec);
  const rtf = new Intl.RelativeTimeFormat("zh-TW", { numeric: "auto" });
  if (abs < 60) return "剛剛";
  if (abs < 3600) return rtf.format(Math.round(diffSec / 60), "minute");
  if (abs < 86400) return rtf.format(Math.round(diffSec / 3600), "hour");
  if (abs < 7 * 86400) return rtf.format(Math.round(diffSec / 86400), "day");
  return t.toLocaleDateString("zh-TW", { year: "numeric", month: "2-digit", day: "2-digit" });
}

/** Only same-origin absolute paths are accepted (mirrors the backend rule). */
export function safeReturnTo(raw: string | null | undefined): string {
  if (!raw || !raw.startsWith("/") || raw.startsWith("//") || raw.startsWith("/\\")) return "/";
  return raw;
}
