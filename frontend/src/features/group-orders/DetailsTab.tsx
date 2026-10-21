import { EmptyState } from "@/shared/components/EmptyState";
import { useRef, useState } from "react";
import { toast } from "sonner";
import { errorMessage, fieldError } from "@/shared/api/errors";
import type { GroupOrderDetail } from "@/shared/api/types";
import { FieldError } from "@/shared/components/FieldError";
import { formatHHmm, formatMD } from "@/shared/lib/datetime";
import { formatMoney } from "@/shared/lib/format";
import { Badge } from "@/shared/ui/badge";
import { Button } from "@/shared/ui/button";
import { Input } from "@/shared/ui/input";
import { Textarea } from "@/shared/ui/textarea";
import { HistorySheet } from "./HistorySheet";
import { useAddParticipant, useParticipants } from "./hooks";
import { buildConfirmationText, itemText, splitParticipants } from "./lib";
import { QueryGate } from "./ParticipantsQuery";

function AddParticipant({ groupId }: { groupId: string }) {
  const add = useAddParticipant(groupId);
  const [open, setOpen] = useState(false);
  const [name, setName] = useState("");
  const [error, setError] = useState<string | null>(null);

  if (!open)
    return (
      <Button type="button" variant="outline" className="h-11" onClick={() => setOpen(true)}>
        ＋ 臨時加入成員
      </Button>
    );
  return (
    <form
      className="space-y-1"
      onSubmit={(e) => {
        e.preventDefault();
        const trimmed = name.trim();
        if (!trimmed) return;
        setError(null);
        add.mutate(trimmed, {
          onSuccess: () => {
            toast.success(`已加入 ${trimmed}`);
            setName("");
            setOpen(false);
          },
          onError: (err) => {
            const fe = fieldError(err, ["name"]);
            if (fe) setError(fe.message);
            else toast.error(errorMessage(err));
          },
        });
      }}
    >
      <div className="flex flex-wrap gap-2">
        <Input
          className="h-11 min-w-0 flex-1"
          aria-label="成員姓名"
          aria-invalid={!!error}
          aria-describedby={error ? "add-participant-error" : undefined}
          maxLength={40}
          autoFocus
          value={name}
          onChange={(e) => setName(e.target.value)}
        />
        <Button type="submit" className="h-11" disabled={add.isPending || !name.trim()}>
          加入
        </Button>
        <Button
          type="button"
          variant="ghost"
          className="h-11"
          onClick={() => {
            setOpen(false);
            setError(null);
          }}
        >
          取消
        </Button>
      </div>
      <FieldError id="add-participant-error" message={error} />
    </form>
  );
}

export function DetailsTab({ group }: { group: GroupOrderDetail }) {
  const query = useParticipants(group.id, group.status === "open");
  const [selected, setSelected] = useState<{ id: string; name: string } | null>(null);
  const archived = group.status === "archived";
  const areaRef = useRef<HTMLTextAreaElement>(null);

  const gate = <QueryGate query={query} />;
  const { ordered, notOrdered } = splitParticipants(query.data ?? []);
  const text = buildConfirmationText({ title: group.title, vendorName: group.vendor.name, participants: query.data ?? [] });

  async function copy() {
    try {
      if (!navigator.clipboard?.writeText) throw new Error("clipboard unavailable");
      await navigator.clipboard.writeText(text);
      toast.success("已複製訂餐確認列表");
    } catch {
      areaRef.current?.focus();
      areaRef.current?.select();
      toast.message("無法自動複製，已選取文字，請手動複製");
    }
  }

  return (
    <div className="space-y-4">
      {!archived && group.canManage && !query.isError && <AddParticipant groupId={group.id} />}
      {query.data && !query.isError ? (
        <>
          {ordered.length === 0 && (
            <EmptyState title="還沒有人下單" hint="把訂購頁連結分享給大家，下單後會出現在這裡。" />
          )}
          <ul className="divide-y rounded-xl border" aria-label="已訂餐">
            {ordered.map((p) => (
              <li key={p.id}>
                <button
                  type="button"
                  className="flex min-h-11 w-full flex-col gap-1 px-4 py-3 text-left hover:bg-muted/50 focus-visible:outline-none focus-visible:ring-3 focus-visible:ring-ring/50"
                  onClick={() => setSelected({ id: p.id, name: p.name })}
                >
                  <span className="flex flex-wrap items-center gap-2">
                    <span className="text-base font-medium">{p.name}</span>
                    {p.submissionCount > 1 && (
                      <Badge variant="secondary">改過 {p.submissionCount} 次</Badge>
                    )}
                    {group.canManage && p.suspiciousDevice && <Badge variant="destructive">⚠ 同裝置多人</Badge>}
                    <span className="ml-auto font-medium">{formatMoney(p.amountDue)}</span>
                  </span>
                  <span className="break-words text-sm">{p.items.map(itemText).join("、")}</span>
                  {p.orderNote && (
                    <span className="text-sm text-muted-foreground">備註：{p.orderNote}</span>
                  )}
                  {p.submittedAt && (
                    <span className="text-xs text-muted-foreground">
                      {formatMD(new Date(p.submittedAt))} {formatHHmm(p.submittedAt)} 送出
                    </span>
                  )}
                </button>
              </li>
            ))}
          </ul>

          {ordered.length > 0 && (
            <section className="space-y-2" aria-labelledby="confirmation-preview">
              <h2 id="confirmation-preview" className="font-semibold">LINE 訂餐確認列表</h2>
              <p className="text-sm text-muted-foreground">列出目前每人餐點、數量與備註，複製後可貼到 LINE 請大家確認。</p>
              <Textarea
                ref={areaRef}
                readOnly
                rows={Math.min(12, text.split("\n").length)}
                className="max-h-80 leading-relaxed"
                aria-label="訂餐確認列表"
                value={text}
              />
              <Button type="button" className="h-11" onClick={copy}>複製訂餐確認列表</Button>
            </section>
          )}

          {notOrdered.length > 0 && (
            <section aria-label="未訂餐" className="space-y-2 text-muted-foreground">
              <h3 className="text-sm font-medium">未訂餐（{notOrdered.length}）</h3>
              <ul className="flex flex-wrap gap-2">
                {notOrdered.map((p) => (
                  <li key={p.id}>
                    <button
                      type="button"
                      className="min-h-11 rounded-full border bg-muted/50 px-4 text-sm focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-ring"
                      onClick={() => setSelected({ id: p.id, name: p.name })}
                    >
                      {p.name}
                    </button>
                  </li>
                ))}
              </ul>
            </section>
          )}
        </>
      ) : (
        gate
      )}
      <HistorySheet group={group} participant={selected} onClose={() => setSelected(null)} />
    </div>
  );
}
