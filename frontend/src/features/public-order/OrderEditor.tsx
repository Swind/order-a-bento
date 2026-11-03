import { CircleCheck, Minus, Plus } from "lucide-react";
import { useQueryClient } from "@tanstack/react-query";
import { useEffect, useRef, useState } from "react";
import { toast } from "sonner";
import { ApiError } from "@/shared/api/client";
import { errorMessage } from "@/shared/api/errors";
import type { EffectiveOrder, PublicGroup } from "@/shared/api/types";
import { formatHHmm } from "@/shared/lib/datetime";
import { collectClientInfo } from "@/shared/lib/deviceInfo";
import { formatMoney } from "@/shared/lib/format";
import { orderTotal } from "@/shared/lib/money";
import { cn } from "@/shared/lib/utils";
import { EmptyState } from "@/shared/components/EmptyState";
import { Button } from "@/shared/ui/button";
import { Dialog, DialogContent, DialogDescription, DialogTitle } from "@/shared/ui/dialog";
import { Input } from "@/shared/ui/input";
import { Textarea } from "@/shared/ui/textarea";
import { publicGroupKey, useSubmitOrder } from "./hooks";
import { restoreOrderFocus } from "./focus";
import {
  ITEM_NOTE_MAX,
  MAX_QTY,
  ORDER_NOTE_MAX,
  orderContent,
  resolveIdempotencyKey,
  type PendingKey,
} from "./lib";

interface Props {
  token: string;
  group: PublicGroup;
  participantId: string;
  order: EffectiveOrder | null;
  readOnly: boolean;
  active?: boolean;
  onClosed: (reason: "group_closed" | "deadline_passed") => void;
}

