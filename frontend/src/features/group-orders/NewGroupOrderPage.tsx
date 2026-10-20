import { Check } from "lucide-react";
import { useState, type FormEvent } from "react";
import { Link, useNavigate } from "react-router";
import { toast } from "sonner";
import { useAccessibleDirectory } from "@/features/sharing-groups/hooks";
import { ScopeSelect } from "@/features/sharing-groups/ScopeSelect";
import type { RosterSummary, VendorSummary } from "@/shared/api/types";
import { DeliveryAddressSelect } from "@/features/delivery-addresses/DeliveryAddressSelect";
import { ApiError } from "@/shared/api/client";
import { errorMessage, fieldError } from "@/shared/api/errors";
import { FieldError } from "@/shared/components/FieldError";
import { formatMD, parseLocalInput, toLocalInputValue } from "@/shared/lib/datetime";
import { cn } from "@/shared/lib/utils";
import { Button } from "@/shared/ui/button";
import { Input } from "@/shared/ui/input";
import { Label } from "@/shared/ui/label";
import { Skeleton } from "@/shared/ui/skeleton";
import { Textarea } from "@/shared/ui/textarea";
import { useCreateGroupOrder } from "./hooks";

const STEPS = ["選名單", "選店家", "設定"] as const;
const TITLE_MAX = 50;
const NOTE_MAX = 200;

function StepIndicator({ step }: { step: number }) {
  return (
    <ol className="flex items-center gap-2" aria-label="開團步驟">
      {STEPS.map((label, i) => (
        <li
          key={label}
          aria-current={i === step ? "step" : undefined}
          className={cn(
            "flex flex-1 items-center gap-1.5 text-sm",
            i === step ? "font-semibold text-foreground" : "text-muted-foreground",
          )}
        >
          <span
            className={cn(
              "flex size-6 shrink-0 items-center justify-center rounded-full border text-xs",
              i < step && "border-primary bg-primary text-primary-foreground",
              i === step && "border-primary",
            )}
          >
            {i < step ? <Check className="size-3.5" /> : i + 1}
          </span>
          <span className="truncate">{label}</span>
        </li>
      ))}
    </ol>
  );
}

function ChoiceRow({
  selected,
  disabled,
  title,
  meta,
  hint,
  sourceLabel,
  onSelect,
}: {
  selected: boolean;
  disabled: boolean;
  title: string;
  meta: string;
  hint?: string;
  sourceLabel?: string;
  onSelect: () => void;
}) {
  return (
    <button
      type="button"
      aria-pressed={selected}
      disabled={disabled}
      onClick={onSelect}
      className={cn(
        "flex min-h-14 w-full items-center justify-between gap-3 rounded-xl border px-4 py-2 text-left transition-colors focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-ring",
        selected ? "border-primary bg-primary/5 ring-2 ring-primary/40" : "hover:bg-muted/50",
        disabled && "cursor-not-allowed opacity-50",
      )}
    >
      <span className="min-w-0">
        <span className="block truncate text-base font-medium">{title}</span>
        {sourceLabel && <span className="block text-xs text-muted-foreground">{sourceLabel}</span>}
        {hint && <span className="block text-sm text-muted-foreground">{hint}</span>}
      </span>
      <span className="shrink-0 text-sm">{meta}</span>
    </button>
  );
}

function ListState({
  query,
  emptyText,
  emptyLink,
}: {
  query: { isPending: boolean; isError: boolean; error: unknown; refetch: () => unknown; data?: unknown[] };
  emptyText: string;
  emptyLink: { to: string; label: string };
}) {
  if (query.isPending)
    return (
      <div className="space-y-3" aria-busy="true">
        <Skeleton className="h-14 w-full" />
        <Skeleton className="h-14 w-full" />
      </div>
    );
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
  if (query.data?.length === 0)
    return (
      <p className="rounded-xl border border-dashed py-8 text-center text-muted-foreground">
        {emptyText}{" "}
        <Link to={emptyLink.to} className="underline">
          {emptyLink.label}
        </Link>
      </p>
    );
  return null;
}

