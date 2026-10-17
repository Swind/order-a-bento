import { EmptyState } from "@/shared/components/EmptyState";
import { useState } from "react";
import { toast } from "sonner";
import { errorMessage } from "@/shared/api/errors";
import type { GroupOrderDetail, ParticipantRow } from "@/shared/api/types";
import { formatMoney } from "@/shared/lib/format";
import { cn } from "@/shared/lib/utils";
import { Badge } from "@/shared/ui/badge";
import { Button } from "@/shared/ui/button";
import { Switch } from "@/shared/ui/switch";
import { useParticipants, useSettlement, useSummary } from "./hooks";
import {
  balanceTag,
  CHECKLIST_FILTERS,
  filterChecklist,
  itemText,
  splitParticipants,
  type ChecklistFilter,
} from "./lib";
import { QueryGate } from "./ParticipantsQuery";

function BalanceBadge({ balance }: { balance: number | null | undefined }) {
  const tag = balanceTag(balance);
  if (!tag) return null;
  return (
    <Badge
      variant={tag.tone === "owe" ? "default" : "secondary"}
      className={
        tag.tone === "owe"
          ? "bg-amber-500 text-black"
          : "bg-sky-100 text-sky-900 dark:bg-sky-900 dark:text-sky-100"
      }
    >
      {tag.text}
    </Badge>
  );
}

function ChecklistRow({
  row,
  disabled,
  onChange,
}: {
  row: ParticipantRow;
  disabled: boolean;
  onChange: (next: { pickedUp: boolean; paid: boolean }) => void;
}) {
  return (
    <li className="space-y-2 px-4 py-3">
      <div className="flex flex-wrap items-center gap-2">
        <span className="text-base font-medium">{row.name}</span>
        {row.submissionCount > 1 && <Badge variant="secondary">改過 {row.submissionCount} 次</Badge>}
        {row.suspiciousDevice && <Badge variant="destructive">⚠ 同裝置多人</Badge>}
        <BalanceBadge balance={row.balance} />
        <span className="ml-auto font-medium">{formatMoney(row.amountDue)}</span>
      </div>
      <p className="break-words text-sm text-muted-foreground">
        {row.items.map(itemText).join("、")}
      </p>
      <div className="flex gap-6">
        <label className="flex min-h-11 items-center gap-2 text-sm">
          <Switch
            aria-label={`${row.name} 領取`}
            checked={row.pickedUp}
            disabled={disabled}
            onCheckedChange={(v) => onChange({ pickedUp: v, paid: row.paid })}
          />
          領取
        </label>
        <label className="flex min-h-11 items-center gap-2 text-sm">
          <Switch
            aria-label={`${row.name} 繳費`}
            checked={row.paid}
            disabled={disabled}
            onCheckedChange={(v) => onChange({ pickedUp: row.pickedUp, paid: v })}
          />
          繳費
        </label>
      </div>
    </li>
  );
}

export function ChecklistTab({ group }: { group: GroupOrderDetail }) {
  const isOpen = group.status === "open";
  const query = useParticipants(group.id, isOpen);
  const summary = useSummary(group.id, isOpen);
  const settle = useSettlement(group.id);
  const [filter, setFilter] = useState<ChecklistFilter>("all");
  const archived = group.status === "archived";

  if (!query.data) return <QueryGate query={query} />;
  const { ordered, notOrdered } = splitParticipants(query.data);
  const shown = filterChecklist(ordered, filter);

  return (
    <div className="space-y-4">
      <div className="flex flex-wrap gap-2" role="group" aria-label="篩選">
        {CHECKLIST_FILTERS.map((f) => (
          <Button
            key={f.value}
            type="button"
            variant={filter === f.value ? "default" : "outline"}
            aria-pressed={filter === f.value}
            className={cn("h-11 rounded-full px-4", filter !== f.value && "bg-background")}
            onClick={() => setFilter(f.value)}
          >
            {f.label}
          </Button>
        ))}
      </div>

      {shown.length === 0 ? (
        <EmptyState
          title={ordered.length === 0 ? "還沒有人下單" : "沒有符合的項目"}
          hint={
            ordered.length === 0
              ? "有人下單後，就能在這裡勾選領取與繳費。"
              : "換個篩選條件看看。"
          }
        />
      ) : (
        <ul className="divide-y rounded-xl border" aria-label="領取與繳費">
          {shown.map((r) => (
            <ChecklistRow
              key={r.id}
              row={r}
              disabled={archived}
              onChange={(next) =>
                settle.mutate(
                  { pid: r.id, ...next },
                  { onError: (e) => toast.error(errorMessage(e)) },
                )
              }
            />
          ))}
        </ul>
      )}

      {summary.data && (
        <div className="space-y-1 rounded-xl bg-muted p-4">
          <p className="text-base font-semibold">
            已收 {formatMoney(summary.data.settlement.paidAmount)} / 應收{" "}
            {formatMoney(summary.data.totalAmount)}
          </p>
          <p className="text-sm text-muted-foreground">
            未繳 {formatMoney(summary.data.settlement.unpaidAmount)}
          </p>
        </div>
      )}

      {notOrdered.length > 0 && (
        <section aria-label="未訂餐" className="space-y-2 rounded-xl bg-muted/50 p-4 text-muted-foreground">
          <h3 className="text-sm font-medium">未訂餐（{notOrdered.length}）</h3>
          <ul className="flex flex-wrap gap-x-4 gap-y-1 text-sm">
            {notOrdered.map((p) => (
              <li key={p.id}>{p.name}</li>
            ))}
          </ul>
        </section>
      )}
    </div>
  );
}
