import { useState } from "react";
import { Link, useParams } from "react-router";
import { errorMessage } from "@/shared/api/errors";
import { ConfirmDeleteButton } from "@/shared/components/ConfirmDeleteButton";
import { Button } from "@/shared/ui/button";
import { Input } from "@/shared/ui/input";
import { useSharingGroup, useSharingMembers, useSharingMutation } from "./hooks";

export function SharingGroupDetailPage() {
  const { id = "" } = useParams();
  const group = useSharingGroup(id);
  const members = useSharingMembers(id);
  const mutation = useSharingMutation(id);
  const [name, setName] = useState<string | null>(null);
  const [userId, setUserId] = useState("");
  const [role, setRole] = useState<"admin" | "member">("member");
  const admin = group.data?.role === "admin";
  if (group.isPending) return <p role="status">載入中…</p>;
  if (group.isError) {
    return (
      <div className="space-y-3">
        <Link to="/sharing-groups" className="text-sm underline">← 共用群組</Link>
        <p role="alert" className="text-destructive">{errorMessage(group.error)}</p>
        <Button variant="outline" onClick={() => group.refetch()}>重試</Button>
      </div>
    );
  }

  const currentName = name ?? group.data.name;
  return (
    <div className="space-y-5">
      <Link to="/sharing-groups" className="text-sm underline">← 共用群組</Link>
      <h1 className="text-2xl font-bold">{group.data.name}</h1>
      {admin && (
        <>
          <form
            className="flex flex-wrap items-end gap-2"
            onSubmit={(event) => {
              event.preventDefault();
              if (!currentName.trim()) return;
              mutation.mutate({ method: "PATCH", body: { name: currentName.trim() } });
            }}
          >
            <label className="space-y-1">
              群組名稱
              <Input required maxLength={100} value={currentName} onChange={(event) => setName(event.target.value)} />
            </label>
            <Button type="submit" disabled={mutation.isPending}>儲存名稱</Button>
          </form>
          <form
            className="space-y-2 rounded-lg border p-3"
            onSubmit={(event) => {
              event.preventDefault();
              if (!userId.trim()) return;
              mutation.mutate(
                { suffix: "/members", body: { userId: userId.trim(), role } },
                { onSuccess: () => setUserId("") },
              );
            }}
          >
            <h2 className="font-semibold">加入既有使用者</h2>
            <p className="text-sm text-muted-foreground">
              請對方提供「共用群組」頁面上的本站使用者 ID。
            </p>
            <label className="block space-y-1">
              本站使用者 ID
              <Input required value={userId} onChange={(event) => setUserId(event.target.value)} />
            </label>
            <label className="flex items-center gap-2">
              加入角色
              <select
                className="h-11 rounded-md border bg-background px-3"
                value={role}
                onChange={(event) => setRole(event.target.value as "admin" | "member")}
              >
                <option value="member">成員</option>
                <option value="admin">管理員</option>
              </select>
            </label>
            <Button type="submit" disabled={mutation.isPending || !userId.trim()}>加入成員</Button>
          </form>
        </>
      )}
      {mutation.isError && <p role="alert" className="text-destructive">{errorMessage(mutation.error)}</p>}
      <section className="space-y-2" aria-label="群組成員">
        <h2 className="text-lg font-semibold">群組成員</h2>
        <p className="text-sm text-muted-foreground">
          成員可讀取並選用共用名單與店家；管理員可維護共用資料。最後一位管理員不能移除或降級。
        </p>
        {members.isError && (
          <div className="space-y-2">
            <p role="alert" className="text-destructive">{errorMessage(members.error)}</p>
            <Button variant="outline" onClick={() => members.refetch()}>重試</Button>
          </div>
        )}
        {members.isPending && <p role="status">載入成員中…</p>}
        <ul className="space-y-2">
          {!members.isError && members.data?.map((member) => (
            <li key={member.id} className="space-y-2 rounded-lg border p-3">
              <p className="font-medium">
                {member.displayName} · {member.role === "admin" ? "管理員" : "成員"}
              </p>
              <p className="break-all text-sm">
                {member.email}{member.identityKind ? ` · ${member.identityKind}` : ""} · <code>{member.id}</code>
              </p>
              {admin && (
                <div className="flex items-center gap-2">
                  <select
                    aria-label={`${member.displayName} 的角色`}
                    className="h-11 rounded-md border bg-background px-3"
                    value={member.role ?? "member"}
                    disabled={mutation.isPending}
                    onChange={(event) => mutation.mutate({
                      suffix: `/members/${encodeURIComponent(member.id)}`,
                      method: "PATCH",
                      body: { role: event.target.value },
                    })}
                  >
                    <option value="member">成員</option>
                    <option value="admin">管理員</option>
                  </select>
                  <ConfirmDeleteButton
                    label={`移除 ${member.displayName}`}
                    showLabel
                    pending={mutation.isPending}
                    onConfirm={() => mutation.mutate({
                      suffix: `/members/${encodeURIComponent(member.id)}`,
                      method: "DELETE",
                    })}
                  />
                </div>
              )}
            </li>
          ))}
        </ul>
      </section>
    </div>
  );
}
