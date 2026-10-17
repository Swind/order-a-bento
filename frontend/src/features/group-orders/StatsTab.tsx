import { useRef, useState } from "react";
import { toast } from "sonner";
import { errorMessage } from "@/shared/api/errors";
import type { GroupOrderDetail } from "@/shared/api/types";
import { formatMoney } from "@/shared/lib/format";
import { EmptyState } from "@/shared/components/EmptyState";
import { Button } from "@/shared/ui/button";
import { Skeleton } from "@/shared/ui/skeleton";
import { Switch } from "@/shared/ui/switch";
import { Textarea } from "@/shared/ui/textarea";
import { useSetSoldOut, useSummary } from "./hooks";
import { buildOrderText } from "./lib";

export function StatsTab({ group }: { group: GroupOrderDetail }) {
  const query = useSummary(group.id, group.status === "open");
  const soldOut = useSetSoldOut(group.id);
  const [fallbackText, setFallbackText] = useState<string | null>(null);
  const areaRef = useRef<HTMLTextAreaElement>(null);
  const archived = group.status === "archived";

  if (query.isPending) return <Skeleton className="h-40 w-full" />;
  if (query.isError)
    return (
      <div className="space-y-2">
        <p role="alert" className="text-destructive">
          {errorMessage(query.error)}
        </p>
        <Button variant="outline" className="h-11" onClick={() => query.refetch()}>
          重試
        </Button>
      </div>
    );
  const s = query.data;
  const soldOutById = new Map(group.offeredItems.map((i) => [i.id, i.soldOut]));

  async function copy() {
    const text = buildOrderText({
      title: group.title,
      vendorName: group.vendor.name,
      items: s.items,
      totalQuantity: s.totalQuantity,
      totalAmount: s.totalAmount,
    });
    try {
      if (!navigator.clipboard?.writeText) throw new Error("clipboard unavailable");
      await navigator.clipboard.writeText(text);
      setFallbackText(null);
      toast.success("已複製訂單文字");
    } catch {
      setFallbackText(text);
      setTimeout(() => areaRef.current?.select(), 0);
      toast.message("無法自動複製，已選取文字，請手動複製");
    }
  }

  if (s.items.length === 0)
    return (
      <EmptyState title="還沒有人下單" hint="把訂購頁連結分享給大家，下單後這裡會自動統計品項與金額。" />
    );

  return (
    <div className="space-y-4">
      <ul className="divide-y rounded-xl border" aria-label="品項統計">
        {s.items.map((i) => {
          const isSoldOut = soldOutById.get(i.itemId) ?? false;
          return (
            <li key={i.itemId} className="flex items-center gap-3 px-4 py-2">
              <span className="min-w-0 flex-1">
                <span className="block break-words text-base">{i.name}</span>
                <span className="text-sm text-muted-foreground">{formatMoney(i.price)}</span>
              </span>
              <span className="w-10 shrink-0 text-right tabular-nums">×{i.quantity}</span>
              <span className="w-16 shrink-0 text-right tabular-nums">
                {formatMoney(i.subtotal)}
              </span>
              <label className="flex min-h-11 shrink-0 items-center gap-1.5 text-xs">
                <Switch
                  size="sm"
                  aria-label={`${i.name} 售完`}
                  checked={isSoldOut}
                  disabled={archived || soldOut.isPending}
                  onCheckedChange={(checked) =>
                    soldOut.mutate(
                      { itemId: i.itemId, soldOut: checked },
                      { onError: (e) => toast.error(errorMessage(e)) },
                    )
                  }
                />
                售完
              </label>
            </li>
          );
        })}
      </ul>

      <div className="space-y-1 rounded-xl bg-muted p-4">
        <p className="text-base font-semibold">
          共 {s.totalQuantity} 個 · {formatMoney(s.totalAmount)}
        </p>
        <p className="text-sm text-muted-foreground">
          已訂 {s.participants.ordered} / 未訂 {s.participants.notOrdered}
        </p>
      </div>

      <Button type="button" className="h-11" onClick={copy}>
        複製訂單文字
      </Button>
      {fallbackText && (
        <Textarea
          ref={areaRef}
          readOnly
          rows={Math.min(10, fallbackText.split("\n").length)}
          aria-label="訂單文字"
          value={fallbackText}
          onFocus={(e) => e.currentTarget.select()}
        />
      )}
    </div>
  );
}
