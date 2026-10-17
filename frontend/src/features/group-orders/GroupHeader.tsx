import { Pencil } from "lucide-react";
import { useState } from "react";
import { toast } from "sonner";
import { errorMessage } from "@/shared/api/errors";
import type { GroupOrderDetail } from "@/shared/api/types";
import { ConfirmActionButton } from "@/shared/components/ConfirmActionButton";
import { formatHHmm, formatMD, parseLocalInput, toLocalInputValue } from "@/shared/lib/datetime";
import { Button } from "@/shared/ui/button";
import { Input } from "@/shared/ui/input";
import { StatusBadge } from "./GroupCard";
import { usePatchGroup, useRotateShareToken } from "./hooks";

function TitleEditor({ group, readOnly }: { group: GroupOrderDetail; readOnly: boolean }) {
  const patch = usePatchGroup(group.id);
  const [editing, setEditing] = useState(false);
  const [value, setValue] = useState(group.title);

  function save() {
    const title = value.trim();
    if (!title || title === group.title) {
      setEditing(false);
      return;
    }
    patch.mutate(
      { title },
      {
        onSuccess: () => setEditing(false),
        onError: (e) => toast.error(errorMessage(e)),
      },
    );
  }

  if (!editing)
    return (
      <div className="flex min-w-0 items-center gap-1">
        <h1 className="min-w-0 break-words text-2xl font-bold">{group.title}</h1>
        {!readOnly && (
          <Button
            type="button"
            variant="ghost"
            className="size-11 shrink-0"
            aria-label="修改標題"
            onClick={() => {
              setValue(group.title);
              setEditing(true);
            }}
          >
            <Pencil />
          </Button>
        )}
      </div>
    );
  return (
    <form
      className="flex flex-wrap items-center gap-2"
      onSubmit={(e) => {
        e.preventDefault();
        save();
      }}
    >
      <Input
        className="h-11 min-w-0 flex-1"
        aria-label="標題"
        maxLength={80}
        value={value}
        onChange={(e) => setValue(e.target.value)}
        autoFocus
      />
      <Button type="submit" className="h-11" disabled={patch.isPending}>
        儲存
      </Button>
      <Button type="button" variant="outline" className="h-11" onClick={() => setEditing(false)}>
        取消
      </Button>
    </form>
  );
}

function DeadlineEditor({ group, readOnly }: { group: GroupOrderDetail; readOnly: boolean }) {
  const patch = usePatchGroup(group.id);
  const [editing, setEditing] = useState(false);
  const [value, setValue] = useState("");
  const deadline = group.deadlineAt ? new Date(group.deadlineAt) : null;

  function apply(body: { deadlineAt: string | null }) {
    patch.mutate(body, {
      onSuccess: () => setEditing(false),
      onError: (e) => toast.error(errorMessage(e)),
    });
  }

  if (!editing)
    return (
      <div className="flex flex-wrap items-center gap-1 text-sm text-muted-foreground">
        <span>{deadline ? `截止 ${formatMD(deadline)} ${formatHHmm(deadline)}` : "未設定截止時間"}</span>
        {!readOnly && (
          <Button
            type="button"
            variant="ghost"
            className="h-11 px-2"
            onClick={() => {
              setValue(deadline ? toLocalInputValue(deadline) : "");
              setEditing(true);
            }}
          >
            修改截止時間
          </Button>
        )}
      </div>
    );
  const parsed = parseLocalInput(value);
  return (
    <div className="flex flex-wrap items-center gap-2">
      <Input
        type="datetime-local"
        className="h-11 w-auto min-w-0 max-w-full"
        aria-label="截止時間"
        value={value}
        onChange={(e) => setValue(e.target.value)}
      />
      <Button
        type="button"
        className="h-11"
        disabled={!parsed || patch.isPending}
        onClick={() => parsed && apply({ deadlineAt: parsed.toISOString() })}
      >
        儲存
      </Button>
      <Button
        type="button"
        variant="outline"
        className="h-11"
        disabled={patch.isPending}
        onClick={() => apply({ deadlineAt: null })}
      >
        清除
      </Button>
      <Button type="button" variant="ghost" className="h-11" onClick={() => setEditing(false)}>
        取消
      </Button>
    </div>
  );
}

export function GroupHeader({ group }: { group: GroupOrderDetail }) {
  const patch = usePatchGroup(group.id);
  const rotate = useRotateShareToken(group.id);
  const archived = group.status === "archived";

  function setStatus(status: "open" | "closed" | "archived") {
    patch.mutate({ status }, { onError: (e) => toast.error(errorMessage(e)) });
  }

  return (
    <div className="space-y-2">
      <div className="flex flex-wrap items-center gap-2">
        <TitleEditor key={group.title} group={group} readOnly={archived} />
        <StatusBadge status={group.status} />
      </div>
      <p className="text-muted-foreground">
        {group.vendor.name}
        {group.vendor.phone && ` · ${group.vendor.phone}`}
      </p>
      <DeadlineEditor key={group.deadlineAt ?? "none"} group={group} readOnly={archived} />
      {group.note && <p className="text-sm">{group.note}</p>}

      {archived ? (
        <p className="rounded-lg bg-muted p-3 text-sm text-muted-foreground">
          此團已封存，內容為唯讀。
        </p>
      ) : (
        <div className="flex flex-wrap items-start gap-2">
          {group.status === "open" && (
            <Button
              type="button"
              className="h-11"
              disabled={patch.isPending}
              onClick={() => setStatus("closed")}
            >
              關閉收單
            </Button>
          )}
          {group.status === "closed" && (
            <>
              <Button
                type="button"
                className="h-11"
                disabled={patch.isPending}
                onClick={() => setStatus("open")}
              >
                重新開放
              </Button>
              <ConfirmActionButton
                label="封存"
                message="封存後此團將變成唯讀，無法再修改訂單。"
                pending={patch.isPending}
                onConfirm={() => setStatus("archived")}
              />
            </>
          )}
          <ConfirmActionButton
            label="重新產生連結"
            message="舊的分享連結會立即失效，已分享出去的連結將無法再使用。"
            pending={rotate.isPending}
            onConfirm={() =>
              rotate.mutate(undefined, {
                onSuccess: () => toast.success("已產生新的分享連結"),
                onError: (e) => toast.error(errorMessage(e)),
              })
            }
          />
        </div>
      )}
    </div>
  );
}
