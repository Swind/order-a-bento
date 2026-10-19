import type {
  AcceptedChange,
  ChangeAction,
  Confidence,
  ImportIssue,
  ImportProposal,
  TargetRef,
} from "@/shared/api/types";
import type { ProblemFieldError } from "@/shared/api/client";

/** Same limits as the backend domain types. */
export const LIMITS = {
  name: 50,
  note: 200,
  description: 200,
  category: 30,
  phone: 30,
  address: 200,
  priceMin: 1,
  priceMax: 100_000,
} as const;

const ISSUE_MESSAGES: Record<ImportIssue, string> = {
  price_missing: "請填寫價格",
  price_out_of_range: `價格需為 ${LIMITS.priceMin}–${LIMITS.priceMax} 的整數`,
  name_empty: "名稱不可為空白",
  name_too_long: `名稱不可超過 ${LIMITS.name} 字`,
  note_too_long: `備註不可超過 ${LIMITS.note} 字`,
  description_too_long: `說明不可超過 ${LIMITS.description} 字`,
  category_too_long: `分類不可超過 ${LIMITS.category} 字`,
  duplicate_in_input: "名稱與其他項目重複",
};

export function issueMessage(issue: ImportIssue): string {
  return ISSUE_MESSAGES[issue];
}

// ---------------------------------------------------------------------------
// UI state
// ---------------------------------------------------------------------------

export interface MemberRow {
  action: ChangeAction;
  name: string;
  note: string;
  beforeNote: string | null;
  confidence: Confidence;
  checked: boolean;
}

export interface MenuRow {
  action: ChangeAction;
  name: string;
  /** Raw input text (may be empty / invalid while editing). */
  price: string;
  description: string;
  category: string;
  before: { price: number; description?: string | null; category?: string | null } | null;
  confidence: Confidence;
  checked: boolean;
}

export interface InfoSnapshot {
  name: string;
  phone?: string | null;
  address?: string | null;
  note?: string | null;
}

export interface InfoRow {
  action: ChangeAction;
  before: InfoSnapshot | null;
  name: string;
  phone: string;
  address: string;
  note: string;
  checked: boolean;
}

export interface RosterTargetState {
  type: "roster";
  ref: TargetRef;
  members: MemberRow[];
}

export interface VendorTargetState {
  type: "vendor";
  ref: TargetRef;
  info: InfoRow;
  menu: MenuRow[];
}

export type TargetState = RosterTargetState | VendorTargetState;

export function initState(proposal: ImportProposal): TargetState[] {
  return proposal.targets.filter((t) => t.type !== "deliveryAddress").map((t): TargetState => {
    if (t.type === "roster") {
      return {
        type: "roster",
        ref: t.roster,
        members: t.members.map((m) => ({
          action: m.action,
          name: m.name,
          note: m.note ?? "",
          beforeNote: m.beforeNote ?? null,
          confidence: m.confidence,
          checked: m.action !== "unchanged",
        })),
      };
    }
    const isNew = t.vendor.kind === "new";
    return {
      type: "vendor",
      ref: t.vendor,
      info: {
        action: t.info.action,
        before: t.info.before ?? null,
        name: t.info.after.name,
        phone: t.info.after.phone ?? "",
        address: t.info.after.address ?? "",
        note: t.info.after.note ?? "",
        checked: isNew || t.info.action !== "unchanged",
      },
      menu: t.menu.map((m) => ({
        action: m.action,
        name: m.name,
        price: m.price == null ? "" : String(m.price),
        description: m.description ?? "",
        category: m.category ?? "",
        before: m.before ?? null,
        confidence: m.confidence,
        checked: m.action !== "unchanged",
      })),
    };
  });
}

// ---------------------------------------------------------------------------
// Validation
// ---------------------------------------------------------------------------

const len = (s: string) => [...s.trim()].length;

/** Parses the price input; `null` when empty, `NaN` when not a plain integer. */
export function parsePrice(raw: string): number | null {
  const t = raw.trim();
  if (t === "") return null;
  return /^\d+$/.test(t) ? Number(t) : Number.NaN;
}

