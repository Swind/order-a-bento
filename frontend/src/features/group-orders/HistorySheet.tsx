import { useState } from "react";
import { errorMessage } from "@/shared/api/errors";
import type { GroupOrderDetail, SubmissionHistory } from "@/shared/api/types";
import { formatHHmm, formatMD } from "@/shared/lib/datetime";
import { formatMoney } from "@/shared/lib/format";
import { cn } from "@/shared/lib/utils";
import { Badge } from "@/shared/ui/badge";
import { Button } from "@/shared/ui/button";
import { Sheet, SheetContent, SheetDescription, SheetHeader, SheetTitle } from "@/shared/ui/sheet";
import { Skeleton } from "@/shared/ui/skeleton";
import { useHistory } from "./hooks";
import { deviceColor, itemText } from "./lib";
import { OrganizerOrderEditor } from "./OrganizerOrderEditor";

function DeviceLine({ entry }: { entry: SubmissionHistory }) {
  if (entry.submittedBy === "organizer")
    return <p className="text-xs text-muted-foreground">開團者代為送出</p>;
  const c = entry.client;
  const short = c.deviceShort;
  const parts: React.ReactNode[] = [];
  if (c.uaSummary) parts.push(<span key="ua">{c.uaSummary}</span>);
  if (short)
    parts.push(
      <span key="dev">
        裝置{" "}
        <span
          data-testid="device-short"
          className="font-mono font-semibold"
          style={{ color: deviceColor(short) }}
        >
          #{short}
        </span>
      </span>,
    );
  if (c.ip) parts.push(<span key="ip">{c.ip}</span>);
  if (parts.length === 0) return null;
  return (
    <p className="text-xs text-muted-foreground">
      {parts.flatMap((p, i) => (i === 0 ? [p] : [" · ", p]))}
    </p>
  );
}

function HistoryEntry({ entry, canManage }: { entry: SubmissionHistory; canManage: boolean }) {
  const t = new Date(entry.submittedAt);
  return (
    <li className={cn("space-y-1 rounded-lg border p-3", entry.effective && "border-primary bg-primary/5")}>
      <div className="flex flex-wrap items-center gap-2">
        <span className="text-sm font-medium tabular-nums">
          {formatMD(t)} {formatHHmm(t)}
        </span>
        {entry.effective && <Badge>目前有效</Badge>}
        {entry.cancelled ? (
          <Badge variant="destructive">取消訂單</Badge>
        ) : (
          <span className="ml-auto font-medium">{formatMoney(entry.total)}</span>
        )}
      </div>
      {!entry.cancelled && (
        <ul className="text-sm">
          {entry.lines.map((l, i) => (
            <li key={i}>{itemText(l)}</li>
          ))}
        </ul>
      )}
      {entry.note && <p className="text-sm text-muted-foreground">備註：{entry.note}</p>}
      {canManage && <DeviceLine entry={entry} />}
      {canManage && entry.sharedDeviceWith.length > 0 && (
        <p className="text-xs font-medium text-amber-700 dark:text-amber-400">
          ⚠ 此裝置也替 {entry.sharedDeviceWith.join("、")} 送出過訂單
        </p>
      )}
    </li>
  );
}

interface Props {
  group: GroupOrderDetail;
  participant: { id: string; name: string } | null;
  onClose: () => void;
}

export function HistorySheet({ group, participant, onClose }: Props) {
  const query = useHistory(group.id, participant?.id ?? null);
  const [editing, setEditing] = useState(false);
  const readOnly = group.status === "archived" || !group.canManage;
  const showEditor = editing && !readOnly && !query.isError;
  const effective = query.data?.find((e) => e.effective && !e.cancelled) ?? null;

  function close() {
    setEditing(false);
    onClose();
  }

  return (
    <Sheet open={!!participant} onOpenChange={(open) => !open && close()}>
      <SheetContent side="bottom" className="max-h-[90dvh] overflow-y-auto p-4 pb-[max(1rem,env(safe-area-inset-bottom))]">
        <SheetHeader className="p-0">
          <SheetTitle>{participant?.name}</SheetTitle>
          <SheetDescription>{showEditor ? "代為修改訂單" : "送出紀錄"}</SheetDescription>
        </SheetHeader>
        {participant && showEditor && query.data && (
          <OrganizerOrderEditor
            group={group}
            participantId={participant.id}
            participantName={participant.name}
            initial={
              effective ? { lines: effective.lines, note: effective.note } : null
            }
            onDone={() => setEditing(false)}
            onCancel={() => setEditing(false)}
          />
        )}
        {!showEditor && (
          <>
            {query.isPending && <Skeleton className="h-24 w-full" />}
            {query.isError && (
              <p role="alert" className="text-destructive">
                {errorMessage(query.error)}
              </p>
            )}
            {!query.isError && query.data && query.data.length === 0 && (
              <p className="text-muted-foreground">尚無送出紀錄</p>
            )}
            {!query.isError && query.data && query.data.length > 0 && (
              <ol className="space-y-2" aria-label="送出紀錄">
                {query.data.map((e) => (
                  <HistoryEntry key={e.id} entry={e} canManage={group.canManage} />
                ))}
              </ol>
            )}
            {!readOnly && !query.isError && query.data && (
              <Button type="button" className="h-11" onClick={() => setEditing(true)}>
                代為修改訂單
              </Button>
            )}
          </>
        )}
      </SheetContent>
    </Sheet>
  );
}
