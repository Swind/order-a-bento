import { Plus, Store } from "lucide-react";
import { useState, type FormEvent } from "react";
import { Link, useNavigate, useSearchParams } from "react-router";
import { toast } from "sonner";
import { ApiError } from "@/shared/api/client";
import { errorMessage, fieldError } from "@/shared/api/errors";
import { ImportButton } from "@/features/ai-import/ImportDialog";
import { FieldError } from "@/shared/components/FieldError";
import { Button } from "@/shared/ui/button";
import { Card, CardContent } from "@/shared/ui/card";
import {
  Dialog,
  DialogContent,
  DialogFooter,
  DialogHeader,
  DialogTitle,
  DialogTrigger,
} from "@/shared/ui/dialog";
import { Input } from "@/shared/ui/input";
import { Label } from "@/shared/ui/label";
import { Skeleton } from "@/shared/ui/skeleton";
import { Textarea } from "@/shared/ui/textarea";
import { useCreateVendor, useVendors } from "./hooks";
import { ScopeBadge, ScopeSelect } from "@/features/sharing-groups/ScopeSelect";
import { useSharingGroups } from "@/features/sharing-groups/hooks";

const FIELDS = ["name", "phone", "address", "note"] as const;

function CreateVendorDialog({ sharingGroupId }: { sharingGroupId?: string }) {
  const [open, setOpen] = useState(false);
  const [v, setV] = useState({ name: "", phone: "", address: "", note: "" });
  const [errors, setErrors] = useState<Record<string, string>>({});
  const [existingId, setExistingId] = useState<string | null>(null);
  const navigate = useNavigate();
  const create = useCreateVendor();
  const set = (k: keyof typeof v) => (e: { target: { value: string } }) =>
    setV((p) => ({ ...p, [k]: e.target.value }));

  function submit(e: FormEvent) {
    e.preventDefault();
    setErrors({});
    setExistingId(null);
    if (!v.name.trim()) {
      setErrors({ name: "請輸入店家名稱" });
      return;
    }
    const opt = (s: string) => s.trim() || undefined;
    create.mutate(
      {
        name: v.name.trim(),
        phone: opt(v.phone),
        address: opt(v.address),
        note: opt(v.note),
        ...(sharingGroupId ? { sharingGroupId } : {}),
      },
      {
        onSuccess: (data) => {
          toast.success("已新增店家");
          setOpen(false);
          setV({ name: "", phone: "", address: "", note: "" });
          navigate(`/vendors/${data.id}`);
        },
        onError: (err) => {
          const f = fieldError(err, FIELDS);
          if (f) setErrors({ [f.field]: f.message });
          else toast.error(errorMessage(err));
          if (err instanceof ApiError && err.code === "vendor_name_taken" && err.existingId) {
            setExistingId(err.existingId);
          }
        },
      },
    );
  }

  return (
    <Dialog open={open} onOpenChange={setOpen}>
      <DialogTrigger render={<Button />}>
        <Plus />
        新增店家
      </DialogTrigger>
      <DialogContent>
        <form onSubmit={submit} className="grid gap-3">
          <DialogHeader>
            <DialogTitle>新增店家</DialogTitle>
            <ScopeBadge sharingGroupId={sharingGroupId} />
            <p className="text-sm text-muted-foreground">建立後無法變更資料範圍。</p>
          </DialogHeader>
          <div className="space-y-1.5">
            <Label htmlFor="vendor-name">店家名稱</Label>
            <Input
              id="vendor-name"
              value={v.name}
              onChange={set("name")}
              aria-invalid={!!errors.name}
              autoComplete="off"
            />
            <FieldError message={errors.name} />
            {existingId && (
              <Link to={`/vendors/${existingId}`} className="text-sm text-primary underline">
                前往既有店家
              </Link>
            )}
          </div>
          <div className="space-y-1.5">
            <Label htmlFor="vendor-phone">電話</Label>
            <Input
              id="vendor-phone"
              type="tel"
              value={v.phone}
              onChange={set("phone")}
              aria-invalid={!!errors.phone}
            />
            <FieldError message={errors.phone} />
          </div>
          <div className="space-y-1.5">
            <Label htmlFor="vendor-address">地址</Label>
            <Input
              id="vendor-address"
              value={v.address}
              onChange={set("address")}
              aria-invalid={!!errors.address}
            />
            <FieldError message={errors.address} />
          </div>
          <div className="space-y-1.5">
            <Label htmlFor="vendor-note">備註</Label>
            <Textarea
              id="vendor-note"
              rows={3}
              value={v.note}
              onChange={set("note")}
              aria-invalid={!!errors.note}
            />
            <FieldError message={errors.note} />
          </div>
          <DialogFooter>
            <Button type="submit" disabled={create.isPending}>
              建立
            </Button>
          </DialogFooter>
        </form>
      </DialogContent>
    </Dialog>
  );
}

export function VendorsPage() {
  const [search, setSearch] = useSearchParams();
  const scope = search.get("sharingGroupId") ?? "";
  const setScope = (value: string) => setSearch(value ? { sharingGroupId: value } : {});
  const groups = useSharingGroups();
  const vendors = useVendors(scope || undefined);
  const canManage = !scope || (!groups.isError && groups.data?.find((group) => group.id === scope)?.role === "admin");
  return (
    <div className="space-y-4">
      <div className="flex items-center justify-between gap-2">
        <h1 className="text-2xl font-bold">店家</h1>
        {canManage && <CreateVendorDialog key={scope} sharingGroupId={scope || undefined} />}
      </div>
      <ScopeSelect value={scope} onChange={setScope} />
      {canManage ? <ImportButton key={scope} label="📷 從傳單建立店家" mode="vendor" sharingGroupId={scope || undefined} /> : <p className="text-sm text-muted-foreground">唯讀：可查看並選用開團，只有群組管理員可修改。</p>}
      {vendors.isPending && (
        <div className="space-y-3" aria-busy="true">
          <Skeleton className="h-16 w-full" />
          <Skeleton className="h-16 w-full" />
        </div>
      )}
      {vendors.isError && (
        <div className="space-y-2">
          <p role="alert" className="text-destructive">
            {errorMessage(vendors.error)}
          </p>
          <Button variant="outline" onClick={() => vendors.refetch()}>
            重試
          </Button>
        </div>
      )}
      {vendors.data?.length === 0 && (
        <div className="flex flex-col items-center gap-2 rounded-xl border border-dashed py-12 text-center text-muted-foreground">
          <Store className="size-8" />
          <p>還沒有店家，按「新增店家」開始建立。</p>
        </div>
      )}
      <ul className="grid gap-3 sm:grid-cols-2">
        {!vendors.isError && vendors.data?.map((v) => (
          <li key={v.id}>
            <Link to={`/vendors/${v.id}`} className="block rounded-xl focus-visible:outline-none focus-visible:ring-3 focus-visible:ring-ring/50">
              <Card className="transition-colors hover:bg-muted/50">
                <CardContent className="flex items-center justify-between gap-3">
                  <div className="min-w-0">
                    <p className="truncate text-base font-medium">{v.name}</p>
                    <p className="text-xs text-muted-foreground">{scope ? `共用群組：${groups.data?.find((group) => group.id === scope)?.name ?? scope}` : "個人"}{v.canManage === false ? " · 唯讀" : ""}</p>
                    <p className="truncate text-sm text-muted-foreground">{v.phone || "未填電話"}</p>
                  </div>
                  <span className="shrink-0 text-sm">{v.itemCount} 項</span>
                </CardContent>
              </Card>
            </Link>
          </li>
        ))}
      </ul>
    </div>
  );
}