function dupNames(rows: { name: string; checked: boolean; action: ChangeAction }[]): Set<string> {
  const seen = new Set<string>();
  const dup = new Set<string>();
  for (const r of rows) {
    if (!r.checked || r.action === "unchanged") continue;
    const k = r.name.trim();
    if (!k) continue;
    if (seen.has(k)) dup.add(k);
    seen.add(k);
  }
  return dup;
}

function nameIssue(name: string): ImportIssue | null {
  if (len(name) === 0) return "name_empty";
  if (len(name) > LIMITS.name) return "name_too_long";
  return null;
}

export function memberIssues(row: MemberRow, members: MemberRow[]): ImportIssue[] {
  const out: ImportIssue[] = [];
  const n = nameIssue(row.name);
  if (n) out.push(n);
  if (len(row.note) > LIMITS.note) out.push("note_too_long");
  if (row.checked && dupNames(members).has(row.name.trim())) out.push("duplicate_in_input");
  return out;
}

export function menuIssues(row: MenuRow, menu: MenuRow[]): ImportIssue[] {
  const out: ImportIssue[] = [];
  const n = nameIssue(row.name);
  if (n) out.push(n);
  const price = parsePrice(row.price);
  if (price === null) out.push("price_missing");
  else if (Number.isNaN(price) || price < LIMITS.priceMin || price > LIMITS.priceMax)
    out.push("price_out_of_range");
  if (len(row.description) > LIMITS.description) out.push("description_too_long");
  if (len(row.category) > LIMITS.category) out.push("category_too_long");
  if (row.checked && dupNames(menu).has(row.name.trim())) out.push("duplicate_in_input");
  return out;
}

export type InfoField = "name" | "phone" | "address" | "note";

/** Field-level messages for the vendor info section. */
export function infoErrors(info: InfoRow, isNew: boolean): Partial<Record<InfoField, string>> {
  const out: Partial<Record<InfoField, string>> = {};
  if (isNew) {
    const n = nameIssue(info.name);
    if (n) out.name = issueMessage(n);
  }
  if (len(info.phone) > LIMITS.phone) out.phone = `電話不可超過 ${LIMITS.phone} 字`;
  if (len(info.address) > LIMITS.address) out.address = `地址不可超過 ${LIMITS.address} 字`;
  if (len(info.note) > LIMITS.note) out.note = issueMessage("note_too_long");
  return out;
}

export function targetNameIssue(ref: TargetRef): string | null {
  if (ref.kind !== "new") return null;
  const n = nameIssue(ref.name);
  return n ? issueMessage(n) : null;
}

const isNewVendor = (t: VendorTargetState) => t.ref.kind === "new";

function infoIncluded(t: VendorTargetState): boolean {
  return t.info.checked && (isNewVendor(t) || t.info.action !== "unchanged");
}

/** Number of checked changes (info counts as one). */
export function countChanges(state: TargetState[]): number {
  let n = 0;
  for (const t of state) {
    if (t.type === "roster") n += t.members.filter((m) => m.checked && m.action !== "unchanged").length;
    else n += (infoIncluded(t) ? 1 : 0) + t.menu.filter((m) => m.checked && m.action !== "unchanged").length;
  }
  return n;
}

/** True if any checked change still has an unresolved issue. */
export function hasBlockingIssues(state: TargetState[]): boolean {
  for (const t of state) {
    if (t.type === "roster") {
      const changes = t.members.filter((m) => m.checked && m.action !== "unchanged");
      if (changes.length > 0 && targetNameIssue(t.ref)) return true;
      if (t.members.some((m) => m.checked && m.action !== "unchanged" && memberIssues(m, t.members).length > 0))
        return true;
    } else {
      const anyMenu = t.menu.some((m) => m.checked && m.action !== "unchanged");
      if (infoIncluded(t) && Object.keys(infoErrors(t.info, isNewVendor(t))).length > 0) return true;
      if ((anyMenu || infoIncluded(t)) && isNewVendor(t) && targetNameIssue({ kind: "new", name: t.info.name }))
        return true;
      if (t.menu.some((m) => m.checked && m.action !== "unchanged" && menuIssues(m, t.menu).length > 0))
        return true;
    }
  }
  return false;
}

// ---------------------------------------------------------------------------
// Apply body
// ---------------------------------------------------------------------------

/** For `accepted[i]`: which UI target it came from and which UI rows its arrays hold. */
export interface AcceptedMap {
  target: number;
  members: number[];
  menu: number[];
}

