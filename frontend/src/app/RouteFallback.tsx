import { Skeleton } from "@/shared/ui/skeleton";

/** Small skeleton shown while a lazy route chunk is loading on first paint. */
export function RouteFallback() {
  return (
    <div className="mx-auto w-full max-w-md space-y-4 px-4 py-6" aria-busy="true" aria-label="載入中">
      <Skeleton className="h-8 w-40" />
      <Skeleton className="h-24 w-full" />
      <Skeleton className="h-24 w-full" />
    </div>
  );
}
