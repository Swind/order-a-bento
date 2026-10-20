import { ArrowRight, Plus, Utensils } from "lucide-react";
import { Link } from "react-router";
import { errorMessage } from "@/shared/api/errors";
import { EmptyState } from "@/shared/components/EmptyState";
import { buttonVariants } from "@/shared/ui/button";
import { Skeleton } from "@/shared/ui/skeleton";
import { GroupCardList } from "./GroupCard";
import { useGroups } from "./hooks";

export function DashboardPage() {
  const query = useGroups();
  const open = query.data?.filter((g) => g.status === "open") ?? [];
  return (
    <div className="mx-auto max-w-2xl space-y-8">
      <div className="flex flex-col gap-5 rounded-2xl border bg-card p-5 sm:flex-row sm:items-center sm:justify-between sm:p-6">
        <div className="space-y-2">
          <p className="flex items-center gap-2 text-sm font-medium text-primary">
            <Utensils className="size-4" /> 一起好好吃午餐
          </p>
          <h1 className="text-3xl font-bold tracking-tight">今天訂什麼？</h1>
          <p className="text-sm text-muted-foreground">選好名單和店家，把連結分享給大家。</p>
        </div>
        <Link
          to="/group-orders/new"
          className={buttonVariants({ size: "lg", className: "h-12 w-full px-6 text-base sm:w-auto" })}
        >
          <Plus />
          開團
        </Link>
      </div>

      <section className="space-y-3" aria-label="進行中的團">
        <h2 className="flex items-center gap-2 text-lg font-semibold">
          進行中的團
          {query.data && (
            <span className="rounded-full bg-secondary px-2 py-0.5 text-sm tabular-nums text-secondary-foreground">{open.length}</span>
          )}
        </h2>
        {query.isPending && <Skeleton className="h-24 w-full" />}
        {query.isError && (
          <p role="alert" className="text-destructive">
            {errorMessage(query.error)}
          </p>
        )}
        {query.data && open.length === 0 && (
          <EmptyState title="目前沒有進行中的團" hint="按上方「開團」，選好名單與店家就能開始收訂單。" />
        )}
        {open.length > 0 && <GroupCardList groups={open} />}
        <Link
          to="/group-orders"
          className={buttonVariants({ variant: "ghost", className: "h-11 text-muted-foreground" })}
        >
          查看所有開團紀錄
          <ArrowRight />
        </Link>
      </section>
    </div>
  );
}
