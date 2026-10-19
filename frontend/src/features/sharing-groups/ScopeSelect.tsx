import { useSharingGroups } from "./hooks";

export function ScopeSelect({ value, onChange }: { value: string; onChange: (value: string) => void }) {
  const groups = useSharingGroups();
  return (
    <div className="space-y-1">
      <label htmlFor="directory-scope" className="text-sm font-medium">資料範圍</label>
      <select
        id="directory-scope"
        className="block h-11 w-full rounded-md border bg-background px-3"
        value={value}
        onChange={(event) => onChange(event.target.value)}
      >
        <option value="">個人</option>
        {groups.data?.map((group) => (
          <option key={group.id} value={group.id}>
            {group.name}（{group.role === "admin" ? "管理員" : "唯讀"}）
          </option>
        ))}
      </select>
      {groups.isError && (
        <p role="alert" className="text-sm text-destructive">無法載入共用群組，請重新整理後再試。</p>
      )}
    </div>
  );
}

export function ScopeBadge({ sharingGroupId, canManage }: {
  sharingGroupId?: string | null;
  canManage?: boolean;
}) {
  const groups = useSharingGroups();
  const groupName = groups.data?.find((group) => group.id === sharingGroupId)?.name ?? sharingGroupId;
  return (
    <p className="text-sm text-muted-foreground">
      {sharingGroupId ? `共用群組：${groupName}` : "個人資料"}
      {canManage === false ? " · 唯讀（可選用開團）" : ""}
    </p>
  );
}
