import { errorMessage } from "@/shared/api/errors";
import { Button } from "@/shared/ui/button";
import { Skeleton } from "@/shared/ui/skeleton";
import type { UseQueryResult } from "@tanstack/react-query";

/** Loading / error placeholder shared by the tabs that read the participants query. */
export function QueryGate({ query }: { query: UseQueryResult<unknown> }) {
  if (query.isPending) return <Skeleton className="h-40 w-full" />;
  if (query.isError)
    return (
      <div className="space-y-2">
        <p role="alert" className="text-destructive">
          {errorMessage(query.error)}
        </p>
        <Button variant="outline" className="h-11" onClick={() => query.refetch()}>
          重試
        </Button>
      </div>
    );
  return null;
}
