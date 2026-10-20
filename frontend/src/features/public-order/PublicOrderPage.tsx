import { useState } from "react";
import { useParams } from "react-router";
import { ApiError } from "@/shared/api/client";
import { errorMessage } from "@/shared/api/errors";
import type { PublicGroup, PublicParticipant } from "@/shared/api/types";
import { formatDeadline, formatHHmm } from "@/shared/lib/datetime";
import { cn } from "@/shared/lib/utils";
import { useNow } from "@/shared/lib/useNow";
import { Button } from "@/shared/ui/button";
import { EmptyState } from "@/shared/components/EmptyState";
import { Input } from "@/shared/ui/input";
import { Skeleton } from "@/shared/ui/skeleton";
import { useEffectiveOrder, usePublicGroup } from "./hooks";
import { matchesName, readLastParticipant, writeLastParticipant } from "./lib";
import { OrderEditor } from "./OrderEditor";

function GroupHeader({ group, closedBanner }: { group: PublicGroup; closedBanner: string | null }) {
  const now = useNow(30_000);
  return (
    <header className="space-y-2 rounded-2xl border bg-card p-5">
      <p className="text-xs font-medium tracking-wide text-primary">🍱 一起訂便當</p>
      <h1 className="break-words text-2xl font-bold tracking-tight">{group.title}</h1>
      <p className="text-base">
        {group.vendor.name}
        {group.vendor.phone && (
          <>
            {" · "}
            <a href={`tel:${group.vendor.phone}`} className="underline">
              {group.vendor.phone}
            </a>
          </>
        )}
      </p>
      {group.deadlineAt && (
        <p className="text-sm text-muted-foreground">{formatDeadline(group.deadlineAt, now)}</p>
      )}
      {group.note && <p className="text-sm">{group.note}</p>}
      {closedBanner && (
        <p role="status" className="rounded-lg bg-destructive/10 px-3 py-2 text-center font-medium text-destructive">
          {closedBanner}
        </p>
      )}
    </header>
  );
}

function IdentityConfirm({
  token,
  participant,
  onYes,
  onNo,
}: {
  token: string;
  participant: PublicParticipant;
  onYes: () => void;
  onNo: () => void;
}) {
  const order = useEffectiveOrder(token, participant.id);
  if (order.isPending) return <Skeleton className="h-24 w-full" />;
  const at = order.data?.order && !order.data.order.cancelled ? order.data.order.submittedAt : null;
  return (
    <div role="alertdialog" aria-label="確認身分" className="space-y-3 rounded-xl border-2 border-primary p-4">
      <p className="text-base">
        {at
          ? `${participant.name} 已於 ${formatHHmm(at)} 送出訂單，你確定是本人嗎？`
          : `${participant.name} 已送出訂單，你確定是本人嗎？`}
      </p>
      <div className="flex gap-2">
        <Button className="h-11 flex-1" onClick={onYes}>
          是我
        </Button>
        <Button variant="outline" className="h-11 flex-1" onClick={onNo}>
          選錯了
        </Button>
      </div>
    </div>
  );
}

function ParticipantPicker({
  token,
  group,
  remembered,
  onChoose,
}: {
  token: string;
  group: PublicGroup;
  remembered: string | null;
  onChoose: (pid: string) => void;
}) {
  const [query, setQuery] = useState("");
  const [pending, setPending] = useState<PublicParticipant | null>(null);
  const shown = group.participants.filter((p) => matchesName(p.name, query));

  function pick(p: PublicParticipant) {
    if (p.hasOrdered && p.id !== remembered) setPending(p);
    else onChoose(p.id);
  }

  return (
    <section className="space-y-3" aria-label="選自己">
      <h2 className="text-lg font-semibold">你是誰？</h2>
      <Input
        type="search"
        className="h-11"
        placeholder="搜尋名字"
        aria-label="搜尋名字"
        value={query}
        onChange={(e) => setQuery(e.target.value)}
        autoComplete="off"
      />
      {pending && (
        <IdentityConfirm
          token={token}
          participant={pending}
          onYes={() => onChoose(pending.id)}
          onNo={() => setPending(null)}
        />
      )}
      {group.participants.length === 0 ? (
        <EmptyState title="名單裡還沒有人" hint="請向開團的人確認，或請他把你加進名單。" />
      ) : (
        shown.length === 0 && (
          <p className="py-6 text-center text-muted-foreground">
            找不到符合的名字，請確認是否輸入正確，或請開團的人把你加進名單。
          </p>
        )
      )}
      <ul className={cn("overflow-hidden divide-y rounded-xl border bg-card", shown.length === 0 && "hidden")}>
        {shown.map((p) => (
          <li key={p.id}>
            <button
              type="button"
              onClick={() => pick(p)}
              className={cn(
                "flex min-h-12 w-full items-center justify-between gap-3 px-4 py-2 text-left text-base hover:bg-muted/50 focus-visible:outline-none focus-visible:ring-3 focus-visible:ring-ring/50",
                pending?.id === p.id && "bg-muted",
              )}
            >
              <span className="truncate">{p.name}</span>
              {p.hasOrdered && (
                <span className="shrink-0 rounded-full bg-green-100 px-2 py-0.5 text-sm text-green-800 dark:bg-green-900/40 dark:text-green-300">
                  ✓ 已訂
                </span>
              )}
            </button>
          </li>
        ))}
      </ul>
    </section>
  );
}

