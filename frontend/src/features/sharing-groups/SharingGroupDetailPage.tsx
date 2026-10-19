import { useState } from "react";
import { Link, useParams } from "react-router";
import { errorMessage } from "@/shared/api/errors";
import { ConfirmDeleteButton } from "@/shared/components/ConfirmDeleteButton";
import { Button } from "@/shared/ui/button";
import { Input } from "@/shared/ui/input";
import { useSharingGroup, useSharingMembers, useSharingMutation, useSharingUserLookup } from "./hooks";

export function SharingGroupDetailPage() {
  const { id = "" } = useParams();
  const group = useSharingGroup(id);
  const members = useSharingMembers(id);
  const mutation = useSharingMutation(id);
  const lookup = useSharingUserLookup(id);
  const [name, setName] = useState<string | null>(null);
  const [userId, setUserId] = useState("");
  const [email, setEmail] = useState("");
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
              if (!email.trim()) return;
              setUserId("");
              lookup.mutate(email, {
                onSuccess: (users) => {
                  if (users.length === 1 && !users[0].role) setUserId(users[0].id);
                },
              });
            }}
          >
            <h2 className="font-semibold">加入既有使用者</h2>
            <p className="text-sm text-muted-foreground">
              輸入對方完整 Email 查找本站帳號，再確認加入；找不到時請對方先登入本站。不會寄送邀請信。
            </p>
            <label className="block space-y-1">
              使用者 Email
              <Input type="email" required maxLength={320} value={email} disabled={lookup.isPending || mutation.isPending} onChange={(event) => {
                setEmail(event.target.value);
                setUserId("");
                lookup.reset();
              }} />
            </label>
            <Button type="submit" disabled={lookup.isPending || mutation.isPending || !email.trim()}>
              {lookup.isPending ? "查找中…" : "查找帳號"}
            </Button>
            {lookup.isError && <p role="alert" className="text-destructive">{errorMessage(lookup.error)}</p>}
            {lookup.isSuccess && lookup.data.length === 0 && (
              <p role="status">找不到這個 Email 的本站帳號，請確認 Email 或請對方先登入本站。</p>
            )}
            {lookup.isSuccess && lookup.data.length > 0 && (
              <fieldset className="space-y-2">
                <legend className="text-sm font-medium">確認要加入的帳號（相同 Email 不會合併身分）</legend>
                {lookup.data.map((candidate) => {
                  const alreadyMember = !!candidate.role || members.data?.some((member) => member.id === candidate.id);
                  return (
                    <label key={candidate.id} className="flex items-start gap-2 rounded-md border p-2 text-sm">
                      <input type="radio" name="new-member" value={candidate.id} checked={userId === candidate.id}
                        disabled={alreadyMember || mutation.isPending} onChange={() => setUserId(candidate.id)} />
                      <span className="min-w-0 break-all">
                        {candidate.displayName} · {candidate.email} · {candidate.identityKind === "local" ? "本機帳號" : "OIDC 帳號"}
                        {alreadyMember ? " · 已是成員" : ""}
                        <code className="block text-xs text-muted-foreground">{candidate.id}</code>
                      </span>
                    </label>
                  );
                })}
              </fieldset>
            )}
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
            <Button type="button" disabled={mutation.isPending || lookup.isPending || !userId} onClick={() => mutation.mutate(
              { suffix: "/members", body: { userId, role } },
              { onSuccess: () => { setUserId(""); setEmail(""); lookup.reset(); } },
            )}>加入成員</Button>
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
