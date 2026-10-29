import { Pencil } from "lucide-react";
import { useState } from "react";
import { toast } from "sonner";
import { DeliveryAddressSelect } from "@/features/delivery-addresses/DeliveryAddressSelect";
import { errorMessage } from "@/shared/api/errors";
import type { GroupOrderDetail } from "@/shared/api/types";
import { ConfirmActionButton } from "@/shared/components/ConfirmActionButton";
import { formatHHmm, formatMD, parseLocalInput, toLocalInputValue } from "@/shared/lib/datetime";
import { Button } from "@/shared/ui/button";
import { Input } from "@/shared/ui/input";
import { StatusBadge } from "./GroupCard";
import { usePatchGroup, useRotateShareToken } from "./hooks";

function DeliveryAddressEditor({ group, readOnly }: { group: GroupOrderDetail; readOnly: boolean }) {
  const patch = usePatchGroup(group.id);
  const [editing, setEditing] = useState(false);
  const [id, setId] = useState("");

  function save(deliveryAddressId: string | null) {
    patch.mutate({ deliveryAddressId }, {
      onSuccess: () => setEditing(false),
      onError: (e) => toast.error(errorMessage(e)),
    });
  }

  return (
    <div className="space-y-2 text-sm">
      {editing && !readOnly ? (
        <div className="space-y-2">
          <DeliveryAddressSelect value={id} onChange={setId} disabled={patch.isPending} />
          <div className="flex flex-wrap gap-2">
            <Button type="button" className="h-11" disabled={!id || patch.isPending} onClick={() => save(id)}>
              儲存送餐地址
            </Button>
            {group.deliveryAddress && (
              <Button type="button" variant="outline" className="h-11" disabled={patch.isPending} onClick={() => save(null)}>
                清除地址
              </Button>
            )}
            <Button type="button" variant="ghost" className="h-11" onClick={() => setEditing(false)}>
              取消
            </Button>
          </div>
        </div>
      ) : !readOnly && (
        <Button type="button" variant="outline" className="h-11" onClick={() => { setId(""); setEditing(true); }}>
          設定送餐地址
        </Button>
      )}
    </div>
  );
}

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

  if (!editing || readOnly)
    return (
      <div className="flex min-w-0 max-w-full items-center gap-1">
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

  if (!editing || readOnly)
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
  const readOnly = archived || !group.canManage;

  function setStatus(status: "open" | "closed" | "archived") {
    if (readOnly) return;
    patch.mutate({ status }, { onError: (e) => toast.error(errorMessage(e)) });
  }

  return (
    <div className="space-y-3">
      <div className="flex flex-wrap items-center gap-2">
        <TitleEditor key={group.title} group={group} readOnly={readOnly} />
        <StatusBadge status={group.status} deadlineAt={group.deadlineAt} />
      </div>
      <div className="space-y-1">
        <p className="break-words font-medium">
          {group.vendor.name}
          {group.vendor.phone && <span className="font-normal text-muted-foreground"> · {group.vendor.phone}</span>}
        </p>
        <p className="break-words text-sm text-muted-foreground">{group.sharingGroupId ? `共用群組：${group.sharingGroupName ?? group.sharingGroupId}` : "個人"}</p>
      </div>
      <div className="grid gap-x-4 gap-y-1 text-sm text-muted-foreground sm:grid-cols-2">
        <p>{group.deadlineAt ? `截止 ${formatMD(new Date(group.deadlineAt))} ${formatHHmm(group.deadlineAt)}` : "未設定截止時間"}</p>
        <p className="break-words">{group.deliveryAddress ? `送餐地址：${group.deliveryAddress}` : "尚未設定送餐地址"}</p>
      </div>
      {group.note && <p className="whitespace-pre-wrap break-words text-sm">{group.note}</p>}

      {archived ? (
        <p className="rounded-lg bg-muted p-3 text-sm text-muted-foreground">
          此團已封存，內容為唯讀。
        </p>
      ) : readOnly ? (
        <p className="rounded-lg bg-muted p-3 text-sm text-muted-foreground">此團為唯讀，只有群組管理員與仍在群組內的開團者可管理。</p>
      ) : (
        <div className="flex flex-wrap items-start gap-3">
          {group.status === "open" && (
            <Button
              type="button"
              variant="outline"
              className="h-11"
              disabled={patch.isPending}
              onClick={() => setStatus("closed")}
            >
              關閉收單
            </Button>
          )}
          {group.status === "closed" && (
            <Button
              type="button"
              className="h-11"
              disabled={patch.isPending}
              onClick={() => setStatus("open")}
            >
              重新開放
            </Button>
          )}
          <details className="min-w-0 max-w-full rounded-lg border bg-card open:w-full">
            <summary className="min-h-11 cursor-pointer content-center rounded-lg px-3 py-2 text-sm font-medium focus-visible:outline-2 focus-visible:outline-ring">更多操作</summary>
            <div className="space-y-3 border-t p-3">
              <DeadlineEditor key={group.deadlineAt ?? "none"} group={group} readOnly={false} />
              <DeliveryAddressEditor group={group} readOnly={false} />
              <div className="flex flex-wrap gap-2 border-t pt-4">
                {group.status === "closed" && <ConfirmActionButton
                  label="封存"
                  message="封存後此團將變成唯讀，無法再修改訂單。"
                  pending={patch.isPending}
                  onConfirm={() => setStatus("archived")}
                />}
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
            </div>
          </details>
        </div>
      )}
    </div>
  );
}
