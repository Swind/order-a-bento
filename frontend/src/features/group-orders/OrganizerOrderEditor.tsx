import { Minus, Plus } from "lucide-react";
import { useRef, useState } from "react";
import { toast } from "sonner";
import { errorMessage } from "@/shared/api/errors";
import type { GroupOrderDetail } from "@/shared/api/types";
import { formatMoney } from "@/shared/lib/format";
import { cn } from "@/shared/lib/utils";
import { Button } from "@/shared/ui/button";
import { Input } from "@/shared/ui/input";
import { Textarea } from "@/shared/ui/textarea";
import {
  ITEM_NOTE_MAX,
  MAX_QTY,
  ORDER_NOTE_MAX,
  resolveIdempotencyKey,
  type PendingKey,
} from "@/features/public-order/lib";
import { useOrganizerSubmit } from "./hooks";

export interface EditorInitial {
  lines: { itemId: string; name: string; quantity: number; note?: string | null }[];
  note?: string | null;
}

interface Props {
  group: GroupOrderDetail;
  participantId: string;
  participantName: string;
  initial: EditorInitial | null;
  onDone: () => void;
  onCancel: () => void;
}

/** Stepper editor for correcting an order on behalf of a participant (organizer). */
export function OrganizerOrderEditor({
  group,
  participantId,
  participantName,
  initial,
  onDone,
  onCancel,
}: Props) {
  const menu = group.offeredItems;
  const menuIds = new Set(menu.map((i) => i.id));
  const initLines = (initial?.lines ?? [])
    .filter((l) => menuIds.has(l.itemId))
    .map((l) => ({ id: l.itemId, quantity: l.quantity, note: l.note ?? "" }));
  const [qty, setQty] = useState<Record<string, number>>(() =>
    Object.fromEntries(initLines.map((l) => [l.id, l.quantity])),
  );
  const [notes, setNotes] = useState<Record<string, string>>(() =>
    Object.fromEntries(initLines.filter((l) => l.note).map((l) => [l.id, l.note])),
  );
  const [noteOpen, setNoteOpen] = useState<Record<string, boolean>>(() =>
    Object.fromEntries(initLines.filter((l) => l.note).map((l) => [l.id, true])),
  );
  const [orderNote, setOrderNote] = useState(initial?.note ?? "");
  const [confirmCancel, setConfirmCancel] = useState(false);
  const pendingKey = useRef<PendingKey | null>(null);
  const submit = useOrganizerSubmit(group.id, participantId);

  const lines = menu
    .filter((i) => (qty[i.id] ?? 0) > 0)
    .map((i) => ({
      itemId: i.id,
      quantity: qty[i.id],
      ...(notes[i.id]?.trim() ? { note: notes[i.id].trim() } : {}),
    }));
  const count = lines.reduce((n, l) => n + l.quantity, 0);
  const total = menu.reduce((s, i) => s + i.price * (qty[i.id] ?? 0), 0);
  const soldOutSelected = menu.some((i) => i.soldOut && (qty[i.id] ?? 0) > 0);

  function setQuantity(id: string, next: number) {
    setQty((q) => ({ ...q, [id]: Math.min(MAX_QTY, Math.max(0, next)) }));
  }

  function send(payloadLines: typeof lines, note: string, cancelling: boolean) {
    if (!group.canManage || group.status === "archived") return;
    const trimmed = note.trim();
    const fingerprint = JSON.stringify([participantId, payloadLines, trimmed]);
    pendingKey.current = resolveIdempotencyKey(pendingKey.current, fingerprint);
    submit.mutate(
      {
        lines: payloadLines,
        ...(trimmed ? { note: trimmed } : {}),
        idempotencyKey: pendingKey.current.key,
      },
      {
        onSuccess: () => {
          pendingKey.current = null;
          toast.success(cancelling ? `已取消 ${participantName} 的訂單` : "已更新訂單");
          onDone();
        },
        onError: (e) => toast.error(errorMessage(e)),
      },
    );
  }

  if (!group.canManage || group.status === "archived") return null;

  return (
    <div className="space-y-4">
      <ul className="divide-y rounded-xl border">
        {menu.map((item) => {
          const q = qty[item.id] ?? 0;
          const conflict = item.soldOut && q > 0;
          return (
            <li
              key={item.id}
              className={cn("space-y-2 px-3 py-2", conflict && "bg-destructive/10")}
            >
              <div className="flex items-start justify-between gap-2">
                <span className="min-w-0 break-words text-base">
                  {item.name}
                  {item.soldOut && (
                    <span className="ml-2 rounded bg-muted px-1.5 py-0.5 text-xs">售完</span>
                  )}
                </span>
                <span className="shrink-0">{formatMoney(item.price)}</span>
              </div>
              <div className="flex items-center justify-between gap-2">
                {q > 0 ? (
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
                    disabled={q <= 0}
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
                    disabled={item.soldOut || q >= MAX_QTY}
                    onClick={() => setQuantity(item.id, q + 1)}
                  >
                    <Plus />
                  </Button>
                </div>
              </div>
              {q > 0 && noteOpen[item.id] && (
                <Input
                  className="h-11"
                  aria-label={`${item.name} 備註`}
                  placeholder="例如：不要辣"
                  maxLength={ITEM_NOTE_MAX}
                  value={notes[item.id] ?? ""}
                  onChange={(e) => setNotes((n) => ({ ...n, [item.id]: e.target.value }))}
                />
              )}
            </li>
          );
        })}
      </ul>

      <div className="space-y-1.5">
        <label htmlFor="organizer-order-note" className="text-sm font-medium">
          訂單備註（選填）
        </label>
        <Textarea
          id="organizer-order-note"
          rows={2}
          maxLength={ORDER_NOTE_MAX}
          value={orderNote}
          onChange={(e) => setOrderNote(e.target.value)}
        />
      </div>

      <div className="sticky bottom-0 space-y-2 border-t bg-popover pt-3 pb-[max(0.5rem,env(safe-area-inset-bottom))]">
        <p className="text-center text-base font-medium">
          共 {count} 個 {formatMoney(total)}
        </p>
        <div className="flex gap-2">
          <Button
            type="button"
            className="h-12 flex-1 text-base"
            disabled={submit.isPending || count === 0 || soldOutSelected}
            onClick={() => send(lines, orderNote, false)}
          >
            儲存訂單
          </Button>
          <Button type="button" variant="outline" className="h-12" onClick={onCancel}>
            返回
          </Button>
        </div>
        {initial && initial.lines.length > 0 && !confirmCancel && (
          <Button
            type="button"
            variant="ghost"
            className="h-11 w-full text-destructive"
            onClick={() => setConfirmCancel(true)}
          >
            取消此人訂單
          </Button>
        )}
        {confirmCancel && (
          <div className="flex flex-wrap items-center gap-2" role="alertdialog" aria-label="確認取消訂單">
            <span className="flex-1 text-sm">確定要取消 {participantName} 的訂單嗎？</span>
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
  );
}