export function OrderEditor({ token, group, participantId, order, readOnly, active = true, onClosed }: Props) {
  const effective = order && !order.cancelled ? order : null;
  const [editing, setEditing] = useState(order === null);
  const [sendError, setSendError] = useState<string | null>(null);
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
  const [busy, setBusy] = useState(false);
  const sending = useRef(false);
  const cancelTriggerRef = useRef<HTMLButtonElement>(null);
  const keepRef = useRef<HTMLButtonElement>(null);
  const headingRef = useRef<HTMLHeadingElement>(null);
  const completedRef = useRef(false);
  const pendingKey = useRef<PendingKey | null>(null);
  const submit = useSubmitOrder(token, participantId);
  const qc = useQueryClient();
  const activeRef = useRef(active);
  useEffect(() => { activeRef.current = active; }, [active]);
  const participantName = group.participants.find((p) => p.id === participantId)?.name;

  const lines = group.menu
    .filter((i) => (qty[i.id] ?? 0) > 0)
    .map((i) => ({
      itemId: i.id,
      quantity: qty[i.id],
      ...(notes[i.id]?.trim() ? { note: notes[i.id].trim() } : {}),
    }));
  const count = lines.reduce((n, l) => n + l.quantity, 0);
  const total = orderTotal(group.menu.map((i) => ({ price: i.price, quantity: qty[i.id] ?? 0 })));
  const soldOutSelected = group.menu.some((i) => i.soldOut && (qty[i.id] ?? 0) > 0);
  const dirty = orderContent(lines, orderNote) !== orderContent(effective?.lines ?? [], effective?.note);

  // Publish the sticky bar's real height (incl. safe-area inset) as --order-bar-h so the page can
  // reserve exactly that much bottom padding and the footer scrolls clear of the bar.
  const barRef = useRef<HTMLDivElement>(null);
  const barShown = active && !readOnly && editing;
  const readOnlyRef = useRef(readOnly);
  useEffect(() => {
    readOnlyRef.current = readOnly;
    return () => { readOnlyRef.current = true; };
  }, [readOnly]);
  const [wasActive, setWasActive] = useState(active);
  if (wasActive !== active) {
    setWasActive(active);
    if (!active) setConfirmCancel(false);
  }
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

  function resetDraft(saved: EffectiveOrder | null) {
    const source = saved && !saved.cancelled ? saved : null;
    setQty(Object.fromEntries((source?.lines ?? []).map((l) => [l.itemId, l.quantity])));
    setNotes(Object.fromEntries((source?.lines ?? []).map((l) => [l.itemId, l.note ?? ""])));
    setNoteOpen(Object.fromEntries((source?.lines ?? []).filter((l) => l.note).map((l) => [l.itemId, true])));
    setOrderNote(source?.note ?? "");
  }

  function focusHeading() {
    queueMicrotask(() => {
      if (activeRef.current && !readOnlyRef.current) restoreOrderFocus(headingRef.current);
    });
  }

  function startEditing() {
    if (sending.current) return;
    resetDraft(order);
    setSendError(null);
    setEditing(true);
    focusHeading();
  }

  async function send(payloadLines: typeof lines, note: string, cancelling: boolean) {
    if (sending.current || readOnly || !active) return;
    sending.current = true;
    setBusy(true);
    setSendError(null);
    try {
      const trimmedNote = note.trim();
      const fingerprint = JSON.stringify([participantId, payloadLines, trimmedNote]);
      pendingKey.current = resolveIdempotencyKey(pendingKey.current, fingerprint);
      const body = {
        lines: payloadLines,
        ...(trimmedNote ? { note: trimmedNote } : {}),
        idempotencyKey: pendingKey.current.key,
        client: await collectClientInfo(),
      };
      if (readOnlyRef.current) return;
      const data = await submit.mutateAsync(body);
      if (readOnlyRef.current) return;
      if (!data.order) throw new Error("未取得已儲存的訂單，請重試");
      resetDraft(data.order);
      pendingKey.current = null;
      setEditing(false);
      completedRef.current = true;
      setConfirmCancel(false);
      if (!cancelling) focusHeading();
      toast.success(data.order.cancelled ? "已取消你的訂單" : effective ? "已更新訂單" : "訂單已送出");
    } catch (err) {
      if (err instanceof ApiError && err.status === 409) {
        if (err.code === "group_closed" || err.code === "deadline_passed") {
          onClosed(err.code);
          return;
        }
        if (err.code === "sold_out") qc.invalidateQueries({ queryKey: publicGroupKey(token) });
      }
      if (!readOnlyRef.current) {
        setSendError(errorMessage(err));
      }
    } finally {
      sending.current = false;
      setBusy(false);
    }
  }

  if (readOnly) return <SavedOrder order={order} participantName={participantName} />;

  return (
    <div className="space-y-4">
      {sendError && !confirmCancel && <p role="alert" className="text-destructive">{sendError}</p>}
      {!editing ? <>
        <h3 ref={headingRef} tabIndex={-1} role="status" className={cn(
          "flex items-center gap-3 text-lg font-semibold",
          !busy && effective && "rounded-xl border border-green-200 bg-green-50 p-4 text-xl font-bold text-green-900 dark:border-green-800 dark:bg-green-950/40 dark:text-green-200",
        )}>
          {!busy && effective && <CircleCheck aria-hidden="true" className="size-6 shrink-0" />}
          {busy ? "處理訂單中…" : order?.cancelled ? "你的訂單已取消" : order ? "訂單已送出" : "尚未取得已儲存的訂單，請重新查詢"}
        </h3>
        {!busy && <SavedOrder order={order} participantName={participantName} />}
        {order && !busy && <Button className="h-12 w-full text-base" onClick={startEditing}>{order.cancelled ? "重新訂餐" : "修改訂單"}</Button>}
      </> : <>
      <h3 ref={headingRef} tabIndex={-1} className="text-lg font-semibold">{effective ? "修改訂單" : "選擇餐點"}</h3>
      <p role="status" className="text-sm font-medium">
        {busy ? "處理訂單中…" : !effective && count === 0 ? "請先選擇餐點" : dirty ? (effective ? "有尚未送出的修改" : "尚未送出") : effective ? "訂單已送出" : "尚未送出"}
      </p>
      {order && <Button variant="outline" className="h-11" disabled={busy} onClick={() => {
        if (sending.current) return;
        resetDraft(order);
        setSendError(null);
        setEditing(false);
        focusHeading();
      }}>放棄修改</Button>}
      </>}
      {effective && (
        <section aria-label="上次送出的訂單" className={editing ? "space-y-1 rounded-xl border border-primary/20 bg-accent p-4" : "text-center"}>
          {editing && <>
            <p className="text-sm font-semibold text-accent-foreground">上次送出的訂單 · 已於 {formatHHmm(effective.submittedAt)} 送出</p>
            <p className="text-sm text-accent-foreground">{effective.lines.map((line) => `${line.name} ×${line.quantity}`).join("、")}</p>
            <p className="text-sm text-muted-foreground">截止前都可以修改，修改後請再次送出。</p>
          </>}
          <Button
            ref={cancelTriggerRef}
            type="button"
            variant="ghost"
            className={cn("h-11", editing ? "text-destructive" : "text-muted-foreground hover:text-destructive")}
            disabled={busy}
            onClick={() => { completedRef.current = false; setConfirmCancel(true); }}
          >
            取消我的訂單
          </Button>
        </section>
      )}

      {active && confirmCancel && <Dialog
        open
        onOpenChange={(open) => { if (!sending.current) setConfirmCancel(open); }}
      >
        <DialogContent
          showCloseButton={false}
          initialFocus={keepRef}
          finalFocus={() => {
            const target = completedRef.current ? headingRef.current : cancelTriggerRef.current;
            // Base UI can suppress automatic return after an outside press; both close paths need a target.
            return restoreOrderFocus(target);
          }}
        >
          <DialogTitle>確認取消訂單</DialogTitle>
          <DialogDescription>確定要取消你的訂單嗎？尚未送出的修改也會在取消成功後清除。</DialogDescription>
          {sendError && <p role="alert" className="text-destructive">{sendError}</p>}
          <div className="flex gap-2">
            <Button ref={keepRef} type="button" variant="outline" className="h-11 flex-1" disabled={busy} onClick={() => setConfirmCancel(false)}>
              保留
            </Button>
            <Button type="button" variant="destructive" className="h-11 flex-1" disabled={busy} aria-busy={busy} onClick={() => send([], "", true)}>
              {busy ? "取消中…" : "確定取消"}
            </Button>
          </div>
        </DialogContent>
      </Dialog>}

      {editing && <fieldset disabled={busy} className="min-w-0 space-y-4" aria-busy={busy}>
      {group.menu.length === 0 && (
        <EmptyState title="這個團還沒有菜單品項" hint="請向開團的人確認，菜單補上後重新整理頁面即可點餐。" />
      )}
      <ul className={cn("overflow-hidden divide-y rounded-xl border bg-card", group.menu.length === 0 && "hidden")}>
        {group.menu.map((item) => {
          const q = qty[item.id] ?? 0;
          const disabledItem = readOnly || item.soldOut;
          const conflict = item.soldOut && q > 0;
          return (
            <li
              key={item.id}
              data-testid={`menu-${item.id}`}
              className={cn("space-y-2 px-4 py-4 transition-colors", q > 0 && "bg-accent/60", conflict && "bg-destructive/10")}
            >
              <div className="flex items-start justify-between gap-3">
                <div className="min-w-0">
                  <p className={cn("text-base font-semibold", item.soldOut && "text-muted-foreground")}>
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
                <span className={cn("shrink-0 text-base font-medium tabular-nums", item.soldOut && "text-muted-foreground")}>{formatMoney(item.price)}</span>
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
                  <span className={cn("w-8 text-center text-lg font-semibold tabular-nums", q > 0 && "text-primary")} aria-live="polite">
                    {q}
                  </span>
                  <Button
                    type="button"
                    variant={q > 0 ? "default" : "outline"}
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
      </fieldset>}

      {barShown && (
        <div
          ref={barRef}
          role="region"
          aria-label="訂單送出操作"
          className="fixed inset-x-0 bottom-0 z-10 border-t bg-card shadow-[0_-4px_20px_rgba(0,0,0,0.04)]"
        >
          <div className="mx-auto w-full max-w-md space-y-1 px-4 pt-3 pb-[max(0.75rem,env(safe-area-inset-bottom))]">
            <div role="group" className="flex items-center justify-between pb-2 text-sm text-muted-foreground" aria-label="目前選餐合計">
              <span>共 {count} 個</span><span className="text-xl font-bold tabular-nums text-foreground">{formatMoney(total)}</span>
            </div>
            <Button
              type="button"
              className="h-12 w-full text-base"
              disabled={busy || count === 0 || soldOutSelected || (!!effective && !dirty)}
              aria-busy={busy}
              onClick={() => send(lines, orderNote, false)}
            >
              {busy ? "送出中…" : effective ? dirty ? "儲存修改" : "訂單已送出" : "送出訂單"}
            </Button>
          </div>
        </div>
      )}
    </div>
  );
}

function SavedOrder({ order, participantName }: { order: EffectiveOrder | null; participantName?: string }) {
  if (!order) return <p className="rounded-xl border bg-card p-4">你沒有訂餐</p>;
  if (order.cancelled) return <div className="rounded-xl border bg-card p-4"><p className="break-words">{participantName}</p><p>你的訂單已取消 · <time dateTime={order.submittedAt}>{formatHHmm(order.submittedAt)}</time></p></div>;
  return (
    <section aria-label="已送出訂單" className="space-y-3 rounded-xl border bg-card p-4">
      <h3 className="font-semibold">上次送出的訂單</h3>
      <p className="break-words">{participantName}</p>
      <p className="text-sm text-muted-foreground">已登記至本團，並非店家已接單</p>
      <p className="text-sm text-muted-foreground"><time dateTime={order.submittedAt}>已於 {formatHHmm(order.submittedAt)} 送出</time></p>
      <ul className="divide-y">
        {order.lines.map((line) => (
          <li key={line.itemId} className="space-y-1 py-3">
            <p className="break-words font-medium">{line.name} ×{line.quantity}</p>
            <p className="flex flex-wrap justify-between gap-2 text-sm tabular-nums"><span>{formatMoney(line.unitPrice)} × {line.quantity}</span><span>小計 {formatMoney(line.subtotal)}</span></p>
            {line.note && <p className="whitespace-pre-wrap break-words text-sm">備註：{line.note}</p>}
          </li>
        ))}
      </ul>
      {order.note && <p className="whitespace-pre-wrap break-words">訂單備註：{order.note}</p>}
      <p className="text-lg font-bold tabular-nums">總額 {formatMoney(order.total)}</p>
    </section>
  );
}
