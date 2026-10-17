import type { ReactNode } from "react";
import type { ChangeAction, Confidence, ImportIssue } from "@/shared/api/types";
import { cn } from "@/shared/lib/utils";
import { Badge } from "@/shared/ui/badge";
import { Checkbox } from "@/shared/ui/checkbox";
import { Input } from "@/shared/ui/input";
import { issueMessage, type InfoRow, type MemberRow, type MenuRow } from "./lib";

type ServerErr = { field?: string; message: string }[] | undefined;

const ACTION_LABEL: Record<ChangeAction, string> = {
  create: "＋新增",
  update: "✎覆蓋",
  unchanged: "＝不變",
};

export function ActionBadge({ action }: { action: ChangeAction }) {
  return (
    <Badge variant={action === "create" ? "default" : action === "update" ? "secondary" : "outline"}>
      {ACTION_LABEL[action]}
    </Badge>
  );
}

function rowTone(issues: ImportIssue[], serverErr: ServerErr, confidence: Confidence, checked: boolean) {
  if (checked && (issues.length > 0 || (serverErr?.length ?? 0) > 0)) return "border-destructive";
  if (confidence === "low") return "border-yellow-500";
  return "border-border";
}

function RowShell({
  checked,
  onChecked,
  label,
  tone,
  issues,
  serverErr,
  children,
}: {
  checked: boolean;
  onChecked: (v: boolean) => void;
  label: string;
  tone: string;
  issues: ImportIssue[];
  serverErr: ServerErr;
  children: ReactNode;
}) {
  return (
    <li
      data-testid="change-row"
      data-state={tone === "border-destructive" ? "error" : tone === "border-yellow-500" ? "warn" : "ok"}
      className={cn("flex gap-1 rounded-lg border-2 p-2", tone, !checked && "opacity-60")}
    >
      <label className="flex min-h-11 min-w-11 shrink-0 cursor-pointer items-start justify-center pt-2.5">
        <Checkbox checked={checked} onCheckedChange={(v) => onChecked(v === true)} aria-label={label} />
      </label>
      <div className="min-w-0 flex-1 space-y-2">
        {children}
        {checked && issues.length > 0 && (
          <ul className="space-y-0.5 text-sm text-destructive">
            {issues.map((i) => (
              <li key={i}>⚠ {issueMessage(i)}</li>
            ))}
          </ul>
        )}
        {serverErr?.map((e, i) => (
          <p key={i} role="alert" className="text-sm text-destructive">
            ⚠ {e.message}
          </p>
        ))}
      </div>
    </li>
  );
}

export function MemberRowView({
  row,
  issues,
  serverErr,
  onChange,
}: {
  row: MemberRow;
  issues: ImportIssue[];
  serverErr: ServerErr;
  onChange: (patch: Partial<MemberRow>) => void;
}) {
  return (
    <RowShell
      checked={row.checked}
      onChecked={(checked) => onChange({ checked })}
      label={`套用 ${row.name || "此列"}`}
      tone={rowTone(issues, serverErr, row.confidence, row.checked)}
      issues={issues}
      serverErr={serverErr}
    >
      <div className="flex items-center gap-2">
        <ActionBadge action={row.action} />
        {row.action === "create" ? (
          <Input
            className="h-11"
            value={row.name}
            aria-label="姓名"
            aria-invalid={issues.includes("name_empty") || issues.includes("name_too_long")}
            onChange={(e) => onChange({ name: e.target.value })}
          />
        ) : (
          <span className="min-w-0 break-words text-base font-medium">{row.name}</span>
        )}
      </div>
      {row.action === "update" && (
        <p className="text-sm text-muted-foreground">
          備註：{row.beforeNote || "（空）"} → {row.note || "（空）"}
        </p>
      )}
      <Input
        className="h-11"
        value={row.note}
        placeholder="備註"
        aria-label={`${row.name || "此列"} 備註`}
        aria-invalid={issues.includes("note_too_long")}
        onChange={(e) => onChange({ note: e.target.value })}
      />
    </RowShell>
  );
}