export function NewGroupOrderPage() {
  const navigate = useNavigate();
  const rosters = useAccessibleDirectory<RosterSummary>("rosters");
  const vendors = useAccessibleDirectory<VendorSummary>("vendors");
  const create = useCreateGroupOrder();

  const [step, setStep] = useState(0);
  const [sharingGroupId, setSharingGroupId] = useState("");
  const [rosterId, setRosterId] = useState<string | null>(null);
  const [vendorId, setVendorId] = useState<string | null>(null);
  const [titleOverride, setTitleOverride] = useState<string | null>(null);
  const [deadline, setDeadline] = useState("");
  const [note, setNote] = useState("");
  const [deliveryAddressId, setDeliveryAddressId] = useState("");
  const [errors, setErrors] = useState<{ title?: string; deadlineAt?: string }>({});

  const vendor = vendors.data?.find((v) => v.id === vendorId);
  const defaultTitle = `${formatMD(new Date())} 午餐${vendor ? ` · ${vendor.name}` : ""}`;
  const title = titleOverride ?? defaultTitle;

  function addMinutes(min: number) {
    setDeadline(toLocalInputValue(new Date(Date.now() + min * 60_000)));
    setErrors((e) => ({ ...e, deadlineAt: undefined }));
  }
  function todayAt11() {
    const d = new Date();
    d.setHours(11, 0, 0, 0);
    setDeadline(toLocalInputValue(d));
    setErrors((e) => ({ ...e, deadlineAt: undefined }));
  }

  function submit(e: FormEvent) {
    e.preventDefault();
    if (!rosterId || !vendorId) return;
    const next: typeof errors = {};
    const trimmed = title.trim();
    if (!trimmed) next.title = "請輸入標題";
    else if (trimmed.length > TITLE_MAX) next.title = `標題最多 ${TITLE_MAX} 字`;
    let deadlineAt: string | undefined;
    if (deadline) {
      const d = parseLocalInput(deadline);
      if (!d) next.deadlineAt = "截止時間格式不正確";
      else if (d.getTime() <= Date.now()) next.deadlineAt = "截止時間必須是未來的時間";
      else deadlineAt = d.toISOString();
    }
    setErrors(next);
    if (next.title || next.deadlineAt) return;
    create.mutate(
      {
        title: trimmed,
        rosterId,
        vendorId,
        ...(sharingGroupId ? { sharingGroupId } : {}),
        ...(deliveryAddressId ? { deliveryAddressId } : {}),
        ...(deadlineAt ? { deadlineAt } : {}),
        ...(note.trim() ? { note: note.trim() } : {}),
      },
      {
        onSuccess: (data) => {
          toast.success("已開團");
          navigate(`/group-orders/${data.id}`);
        },
        onError: (err) => {
          const f = fieldError(err, ["title", "deadlineAt"]);
          if (f) {
            setErrors({ [f.field]: f.message });
          } else if (err instanceof ApiError && (err.status === 403 || err.status === 404)) {
            toast.error("共用群組或選用資料已無法存取，請確認開團歸屬並重新選擇資料");
            setRosterId(null);
            setVendorId(null);
            setDeliveryAddressId("");
            setStep(0);
            rosters.refetch();
            vendors.refetch();
          } else {
            toast.error(errorMessage(err));
          }
        },
      },
    );
  }

  return (
    <div className="mx-auto max-w-xl space-y-5">
      <div className="space-y-1"><h1 className="text-2xl font-bold tracking-tight">開團</h1><p className="text-sm text-muted-foreground">三個步驟，揪大家一起訂午餐。</p></div>
      <StepIndicator step={step} />
      <div className="space-y-2 rounded-xl border p-3">
        <ScopeSelect ownership value={sharingGroupId} onChange={setSharingGroupId} />
        <p className="text-sm text-muted-foreground">歸屬與名單、店家的來源無關，開團後無法變更。群組成員可查看，管理員與仍在群組內的開團者可管理。</p>
      </div>

      {step === 0 && (
        <section className="space-y-3" aria-label="選名單">
          <h2 className="text-lg font-semibold">選擇名單</h2>
          <ListState
            query={rosters}
            emptyText="還沒有名單，"
            emptyLink={{ to: "/rosters", label: "前往建立名單" }}
          />
          <ul className="space-y-2">
            {rosters.data?.map((r) => (
              <li key={r.id}>
                <ChoiceRow
                  selected={rosterId === r.id}
                  disabled={r.memberCount === 0}
                  title={r.name}
                  sourceLabel={r.sourceLabel}
                  meta={`${r.memberCount} 人`}
                  hint={r.memberCount === 0 ? "此名單沒有成員，請先新增成員" : undefined}
                  onSelect={() => setRosterId(r.id)}
                />
              </li>
            ))}
          </ul>
        </section>
      )}

      {step === 1 && (
        <section className="space-y-3" aria-label="選店家">
          <h2 className="text-lg font-semibold">選擇店家</h2>
          <ListState
            query={vendors}
            emptyText="還沒有店家，"
            emptyLink={{ to: "/vendors", label: "前往建立店家" }}
          />
          <ul className="space-y-2">
            {vendors.data?.map((v) => (
              <li key={v.id}>
                <ChoiceRow
                  selected={vendorId === v.id}
                  disabled={v.itemCount === 0}
                  title={v.name}
                  sourceLabel={v.sourceLabel}
                  meta={`${v.itemCount} 個品項`}
                  hint={v.itemCount === 0 ? "此店家沒有品項，請先新增菜單" : undefined}
                  onSelect={() => setVendorId(v.id)}
                />
              </li>
            ))}
          </ul>
        </section>
      )}

      {step === 2 && (
        <form onSubmit={submit} className="space-y-4 rounded-xl border bg-card p-5" aria-label="設定">
          <h2 className="text-lg font-semibold">設定</h2>
          <DeliveryAddressSelect value={deliveryAddressId} onChange={setDeliveryAddressId} />
          <div className="space-y-1.5">
            <Label htmlFor="go-title">標題</Label>
            <Input
              id="go-title"
              className="h-11"
              value={title}
              maxLength={TITLE_MAX}
              onChange={(e) => setTitleOverride(e.target.value)}
              aria-invalid={!!errors.title}
              autoComplete="off"
            />
            <FieldError message={errors.title} />
          </div>
          <div className="space-y-1.5">
            <Label htmlFor="go-deadline">截止時間（選填）</Label>
            <Input
              id="go-deadline"
              type="datetime-local"
              className="h-11"
              value={deadline}
              onChange={(e) => {
                setDeadline(e.target.value);
                setErrors((x) => ({ ...x, deadlineAt: undefined }));
              }}
              aria-invalid={!!errors.deadlineAt}
            />
            <div className="flex flex-wrap gap-2">
              <Button type="button" variant="outline" className="h-11" onClick={todayAt11}>
                今天 11:00
              </Button>
              <Button type="button" variant="outline" className="h-11" onClick={() => addMinutes(30)}>
                30 分鐘後
              </Button>
              <Button type="button" variant="outline" className="h-11" onClick={() => addMinutes(60)}>
                1 小時後
              </Button>
              {deadline && (
                <Button type="button" variant="ghost" className="h-11" onClick={() => setDeadline("")}>
                  清除
                </Button>
              )}
            </div>
            <FieldError message={errors.deadlineAt} />
          </div>
          <div className="space-y-1.5">
            <Label htmlFor="go-note">備註（選填）</Label>
            <Textarea
              id="go-note"
              rows={3}
              value={note}
              maxLength={NOTE_MAX}
              onChange={(e) => setNote(e.target.value)}
            />
          </div>
          <Button type="submit" className="h-12 w-full text-base" disabled={create.isPending}>
            {create.isPending ? "開團中…" : "開團"}
          </Button>
        </form>
      )}

      <div className="flex justify-between gap-3">
        {step > 0 ? (
          <Button type="button" variant="outline" className="h-11" onClick={() => setStep(step - 1)}>
            上一步
          </Button>
        ) : (
          <span />
        )}
        {step < 2 && (
          <Button
            type="button"
            className="h-11"
            disabled={step === 0 ? !rosterId : !vendorId}
            onClick={() => setStep(step + 1)}
          >
            下一步
          </Button>
        )}
      </div>
    </div>
  );
}
