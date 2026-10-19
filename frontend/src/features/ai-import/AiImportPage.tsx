import { useState } from "react";
import { Link, useNavigate, useParams } from "react-router";
import { toast } from "sonner";
import { useAuthConfig } from "@/features/auth/hooks";
import { useRoster } from "@/features/rosters/hooks";
import { DeliveryAddressImportConfirmation } from "@/features/delivery-addresses/ImportConfirmation";
import { ScopeBadge } from "@/features/sharing-groups/ScopeSelect";
import { ApiError } from "@/shared/api/client";
import { errorMessage } from "@/shared/api/errors";
import type { ImportRun } from "@/shared/api/types";
import { ConfirmActionButton } from "@/shared/components/ConfirmActionButton";
import { Badge } from "@/shared/ui/badge";
import { Button } from "@/shared/ui/button";
import { Input } from "@/shared/ui/input";
import { Skeleton } from "@/shared/ui/skeleton";
import {
  InfoSection,
  MemberRowView,
  MenuRowView,
  UnchangedList,
} from "./ConfirmRows";
import { useApplyImport, useDiscardImport, useImportRun } from "./hooks";
import {
  buildApplyBody,
  countChanges,
  hasBlockingIssues,
  infoErrors,
  initState,
  memberIssues,
  menuIssues,
  rowKey,
  mapServerErrors,
  refLink,
  targetNameIssue,
  type AcceptedMap,
  type InfoRow,
  type MappedErrors,
  type MemberRow,
  type MenuRow,
  type TargetState,
} from "./lib";

/** Where 「重新上傳」 / 「放棄」 should send the user. */
function originPath(run: ImportRun): string {
  if (run.mode === "deliveryAddress") return "/delivery-addresses";
  if (run.targetRosterId) return `/rosters/${run.targetRosterId}`;
  if (run.targetVendorId) return `/vendors/${run.targetVendorId}`;
  const path = run.mode === "vendor" ? "/vendors" : "/rosters";
  return run.sharingGroupId ? `${path}?sharingGroupId=${encodeURIComponent(run.sharingGroupId)}` : path;
}

function DemoBadge() {
  return <Badge variant="secondary">示範資料</Badge>;
}

function RosterTitle({
  target,
  onName,
}: {
  target: Extract<TargetState, { type: "roster" }>;
  onName: (name: string) => void;
}) {
  const existingId = target.ref.kind === "existing" ? target.ref.existingId : "";
  const roster = useRoster(existingId);
  if (target.ref.kind === "new") {
    const issue = targetNameIssue(target.ref);
    return (
      <div className="space-y-1">
        <label htmlFor="new-roster-name" className="text-sm font-medium">
          新名單：
        </label>
        <Input
          id="new-roster-name"
          className="h-11"
          value={target.ref.name}
          aria-invalid={!!issue}
          onChange={(e) => onName(e.target.value)}
        />
        {issue && <p className="text-sm text-destructive">⚠ {issue}</p>}
      </div>
    );
  }
  return (
    <h2 className="text-lg font-semibold">
      名單：
      <Link to={`/rosters/${existingId}`} className="text-primary underline">
        {roster.data?.name ?? "查看名單"}
      </Link>
    </h2>
  );
}