const opt = (s: string): string | undefined => {
  const t = s.trim();
  return t === "" ? undefined : t;
};

/**
 * Builds the `accepted` array from the checked rows (edited values included).
 * Unchanged rows are never sent; targets without any checked change are skipped.
 * `map` records where each accepted entry/row came from (for mapping 422 paths back).
 */
export function buildApplyBody(state: TargetState[]): { accepted: AcceptedChange[]; map: AcceptedMap[] } {
  const accepted: AcceptedChange[] = [];
  const map: AcceptedMap[] = [];
  state.forEach((t, ti) => {
    if (t.type === "roster") {
      const idx: number[] = [];
      t.members.forEach((m, i) => {
        if (m.checked && m.action !== "unchanged") idx.push(i);
      });
      if (idx.length === 0) return;
      accepted.push({
        type: "roster",
        roster: t.ref.kind === "new" ? { kind: "new", name: t.ref.name.trim() } : t.ref,
        members: idx.map((i) => {
          const m = t.members[i];
          const note = opt(m.note);
          return note === undefined ? { name: m.name.trim() } : { name: m.name.trim(), note };
        }),
      });
      map.push({ target: ti, members: idx, menu: [] });
      return;
    }
    const idx: number[] = [];
    t.menu.forEach((m, i) => {
      if (m.checked && m.action !== "unchanged") idx.push(i);
    });
    const withInfo = infoIncluded(t);
    if (idx.length === 0 && !withInfo) return;
    const isNew = isNewVendor(t);
    accepted.push({
      type: "vendor",
      vendor: isNew ? { kind: "new", name: t.info.name.trim() } : t.ref,
      ...(withInfo
        ? {
            info: {
              ...(isNew ? { name: t.info.name.trim() } : {}),
              phone: opt(t.info.phone),
              address: opt(t.info.address),
              note: opt(t.info.note),
            },
          }
        : {}),
      menu: idx.map((i) => {
        const m = t.menu[i];
        return {
          name: m.name.trim(),
          price: parsePrice(m.price),
          description: opt(m.description),
          category: opt(m.category),
        };
      }),
    });
    map.push({ target: ti, members: [], menu: idx });
  });
  return { accepted, map };
}

// ---------------------------------------------------------------------------
// Server error mapping
// ---------------------------------------------------------------------------

export type Section = "members" | "menu" | "info" | "target";

export function rowKey(target: number, section: Section, row: number): string {
  return `${target}.${section}.${row}`;
}

export interface MappedErrors {
  /** Row key -> messages (with the field when the path named one). */
  rows: Record<string, { field?: string; message: string }[]>;
  /** Errors that could not be tied to a row. */
  unmapped: string[];
}

const PATH_RE = /^accepted\[(\d+)\](?:\.(members|menu)\[(\d+)\]|\.(info))?(?:\.([\w.]+))?$/;

/** Maps `errors[].path` (e.g. `accepted[0].menu[3].price`) back to UI rows. */
export function mapServerErrors(errors: ProblemFieldError[], map: AcceptedMap[]): MappedErrors {
  const out: MappedErrors = { rows: {}, unmapped: [] };
  const push = (key: string, field: string | undefined, message: string) =>
    (out.rows[key] ??= []).push({ field, message });
  for (const e of errors) {
    const m = PATH_RE.exec(e.path);
    const entry = m ? map[Number(m[1])] : undefined;
    if (!m || !entry) {
      out.unmapped.push(e.message);
      continue;
    }
    const [, , section, rowIdx, info, field] = m;
    if (section === "members" || section === "menu") {
      const ui = entry[section][Number(rowIdx)];
      if (ui === undefined) out.unmapped.push(e.message);
      else push(rowKey(entry.target, section, ui), field, e.message);
    } else if (info) {
      push(rowKey(entry.target, "info", 0), field, e.message);
    } else {
      // `.roster` / `.vendor` (optionally `.name`)
      push(rowKey(entry.target, "target", 0), field, e.message);
    }
  }
  return out;
}

/** Id of the roster/vendor page to open after applying. */
export function refLink(ref: TargetRef, kind: "roster" | "vendor"): string | null {
  return ref.kind === "existing" ? `/${kind === "roster" ? "rosters" : "vendors"}/${ref.existingId}` : null;
}