function OrderStep({
  token,
  group,
  participant,
  readOnly,
  onClosed,
  onSwitch,
}: {
  token: string;
  group: PublicGroup;
  participant: PublicParticipant;
  readOnly: boolean;
  onClosed: (reason: "group_closed" | "deadline_passed") => void;
  onSwitch: () => void;
}) {
  const order = useEffectiveOrder(token, participant.id);
  return (
    <section className="space-y-3" aria-label="選便當">
      <div className="flex items-center justify-between gap-2 rounded-xl border bg-card px-4 py-2">
        <h2 className="min-w-0 truncate text-lg font-semibold">{participant.name}</h2>
        <Button variant="outline" className="h-11" onClick={onSwitch}>
          換人
        </Button>
      </div>
      {order.isPending && <Skeleton className="h-48 w-full" />}
      {order.isError && (
        <div className="space-y-2">
          <p role="alert" className="text-destructive">
            {errorMessage(order.error)}
          </p>
          <Button variant="outline" className="h-11" onClick={() => order.refetch()}>
            重試
          </Button>
        </div>
      )}
      {order.data && (
        <OrderEditor
          key={participant.id}
          token={token}
          group={group}
          participantId={participant.id}
          order={order.data.order ?? null}
          readOnly={readOnly}
          onClosed={onClosed}
        />
      )}
    </section>
  );
}

export function PublicOrderPage() {
  const { token = "" } = useParams();
  const query = usePublicGroup(token);
  const [selectedId, setSelectedId] = useState<string | null>(() => readLastParticipant(token));
  const [forcedClosed, setForcedClosed] = useState<"group_closed" | "deadline_passed" | null>(null);
  const now = useNow(30_000);

  if (query.isPending)
    return (
      <div className="space-y-3" aria-busy="true" aria-label="載入中">
        <Skeleton className="h-8 w-2/3" />
        <Skeleton className="h-5 w-1/2" />
        <Skeleton className="h-11 w-full" />
        <Skeleton className="h-40 w-full" />
      </div>
    );

  if (query.isError) {
    const notFound = query.error instanceof ApiError && query.error.status === 404;
    return (
      <div className="space-y-3 py-12 text-center">
        <p role="alert" className="text-lg">
          {notFound ? "這個訂購連結無效或已失效" : errorMessage(query.error)}
        </p>
        {notFound ? (
          <p className="text-sm text-muted-foreground">請向開團的人確認連結是否正確。</p>
        ) : (
          <Button className="h-11" onClick={() => query.refetch()}>
            重試
          </Button>
        )}
      </div>
    );
  }

  const group = query.data;
  const deadlinePassed = !!group.deadlineAt && new Date(group.deadlineAt).getTime() <= now.getTime();
  const readOnly = !group.acceptingOrders || forcedClosed !== null || deadlinePassed;
  const manuallyClosed = group.status !== "open" || forcedClosed === "group_closed";
  const bannerText = readOnly ? (manuallyClosed ? "已關閉" : "已截止") : null;

  const participant = selectedId ? group.participants.find((p) => p.id === selectedId) : undefined;

  function choose(pid: string) {
    writeLastParticipant(token, pid);
    setSelectedId(pid);
  }

  return (
    <div className="space-y-5 pb-[calc(var(--order-bar-h,0px)+1rem)]">
      <GroupHeader group={group} closedBanner={bannerText} />
      {participant ? (
        <OrderStep
          token={token}
          group={group}
          participant={participant}
          readOnly={readOnly}
          onClosed={(reason) => {
            setForcedClosed(reason);
            query.refetch();
          }}
          onSwitch={() => setSelectedId(null)}
        />
      ) : (
        <ParticipantPicker
          token={token}
          group={group}
          remembered={readLastParticipant(token)}
          onChoose={choose}
        />
      )}
      <p className="pt-4 text-center text-xs text-muted-foreground">
        為核對訂單，系統會記錄 IP 與裝置資訊
      </p>
    </div>
  );
}
