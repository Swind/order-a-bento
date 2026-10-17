import { Link } from "react-router";
import type { GroupListItem } from "@/shared/api/types";
import { formatHHmm, formatMD } from "@/shared/lib/datetime";
import { formatMoney } from "@/shared/lib/format";
import { Badge } from "@/shared/ui/badge";
import { STATUS_LABEL } from "./lib";

export function StatusBadge({ status }: { status: string }) {
  return (
    <Badge variant={status === "open" ? "default" : "secondary"}>
      {STATUS_LABEL[status] ?? status}
    </Badge>
  );
}

function whenText(g: GroupListItem): string {
  if (g.deadlineAt) {
    const d = new Date(g.deadlineAt);
    return `截止 ${formatMD(d)} ${formatHHmm(d)}`;
  }
  return `建立 ${formatMD(new Date(g.createdAt))}`;
}

export function GroupCard({ group }: { group: GroupListItem }) {
  return (
    <li>
      <Link
        to={`/group-orders/${group.id}`}
        className="flex min-h-11 flex-col gap-1 rounded-xl border bg-card p-4 hover:bg-muted/50 focus-visible:outline-none focus-visible:ring-3 focus-visible:ring-ring/50"
      >
        <span className="flex items-start justify-between gap-2">
          <span className="min-w-0 break-words text-base font-semibold">{group.title}</span>
          <StatusBadge status={group.status} />
        </span>
        <span className="text-sm text-muted-foreground">
          {group.vendorName} · {whenText(group)}
        </span>
        <span className="flex items-center justify-between text-sm">
          <span>
            已訂 {group.orderedCount}/{group.participantCount} 人
          </span>
          <span className="font-medium">{formatMoney(group.totalAmount)}</span>
        </span>
      </Link>
    </li>
  );
}

export function GroupCardList({ groups }: { groups: GroupListItem[] }) {
  return (
    <ul className="space-y-3">
      {groups.map((g) => (
        <GroupCard key={g.id} group={g} />
      ))}
    </ul>
  );
}
