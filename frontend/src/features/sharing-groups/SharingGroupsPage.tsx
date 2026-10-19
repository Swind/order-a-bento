import { useState, type FormEvent } from "react";
import { Link } from "react-router";
import { toast } from "sonner";
import { useMe } from "@/features/auth/hooks";
import { errorMessage } from "@/shared/api/errors";
import { Button } from "@/shared/ui/button";
import { Input } from "@/shared/ui/input";
import { useSharingGroups, useSharingMutation } from "./hooks";

export function SharingGroupsPage() {
  const groups = useSharingGroups();
  const me = useMe();
  const create = useSharingMutation();
  const [name, setName] = useState("");
  function submit(event: FormEvent) {
    event.preventDefault();
    if (!name.trim()) return;
    create.mutate({ body: { name: name.trim() } }, { onSuccess: () => setName("") });
  }
  return (
    <div className="space-y-5">
      <h1 className="text-2xl font-bold">共用群組</h1>
      <div className="space-y-2 rounded-lg border p-3">
        <p>我的本站使用者 ID：<code className="break-all">{me.data?.id}</code></p>
        <Button
          variant="outline"
          disabled={!me.data}
          onClick={async () => {
            try {
              await navigator.clipboard.writeText(me.data!.id);
              toast.success("已複製使用者 ID");
            } catch {
              toast.error("無法複製，請手動選取使用者 ID");
            }
          }}
        >
          複製使用者 ID
        </Button>
        <p className="text-sm text-muted-foreground">
          將此 ID 交給群組管理員加入。不同登入身分不會因相同 Email 合併。
        </p>
      </div>
      <form className="flex flex-wrap items-end gap-2" onSubmit={submit}>
        <label className="space-y-1">
          群組名稱
          <Input required maxLength={100} value={name} onChange={(event) => setName(event.target.value)} />
        </label>
        <Button type="submit" disabled={!name.trim() || create.isPending}>建立共用群組</Button>
      </form>
      {create.isError && <p role="alert" className="text-destructive">{errorMessage(create.error)}</p>}
      {groups.isPending && <p role="status">載入中…</p>}
      {groups.isError && (
        <div className="space-y-2">
          <p role="alert" className="text-destructive">{errorMessage(groups.error)}</p>
          <Button variant="outline" onClick={() => groups.refetch()}>重試</Button>
        </div>
      )}
      {groups.data?.length === 0 && <p>尚未加入共用群組。</p>}
      <ul className="space-y-2">
        {!groups.isError && groups.data?.map((group) => (
          <li key={group.id}>
            <Link to={`/sharing-groups/${group.id}`} className="block rounded-lg border p-3 hover:bg-muted">
              {group.name} · {group.role === "admin" ? "管理員" : "成員"}
            </Link>
          </li>
        ))}
      </ul>
    </div>
  );
}
