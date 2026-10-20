import type { ReactNode } from "react";
import { cn } from "@/shared/lib/utils";

/** Friendly empty state: a headline, an optional hint pointing at the next action, optional CTA. */
export function EmptyState({
  icon,
  title,
  hint,
  action,
  className,
}: {
  icon?: ReactNode;
  title: string;
  hint?: string;
  action?: ReactNode;
  className?: string;
}) {
  return (
    <div
      className={cn(
        "flex flex-col items-center gap-3 rounded-xl border border-dashed bg-card px-6 py-12 text-center",
        className,
      )}
    >
      {icon && <div className="mb-1 flex size-12 items-center justify-center rounded-full bg-secondary text-secondary-foreground">{icon}</div>}
      <p className="text-base font-medium">{title}</p>
      {hint && <p className="max-w-sm text-sm leading-relaxed text-muted-foreground">{hint}</p>}
      {action}
    </div>
  );
}
