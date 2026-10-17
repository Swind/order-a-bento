import { Plus } from "lucide-react";
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
    <div className="mx-auto max-w-2xl space-y-5">
      <h1 className="text-2xl font-bold">首頁</h1>
      <Link
        to="/group-orders/new"
        className={buttonVariants({ size: "lg", className: "h-14 w-full text-lg sm:w-auto sm:px-8" })}
      >
        <Plus />
        開團
      </Link>

      <section className="space-y-3" aria-label="進行中的團">
        <h2 className="text-lg font-semibold">進行中的團</h2>
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
          className={buttonVariants({ variant: "outline", className: "h-11" })}
        >
          查看所有開團紀錄
        </Link>
      </section>
    </div>
  );
}
