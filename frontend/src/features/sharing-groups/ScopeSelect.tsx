import { useSharingGroups } from "./hooks";

export function ScopeSelect({ value, onChange, ownership = false }: { value: string; onChange: (value: string) => void; ownership?: boolean }) {
  const groups = useSharingGroups();
  const id = ownership ? "order-ownership" : "directory-scope";
  return (
    <div className="space-y-1">
      <label htmlFor={id} className="text-sm font-medium">{ownership ? "開團歸屬" : "資料範圍"}</label>
      <select
        id={id}
        className="block h-11 w-full min-w-0 rounded-lg border border-input bg-background px-3 text-base outline-none focus-visible:border-ring focus-visible:ring-3 focus-visible:ring-ring/50"
        value={value}
        onChange={(event) => onChange(event.target.value)}
      >
        <option value="">個人</option>
        {value && groups.isSuccess && !groups.data.some((group) => group.id === value) && (
          <option value={value} disabled>原共用群組（已無法存取，請重新選擇）</option>
        )}
        {groups.data?.map((group) => (
          <option key={group.id} value={group.id}>
            {group.name}（{group.role === "admin" ? "管理員" : ownership ? "成員" : "唯讀"}）
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
