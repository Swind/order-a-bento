const pad = (n: number) => String(n).padStart(2, "0");

/** `HH:mm` in local time; empty string for an invalid date. */
export function formatHHmm(iso: string | Date): string {
  const d = typeof iso === "string" ? new Date(iso) : iso;
  if (Number.isNaN(d.getTime())) return "";
  return `${pad(d.getHours())}:${pad(d.getMinutes())}`;
}

/** `M/D` in local time. */
export function formatMD(d: Date): string {
  return `${d.getMonth() + 1}/${d.getDate()}`;
}

/** Value for `<input type="datetime-local">` (local time, minute precision). */
export function toLocalInputValue(d: Date): string {
  return `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())}T${pad(d.getHours())}:${pad(d.getMinutes())}`;
}

/** Parses a datetime-local value as local time; null when empty/invalid. */
export function parseLocalInput(value: string): Date | null {
  if (!value) return null;
  const d = new Date(value);
  return Number.isNaN(d.getTime()) ? null : d;
}

/** Countdown text: 「剩 42 分」「剩 1 小時 5 分」「剩 1 天 2 小時」「已截止」. */
export function formatCountdown(deadlineIso: string, now: Date = new Date()): string {
  const ms = new Date(deadlineIso).getTime() - now.getTime();
  if (Number.isNaN(ms) || ms <= 0) return "已截止";
  const totalMin = Math.max(1, Math.ceil(ms / 60_000));
  if (totalMin < 60) return `剩 ${totalMin} 分`;
  const hours = Math.floor(totalMin / 60);
  const min = totalMin % 60;
  if (hours >= 24) {
    const days = Math.floor(hours / 24);
    const h = hours % 24;
    return h ? `剩 ${days} 天 ${h} 小時` : `剩 ${days} 天`;
  }
  return min ? `剩 ${hours} 小時 ${min} 分` : `剩 ${hours} 小時`;
}

/** 「截止 11:00（剩 42 分）」; prefixes the date when not today. */
export function formatDeadline(deadlineIso: string, now: Date = new Date()): string {
  const d = new Date(deadlineIso);
  if (Number.isNaN(d.getTime())) return "";
  const sameDay = formatMD(d) === formatMD(now);
  const when = `${sameDay ? "" : `${formatMD(d)} `}${formatHHmm(d)}`;
  return `截止 ${when}（${formatCountdown(deadlineIso, now)}）`;
}