function ConfirmView({ run, demo }: { run: ImportRun; demo: boolean }) {
  const navigate = useNavigate();
  const apply = useApplyImport(run.id);
  const discard = useDiscardImport(run.id);
  const [state, setState] = useState<TargetState[]>(() => initState(run.proposal));
  const [serverErrors, setServerErrors] = useState<MappedErrors>({ rows: {}, unmapped: [] });

  const n = countChanges(state);
  const blocked = hasBlockingIssues(state);

  function clearErr(key: string) {
    if (!serverErrors.rows[key]) return;
    const { [key]: _gone, ...rest } = serverErrors.rows;
    setServerErrors({ ...serverErrors, rows: rest });
  }
  function update(ti: number, fn: (t: TargetState) => TargetState) {
    setState((s) => s.map((t, i) => (i === ti ? fn(t) : t)));
  }
  function patchMember(ti: number, i: number, patch: Partial<MemberRow>) {
    clearErr(rowKey(ti, "members", i));
    update(ti, (t) =>
      t.type === "roster" ? { ...t, members: t.members.map((m, j) => (j === i ? { ...m, ...patch } : m)) } : t,
    );
  }
  function patchMenu(ti: number, i: number, patch: Partial<MenuRow>) {
    clearErr(rowKey(ti, "menu", i));
    update(ti, (t) =>
      t.type === "vendor" ? { ...t, menu: t.menu.map((m, j) => (j === i ? { ...m, ...patch } : m)) } : t,
    );
  }
  function patchInfo(ti: number, patch: Partial<InfoRow>) {
    clearErr(rowKey(ti, "info", 0));
    update(ti, (t) => (t.type === "vendor" ? { ...t, info: { ...t.info, ...patch } } : t));
  }

  function submit() {
    const { accepted, map } = buildApplyBody(state);
    setServerErrors({ rows: {}, unmapped: [] });
    apply.mutate(accepted, {
      onSuccess: (res) => {
        toast.success("已套用");
        const rosterId = res.rosterIds[0];
        const vendorId = res.vendorIds[0];
        navigate(rosterId ? `/rosters/${rosterId}` : vendorId ? `/vendors/${vendorId}` : originPath(run));
      },
      onError: (err) => {
        if (err instanceof ApiError && err.code === "invalid_changes" && err.errors?.length) {
          setServerErrors(mapServerErrors(err.errors, map as AcceptedMap[]));
        }
        toast.error(errorMessage(err));
      },
    });
  }

  function doDiscard() {
    discard.mutate(undefined, {
      onSuccess: () => {
        toast.success("已放棄");
        navigate(originPath(run));
      },
      onError: (err) => toast.error(errorMessage(err)),
    });
  }

  return (
    <div className="space-y-5">
      <div className="space-y-2">
        <div className="flex flex-wrap items-center gap-2">
          <h1 className="text-2xl font-bold">AI 匯入確認</h1>
          {demo && <DemoBadge />}
        </div>
        <ScopeBadge sharingGroupId={run.sharingGroupId} />
        {run.summary && (
          <p className="rounded-lg bg-muted p-3 text-sm">
            <span className="font-medium">AI 摘要：</span>
            {run.summary}
          </p>
        )}
        {run.proposal.warnings.length > 0 && (
          <div role="alert" className="space-y-1 rounded-lg border border-yellow-500 bg-yellow-500/10 p-3 text-sm">
            {run.proposal.warnings.map((w, i) => (
              <p key={i}>⚠ {w}</p>
            ))}
          </div>
        )}
        {serverErrors.unmapped.length > 0 && (
          <div role="alert" className="space-y-1 rounded-lg border border-destructive p-3 text-sm text-destructive">
            {serverErrors.unmapped.map((m, i) => (
              <p key={i}>{m}</p>
            ))}
          </div>
        )}
      </div>

      {state.length === 0 && <p className="text-muted-foreground">AI 沒有找到可匯入的內容。</p>}

      {state.map((t, ti) => {
        if (t.type === "roster") {
          const unchanged = t.members.filter((m) => m.action === "unchanged").map((m) => m.name);
          const targetErr = serverErrors.rows[rowKey(ti, "target", 0)];
          return (
            <section key={ti} className="space-y-3" aria-label="名單變更">
              <RosterTitle
                target={t}
                onName={(name) => {
                  clearErr(rowKey(ti, "target", 0));
                  update(ti, (x) => (x.type === "roster" ? { ...x, ref: { kind: "new", name } } : x));
                }}
              />
              {targetErr?.map((e, i) => (
                <p key={i} role="alert" className="text-sm text-destructive">⚠ {e.message}</p>
              ))}
              <ul className="space-y-2">
                {t.members.map((m, i) =>
                  m.action === "unchanged" ? null : (
                    <MemberRowView
                      key={i}
                      row={m}
                      issues={memberIssues(m, t.members)}
                      serverErr={serverErrors.rows[rowKey(ti, "members", i)]}
                      onChange={(p) => patchMember(ti, i, p)}
                    />
                  ),
                )}
              </ul>
              <UnchangedList names={unchanged} />
            </section>
          );
        }
        const isNew = t.ref.kind === "new";
        const unchanged = t.menu.filter((m) => m.action === "unchanged").map((m) => m.name);
        const vendorName = isNew ? t.info.name : (t.info.before?.name ?? t.info.name);
        const targetErr = serverErrors.rows[rowKey(ti, "target", 0)];
        return (
          <section key={ti} className="space-y-3" aria-label="店家變更">
            <h2 className="text-lg font-semibold">
              {isNew ? (
                `新店家：${vendorName || "（未命名）"}`
              ) : (
                <>
                  店家：
                  <Link to={refLink(t.ref, "vendor") ?? "#"} className="text-primary underline">
                    {vendorName}
                  </Link>
                </>
              )}
            </h2>
            {targetErr?.map((e, i) => (
              <p key={i} role="alert" className="text-sm text-destructive">⚠ {e.message}</p>
            ))}
            <InfoSection
              info={t.info}
              isNew={isNew}
              errors={infoErrors(t.info, isNew)}
              serverErr={serverErrors.rows[rowKey(ti, "info", 0)]}
              onChange={(p) => patchInfo(ti, p)}
            />
            <h3 className="text-sm font-medium text-muted-foreground">菜單</h3>
            <ul className="space-y-2">
              {t.menu.map((m, i) =>
                m.action === "unchanged" ? null : (
                  <MenuRowView
                    key={i}
                    row={m}
                    issues={menuIssues(m, t.menu)}
                    serverErr={serverErrors.rows[rowKey(ti, "menu", i)]}
                    onChange={(p) => patchMenu(ti, i, p)}
                  />
                ),
              )}
            </ul>
            <UnchangedList names={unchanged} />
          </section>
        );
      })}

      <div className="sticky bottom-0 z-10 -mx-4 flex flex-wrap items-center justify-end gap-2 border-t bg-background px-4 pt-3 pb-[max(0.75rem,env(safe-area-inset-bottom))]">
        <ConfirmActionButton
          label="放棄"
          message="放棄後這次辨識結果會被丟棄，不會寫入任何資料。"
          pending={discard.isPending}
          onConfirm={doDiscard}
        />
        <Button className="h-11" disabled={n === 0 || blocked || apply.isPending} onClick={submit}>
          套用 {n} 筆變更
        </Button>
      </div>
    </div>
  );
}

