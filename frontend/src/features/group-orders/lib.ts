import { formatMoney } from "@/shared/lib/format";
import type { GroupStatus, ParticipantRow, SummaryItem } from "@/shared/api/types";

export const STATUS_LABEL: Record<string, string> = {
  open: "進行中",
  closed: "已關閉",
  archived: "已封存",
};

/** Plain-text order for phoning / messaging the shop. Items with quantity 0 are left out. */
export function buildOrderText(input: {
  vendorName: string;
  deliveryAddress?: string | null;
  items: Pick<SummaryItem, "name" | "quantity">[];
  totalQuantity: number;
}): string {
  const lines = input.items.filter((i) => i.quantity > 0).map((i) => `${i.name} ×${i.quantity}`);
  return [
    `您好，${input.vendorName}！`,
    ...(input.deliveryAddress?.trim() ? [`我們這邊是 ${input.deliveryAddress.trim()}。`] : []),
    "想向您訂購以下便當：",
    "",
    ...lines,
    "",
    `共 ${input.totalQuantity} 個`,
    "",
    ...(input.deliveryAddress?.trim() ? ["再麻煩您協助安排外送，"] : []),
    "請問方便接單嗎？謝謝您！",
  ].join("\n");
}

export interface BalanceTag {
  tone: "owe" | "refund";
  text: string;
}

/** 尚差 (positive) / 應退 (negative); nothing for null or 0. */
export function balanceTag(balance: number | null | undefined): BalanceTag | null {
  if (balance == null || balance === 0) return null;
  return balance > 0
    ? { tone: "owe", text: `尚差 ${formatMoney(balance)}` }
    : { tone: "refund", text: `應退 ${formatMoney(-balance)}` };
}

export type ChecklistFilter = "all" | "unpicked" | "unpaid" | "changed";

export const CHECKLIST_FILTERS: { value: ChecklistFilter; label: string }[] = [
  { value: "all", label: "全部" },
  { value: "unpicked", label: "未領" },
  { value: "unpaid", label: "未繳" },
  { value: "changed", label: "有改單" },
];

export const hasOrdered = (r: ParticipantRow) => r.items.length > 0;

export function splitParticipants(rows: ParticipantRow[]) {
  return {
    ordered: rows.filter(hasOrdered),
    notOrdered: rows.filter((r) => !hasOrdered(r)),
  };
}

/** Person-by-person confirmation for the group's chat, using current effective orders. */
export function buildConfirmationText(input: {
  title: string;
  vendorName: string;
  participants: ParticipantRow[];
}): string {
  const { ordered, notOrdered } = splitParticipants(input.participants);
  return [
    `【${input.title}】訂餐確認`,
    `店家：${input.vendorName}`,
    "",
    ...ordered.map((p) => `${p.name}：${p.items.map(itemText).join("、")}${p.orderNote ? `（訂單備註：${p.orderNote}）` : ""}`),
    "",
    `已訂 ${ordered.length} 人，共 ${ordered.reduce((sum, p) => sum + p.items.reduce((n, i) => n + i.quantity, 0), 0)} 個`,
    ...(notOrdered.length ? [`未訂餐：${notOrdered.map((p) => p.name).join("、")}`] : []),
    "",
    "請大家確認自己的餐點與數量是否正確，謝謝！",
  ].join("\n");
}

export function filterChecklist(rows: ParticipantRow[], filter: ChecklistFilter): ParticipantRow[] {
  switch (filter) {
    case "unpicked":
      return rows.filter((r) => !r.pickedUp);
    case "unpaid":
      return rows.filter((r) => !r.paid);
    case "changed":
      return rows.filter((r) => r.submissionCount > 1);
    default:
      return rows;
  }
}

/** `排骨飯 ×2（不要辣）`. */
export function itemText(i: { name: string; quantity: number; note?: string | null }): string {
  return `${i.name} ×${i.quantity}${i.note ? `（${i.note}）` : ""}`;
}

/** Stable colour derived from the 3-char device id, for eyeballing matches. */
export function deviceColor(short: string): string {
  let h = 0;
  for (const ch of short) h = (h * 31 + ch.charCodeAt(0)) % 360;
  return `hsl(${h} 65% 38%)`;
}

export const ALL_STATUSES: GroupStatus[] = ["open", "closed", "archived"];
export const DEFAULT_STATUSES: GroupStatus[] = ["open", "closed"];

/** Next chip state for the status filter (multi-select; never empty). */
export function toggleStatus(current: GroupStatus[], s: GroupStatus): GroupStatus[] {
  if (!current.includes(s)) return ALL_STATUSES.filter((x) => current.includes(x) || x === s);
  return current.length === 1 ? current : current.filter((x) => x !== s);
}

export type TabValue = "stats" | "details" | "checklist";
export function parseTab(raw: string | null): TabValue {
  return raw === "details" || raw === "checklist" ? raw : "stats";
}
