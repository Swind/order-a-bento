import { useState } from "react";
import { Link } from "react-router";
import { errorMessage } from "@/shared/api/errors";
import type { GroupStatus } from "@/shared/api/types";
import { EmptyState } from "@/shared/components/EmptyState";
import { Button, buttonVariants } from "@/shared/ui/button";
import { Skeleton } from "@/shared/ui/skeleton";
import { GroupCardList } from "./GroupCard";
import { useGroups } from "./hooks";
import { ALL_STATUSES, DEFAULT_STATUSES, toggleStatus } from "./lib";

const CHIPS: { status: GroupStatus; label: string }[] = [
  { status: "open", label: "進行中" },
  { status: "closed", label: "已關閉" },
  { status: "archived", label: "已封存" },
];

export function GroupOrdersPage() {
  const query = useGroups();
  const [statuses, setStatuses] = useState<GroupStatus[]>(DEFAULT_STATUSES);
  const allSelected = ALL_STATUSES.every((s) => statuses.includes(s));
  const chipClass = (on: boolean) =>
    `h-11 rounded-full px-4 ${on ? "" : "bg-background"}`;

  return (
    <div className="mx-auto max-w-2xl space-y-6">
      <div className="flex items-center justify-between gap-2">
        <div className="space-y-1"><h1 className="text-2xl font-bold tracking-tight">開團紀錄</h1><p className="text-sm text-muted-foreground">查看收單進度，或回顧過去的午餐。</p></div>
        <Link to="/group-orders/new" className={buttonVariants({ className: "h-11" })}>
          開團
        </Link>
      </div>
      <div className="flex flex-wrap gap-2" role="group" aria-label="狀態篩選">
        {CHIPS.map((c) => {
          const on = statuses.includes(c.status);
          return (
            <Button
              key={c.status}
              type="button"
              variant={on ? "default" : "outline"}
              aria-pressed={on}
              className={chipClass(on)}
              onClick={() => setStatuses((cur) => toggleStatus(cur, c.status))}
            >
              {c.label}
            </Button>
          );
        })}
        <Button
          type="button"
          variant={allSelected ? "default" : "outline"}
          aria-pressed={allSelected}
          className={chipClass(allSelected)}
          onClick={() => setStatuses(ALL_STATUSES)}
        >
          全部
        </Button>
      </div>

      {query.isPending && (
        <div className="space-y-3" aria-busy="true">
          <Skeleton className="h-24 w-full" />
          <Skeleton className="h-24 w-full" />
        </div>
      )}
      {query.isError && (
        <p role="alert" className="text-destructive">
          {errorMessage(query.error)}
        </p>
      )}
      {!query.isError && query.data &&
        (() => {
          const shown = query.data.filter((g) => statuses.includes(g.status));
          if (shown.length === 0)
            return (
              <EmptyState
                title="沒有符合的開團紀錄"
                hint="換個狀態篩選看看，或開一個新團。"
                action={
                  <Link to="/group-orders/new" className={buttonVariants({ className: "h-11" })}>
                    開團
                  </Link>
                }
              />
            );
          return <GroupCardList groups={shown} />;
        })()}
    </div>
  );
}