function Finished({ run }: { run: ImportRun }) {
  const links: { to: string; label: string }[] = [];
  for (const t of run.proposal.targets) {
    if (t.type === "roster" && t.roster.kind === "existing")
      links.push({ to: `/rosters/${t.roster.existingId}`, label: "查看名單" });
    if (t.type === "vendor" && t.vendor.kind === "existing")
      links.push({ to: `/vendors/${t.vendor.existingId}`, label: "查看店家" });
  }
  if (links.length === 0 && run.status === "applied") {
    const t = run.proposal.targets[0];
    links.push({
      to: originPath(run),
      label: run.mode === "deliveryAddress" ? "前往送餐地址" : t?.type === "vendor" ? "前往店家" : "前往名單",
    });
  }
  return (
    <div className="space-y-3">
      <h1 className="text-2xl font-bold">{run.status === "applied" ? "已套用" : "已放棄"}</h1>
      <p className="text-muted-foreground">
        {run.status === "applied" ? "這次 AI 匯入的變更已經寫入。" : "這次 AI 匯入的結果已被丟棄，沒有寫入任何資料。"}
      </p>
      <div className="flex flex-wrap gap-2">
        {links.map((l) => (
          <Button key={l.to + l.label} className="h-11" variant="outline" render={<Link to={l.to} />}>
            {l.label}
          </Button>
        ))}
        {run.status === "discarded" && (
          <Button className="h-11" variant="outline" render={<Link to={originPath(run)} />}>
            返回
          </Button>
        )}
      </div>
    </div>
  );
}

export function AiImportPage() {
  const { runId = "" } = useParams();
  const run = useImportRun(runId);
  const config = useAuthConfig();

  if (run.isPending) {
    return (
      <div className="space-y-3" aria-busy="true">
        <Skeleton className="h-8 w-48" />
        <Skeleton className="h-24 w-full" />
        <Skeleton className="h-24 w-full" />
      </div>
    );
  }
  if (run.isError) {
    return (
      <div className="space-y-2">
        <p role="alert" className="text-destructive">{errorMessage(run.error)}</p>
        <Button variant="outline" className="h-11" onClick={() => run.refetch()}>
          重試
        </Button>
      </div>
    );
  }
  const r = run.data;
  const demo = !!config.data?.aiDemo || /demo|fake/i.test(r.model);
  switch (r.status) {
    case "proposed":
      if (r.mode === "deliveryAddress") return <DeliveryAddressImportConfirmation key={r.id} run={r} demo={demo} />;
      return <ConfirmView run={r} demo={demo} />;
    case "applied":
    case "discarded":
      return <Finished run={r} />;
    case "failed":
      return (
        <div className="space-y-3">
          <h1 className="text-2xl font-bold">辨識失敗</h1>
          <p role="alert" className="text-destructive">{r.error ?? "AI 無法完成這次辨識。"}</p>
          <Button className="h-11" render={<Link to={originPath(r)} />}>
            重新上傳
          </Button>
        </div>
      );
    default:
      return (
        <div className="space-y-3">
          <h1 className="text-2xl font-bold">辨識進行中…</h1>
          <Button variant="outline" className="h-11" onClick={() => run.refetch()}>
            重新整理
          </Button>
        </div>
      );
  }
}