export function MenuRowView({
  row,
  issues,
  serverErr,
  onChange,
}: {
  row: MenuRow;
  issues: ImportIssue[];
  serverErr: ServerErr;
  onChange: (patch: Partial<MenuRow>) => void;
}) {
  const priceBad = issues.includes("price_missing") || issues.includes("price_out_of_range");
  return (
    <RowShell
      checked={row.checked}
      onChecked={(checked) => onChange({ checked })}
      label={`套用 ${row.name || "此列"}`}
      tone={rowTone(issues, serverErr, row.confidence, row.checked)}
      issues={issues}
      serverErr={serverErr}
    >
      <div className="flex items-center gap-2">
        <ActionBadge action={row.action} />
        {row.action === "create" ? (
          <Input
            className="h-11"
            value={row.name}
            aria-label="品名"
            aria-invalid={issues.includes("name_empty") || issues.includes("name_too_long")}
            onChange={(e) => onChange({ name: e.target.value })}
          />
        ) : (
          <span className="min-w-0 break-words text-base font-medium">{row.name}</span>
        )}
      </div>
      <div className="flex items-center gap-2">
        {row.before && (
          <span className="shrink-0 text-sm text-muted-foreground">${row.before.price} →</span>
        )}
        <div className="relative w-32">
          <span className="pointer-events-none absolute top-1/2 left-2.5 -translate-y-1/2 text-muted-foreground">
            $
          </span>
          <Input
            className={cn("h-11 pl-6", priceBad && row.checked && "border-destructive")}
            inputMode="numeric"
            value={row.price}
            aria-label={`${row.name || "此列"} 價格`}
            aria-invalid={priceBad && row.checked}
            onChange={(e) => onChange({ price: e.target.value })}
          />
        </div>
      </div>
      <div className="grid grid-cols-2 gap-2">
        <Input
          className="h-11"
          value={row.description}
          placeholder="說明"
          aria-label={`${row.name || "此列"} 說明`}
          onChange={(e) => onChange({ description: e.target.value })}
        />
        <Input
          className="h-11"
          value={row.category}
          placeholder="分類"
          aria-label={`${row.name || "此列"} 分類`}
          onChange={(e) => onChange({ category: e.target.value })}
        />
      </div>
    </RowShell>
  );
}

/** Dimmed, read-only rows folded under 「n 筆不變」. */
export function UnchangedList({ names }: { names: string[] }) {
  if (names.length === 0) return null;
  return (
    <details className="rounded-lg border border-dashed p-2 text-sm text-muted-foreground">
      <summary className="flex min-h-11 cursor-pointer items-center">{names.length} 筆不變</summary>
      <ul className="space-y-1 pb-1">
        {names.map((n, i) => (
          <li key={i} className="flex items-center gap-2 opacity-70">
            <ActionBadge action="unchanged" />
            <span className="break-words">{n}</span>
          </li>
        ))}
      </ul>
    </details>
  );
}

function ChangeText({ before, after }: { before: string | null | undefined; after: string }) {
  if (before === undefined || before === null || before === after) return null;
  return (
    <p className="text-sm text-muted-foreground">
      原本：{before || "（空）"} → {after || "（空）"}
    </p>
  );
}

export function InfoSection({
  info,
  isNew,
  errors,
  serverErr,
  onChange,
}: {
  info: InfoRow;
  isNew: boolean;
  errors: Partial<Record<"name" | "phone" | "address" | "note", string>>;
  serverErr: ServerErr;
  onChange: (patch: Partial<InfoRow>) => void;
}) {
  if (!isNew && info.action === "unchanged") {
    return <p className="text-sm text-muted-foreground opacity-70">店家資訊不變</p>;
  }
  const has = Object.keys(errors).length > 0 || (serverErr?.length ?? 0) > 0;
  const fields = [
    ["phone", "電話", info.before?.phone],
    ["address", "地址", info.before?.address],
    ["note", "備註", info.before?.note],
  ] as const;
  return (
    <div
      data-testid="info-section"
      className={cn(
        "flex gap-1 rounded-lg border-2 p-2",
        info.checked && has ? "border-destructive" : "border-border",
        !info.checked && "opacity-60",
      )}
    >
      <label className="flex min-h-11 min-w-11 shrink-0 cursor-pointer items-start justify-center pt-2.5">
        <Checkbox
          checked={info.checked}
          disabled={isNew}
          onCheckedChange={(v) => onChange({ checked: v === true })}
          aria-label="套用店家資訊"
        />
      </label>
      <div className="min-w-0 flex-1 space-y-2">
        <p className="text-sm font-medium">店家資訊（{isNew ? "新增" : "覆蓋"}）</p>
        {isNew && (
          <div>
            <Input
              className="h-11"
              value={info.name}
              aria-label="新店家名稱"
              aria-invalid={!!errors.name}
              onChange={(e) => onChange({ name: e.target.value })}
            />
            {errors.name && <p className="text-sm text-destructive">⚠ {errors.name}</p>}
          </div>
        )}
        {fields.map(([k, label, before]) => (
          <div key={k}>
            <ChangeText before={before} after={info[k]} />
            <Input
              className="h-11"
              value={info[k]}
              placeholder={label}
              aria-label={`店家${label}`}
              aria-invalid={!!errors[k]}
              onChange={(e) => onChange({ [k]: e.target.value })}
            />
            {errors[k] && <p className="text-sm text-destructive">⚠ {errors[k]}</p>}
          </div>
        ))}
        {serverErr?.map((e, i) => (
          <p key={i} role="alert" className="text-sm text-destructive">
            ⚠ {e.message}
          </p>
        ))}
      </div>
    </div>
  );
}
