import { Minus, Plus } from "lucide-react";
import { useQueryClient } from "@tanstack/react-query";
import { useEffect, useRef, useState } from "react";
import { toast } from "sonner";
import { ApiError } from "@/shared/api/client";
import { errorMessage } from "@/shared/api/errors";
import type { EffectiveOrder, PublicGroup } from "@/shared/api/types";
import { formatHHmm } from "@/shared/lib/datetime";
import { collectClientInfo } from "@/shared/lib/deviceInfo";
import { formatMoney } from "@/shared/lib/format";
import { cn } from "@/shared/lib/utils";
import { EmptyState } from "@/shared/components/EmptyState";
import { Button } from "@/shared/ui/button";
import { Input } from "@/shared/ui/input";
import { Textarea } from "@/shared/ui/textarea";
import { publicGroupKey, useSubmitOrder } from "./hooks";
import {
  ITEM_NOTE_MAX,
  MAX_QTY,
  ORDER_NOTE_MAX,
  resolveIdempotencyKey,
  type PendingKey,
} from "./lib";

interface Props {
  token: string;
  group: PublicGroup;
  participantId: string;
  order: EffectiveOrder | null;
  readOnly: boolean;
  onClosed: (reason: "group_closed" | "deadline_passed") => void;
}

export function OrderEditor({ token, group, participantId, order, readOnly, onClosed }: Props) {
  const effective = order && !order.cancelled ? order : null;
  const [qty, setQty] = useState<Record<string, number>>(() =>
    Object.fromEntries((effective?.lines ?? []).map((l) => [l.itemId, l.quantity])),
  );
  const [notes, setNotes] = useState<Record<string, string>>(() =>
    Object.fromEntries((effective?.lines ?? []).filter((l) => l.note).map((l) => [l.itemId, l.note!])),
  );
  const [noteOpen, setNoteOpen] = useState<Record<string, boolean>>(() =>
    Object.fromEntries((effective?.lines ?? []).filter((l) => l.note).map((l) => [l.itemId, true])),
  );
  const [orderNote, setOrderNote] = useState(effective?.note ?? "");
  const [confirmCancel, setConfirmCancel] = useState(false);
  const pendingKey = useRef<PendingKey | null>(null);
  const submit = useSubmitOrder(token, participantId);
  const qc = useQueryClient();

  const lines = group.menu
    .filter((i) => (qty[i.id] ?? 0) > 0)
    .map((i) => ({
      itemId: i.id,
      quantity: qty[i.id],
      ...(notes[i.id]?.trim() ? { note: notes[i.id].trim() } : {}),
    }));
  const count = lines.reduce((n, l) => n + l.quantity, 0);
  const total = group.menu.reduce((s, i) => s + i.price * (qty[i.id] ?? 0), 0);
  const soldOutSelected = group.menu.some((i) => i.soldOut && (qty[i.id] ?? 0) > 0);

  // Publish the sticky bar's real height (incl. safe-area inset) as --order-bar-h so the page can
  // reserve exactly that much bottom padding and the footer scrolls clear of the bar.
  const barRef = useRef<HTMLDivElement>(null);
  const barShown = !readOnly;
  useEffect(() => {
    const root = document.documentElement;
    const el = barRef.current;
    if (!barShown || !el) return;
    const update = () => root.style.setProperty("--order-bar-h", `${el.offsetHeight}px`);
    update();
    const ro = typeof ResizeObserver === "undefined" ? null : new ResizeObserver(update);
    ro?.observe(el);
    return () => {
      ro?.disconnect();
      root.style.removeProperty("--order-bar-h");
    };
  }, [barShown]);

  function setQuantity(id: string, next: number) {
    setQty((q) => ({ ...q, [id]: Math.min(MAX_QTY, Math.max(0, next)) }));
  }

  async function send(payloadLines: typeof lines, note: string, cancelling: boolean) {
    const trimmedNote = note.trim();
    const fingerprint = JSON.stringify([participantId, payloadLines, trimmedNote]);
    pendingKey.current = resolveIdempotencyKey(pendingKey.current, fingerprint);
    const body = {
      lines: payloadLines,
      ...(trimmedNote ? { note: trimmedNote } : {}),
      idempotencyKey: pendingKey.current.key,
      client: await collectClientInfo(),
    };
    submit.mutate(body, {
      onSuccess: () => {
        pendingKey.current = null;
        setConfirmCancel(false);
        if (cancelling) {
          setQty({});
          setNotes({});
          setNoteOpen({});
          setOrderNote("");
          toast.success("已取消你的訂單");
        } else toast.success(effective ? "已更新訂單" : "訂單已送出");
      },
      onError: (err) => {
        if (err instanceof ApiError && err.status === 409) {
          if (err.code === "group_closed" || err.code === "deadline_passed") {
            onClosed(err.code);
            return;
          }
          if (err.code === "sold_out") qc.invalidateQueries({ queryKey: publicGroupKey(token) });
        }
        toast.error(errorMessage(err));
      },
    });
  }

  return (
    <div className="space-y-4">
      {effective ? (
        <p className="text-sm font-medium text-green-700 dark:text-green-400">
          ✓ 已於 {formatHHmm(effective.submittedAt)} 送出
        </p>
      ) : (
        order?.cancelled && (
          <p className="text-sm text-muted-foreground">
            你已於 {formatHHmm(order.submittedAt)} 取消訂單
          </p>
        )
      )}

      {group.menu.length === 0 && (
        <EmptyState title="這個團還沒有菜單品項" hint="請向開團的人確認，菜單補上後重新整理頁面即可點餐。" />
      )}
      <ul className={cn("divide-y rounded-xl border", group.menu.length === 0 && "hidden")}>
        {group.menu.map((item) => {
          const q = qty[item.id] ?? 0;
          const disabledItem = readOnly || item.soldOut;
          const conflict = item.soldOut && q > 0;
          return (
            <li
              key={item.id}
              data-testid={`menu-${item.id}`}
              className={cn("space-y-2 px-4 py-3", conflict && "bg-destructive/10")}
            >
              <div className="flex items-start justify-between gap-3">
                <div className="min-w-0">
                  <p className="text-base font-medium">
                    {item.name}
                    {item.soldOut && (
                      <span className="ml-2 rounded bg-muted px-1.5 py-0.5 text-xs">售完</span>
                    )}
                  </p>
                  {(item.category || item.description) && (
                    <p className="text-sm text-muted-foreground">
                      {[item.category, item.description].filter(Boolean).join(" · ")}
                    </p>
                  )}
                  {conflict && (
                    <p role="alert" className="text-sm text-destructive">
                      此品項已售完，請改選其他品項
                    </p>
                  )}
                </div>
                <span className="shrink-0 text-base">{formatMoney(item.price)}</span>
              </div>
              <div className="flex items-center justify-between gap-3">
                {q > 0 && !readOnly ? (
                  <Button
                    type="button"
                    variant="ghost"
                    className="h-11 px-2 text-sm"
                    onClick={() => setNoteOpen((o) => ({ ...o, [item.id]: !o[item.id] }))}
                  >
                    {noteOpen[item.id] ? "收起備註" : "＋備註"}
                  </Button>
                ) : (
                  <span />
                )}
                <div className="flex items-center gap-2" role="group" aria-label={`${item.name} 數量`}>
                  <Button
                    type="button"
                    variant="outline"
                    className="size-11"
                    aria-label={`減少 ${item.name}`}
                    disabled={disabledItem ? !(conflict && !readOnly) : q <= 0}
                    onClick={() => setQuantity(item.id, q - 1)}
                  >
                    <Minus />
                  </Button>
                  <span className="w-8 text-center text-lg tabular-nums" aria-live="polite">
                    {q}
                  </span>
                  <Button
                    type="button"
                    variant="outline"
                    className="size-11"
                    aria-label={`增加 ${item.name}`}
                    disabled={disabledItem || q >= MAX_QTY}
                    onClick={() => setQuantity(item.id, q + 1)}
                  >
                    <Plus />
                  </Button>
                </div>
              </div>
              {q > 0 && noteOpen[item.id] && !readOnly && (
                <Input
                  className="h-11"
                  aria-label={`${item.name} 備註`}
                  placeholder="例如：不要辣"
                  maxLength={ITEM_NOTE_MAX}
                  value={notes[item.id] ?? ""}
                  onChange={(e) => setNotes((n) => ({ ...n, [item.id]: e.target.value }))}
                />
              )}
              {q > 0 && readOnly && notes[item.id] && (
                <p className="text-sm text-muted-foreground">備註：{notes[item.id]}</p>
              )}
            </li>
          );
        })}
      </ul>

      <div className="space-y-1.5">
        <label htmlFor="order-note" className="text-sm font-medium">
          訂單備註（選填）
        </label>
        <Textarea
          id="order-note"
          rows={2}
          maxLength={ORDER_NOTE_MAX}
          disabled={readOnly}
          value={orderNote}
          onChange={(e) => setOrderNote(e.target.value)}
        />
      </div>

      {!readOnly && (
        <div
          ref={barRef}
          className="fixed inset-x-0 bottom-0 z-10 border-t bg-background shadow-[0_-2px_8px_rgba(0,0,0,0.08)]"
        >
          <div className="mx-auto w-full max-w-md space-y-1 px-4 pt-3 pb-[max(0.75rem,env(safe-area-inset-bottom))]">
            <p className="text-center text-base font-medium">
              共 {count} 個 {formatMoney(total)}
            </p>
            <Button
              type="button"
              className="h-12 w-full text-base"
              disabled={submit.isPending || count === 0 || soldOutSelected}
              onClick={() => send(lines, orderNote, false)}
            >
              {effective ? "更新訂單（取代上一次）" : "送出訂單"}
            </Button>
            {effective && !confirmCancel && (
              <Button
                type="button"
                variant="ghost"
                className="h-11 w-full text-destructive"
                disabled={submit.isPending}
                onClick={() => setConfirmCancel(true)}
              >
                取消我的訂單
              </Button>
            )}
            {effective && confirmCancel && (
              <div className="flex items-center gap-2" role="alertdialog" aria-label="確認取消訂單">
                <span className="flex-1 text-sm">確定要取消你的訂單嗎？</span>
                <Button
                  type="button"
                  variant="destructive"
                  className="h-11"
                  disabled={submit.isPending}
                  onClick={() => send([], "", true)}
                >
                  確定取消
                </Button>
                <Button
                  type="button"
                  variant="outline"
                  className="h-11"
                  onClick={() => setConfirmCancel(false)}
                >
                  保留
                </Button>
              </div>
            )}
          </div>
        </div>
      )}
    </div>
  );
}
