import { Link, useNavigate, useParams } from "react-router";
import { toast } from "sonner";
import { ApiError } from "@/shared/api/client";
import { errorMessage } from "@/shared/api/errors";
import { ConfirmDeleteButton } from "@/shared/components/ConfirmDeleteButton";
import { Button } from "@/shared/ui/button";
import { Skeleton } from "@/shared/ui/skeleton";
import { useDeleteVendor, useVendor } from "./hooks";
import { MenuSection } from "./MenuSection";
import { VendorInfoForm } from "./VendorInfoForm";
import { VendorImageSection } from "./VendorImageSection";
import { ScopeBadge } from "@/features/sharing-groups/ScopeSelect";

export function VendorDetailPage() {
  const { id = "" } = useParams();
  const vendor = useVendor(id);
  const del = useDeleteVendor(id);
  const navigate = useNavigate();

  if (vendor.isPending) {
    return (
      <div className="space-y-3" aria-busy="true">
        <Skeleton className="h-8 w-48" />
        <Skeleton className="h-40 w-full" />
      </div>
    );
  }
  if (vendor.isError) {
    const notFound = vendor.error instanceof ApiError && vendor.error.status === 404;
    return (
      <div className="space-y-2">
        <p role="alert" className="text-destructive">
          {errorMessage(vendor.error)}
        </p>
        {notFound ? (
          <Link to="/vendors" className="text-primary underline">
            回到店家列表
          </Link>
        ) : (
          <Button variant="outline" onClick={() => vendor.refetch()}>
            重試
          </Button>
        )}
      </div>
    );
  }

  const v = vendor.data;
  return (
    <div className="space-y-6">
      <Link to={v.sharingGroupId ? `/vendors?sharingGroupId=${encodeURIComponent(v.sharingGroupId)}` : "/vendors"} className="text-sm text-muted-foreground hover:underline">
        ← 店家
      </Link>
      <div className="flex flex-wrap items-center gap-2">
        <h1 className="min-w-0 break-words text-2xl font-bold">{v.name}</h1>
        {v.canManage !== false && <div className="ml-auto">
          <ConfirmDeleteButton
            label="刪除店家"
            showLabel
            pending={del.isPending}
            onConfirm={() =>
              del.mutate(undefined, {
                onSuccess: () => {
                  toast.success("已刪除店家");
                  navigate("/vendors", { replace: true });
                },
                onError: (err) => toast.error(errorMessage(err)),
              })
            }
          />
        </div>}
      </div>
      <ScopeBadge sharingGroupId={v.sharingGroupId} canManage={v.canManage} />
      {v.canManage !== false ? <VendorInfoForm key={JSON.stringify([v.id, v.name, v.phone, v.address, v.note])} vendor={v} /> : <dl className="space-y-2">
        <div><dt className="text-sm text-muted-foreground">電話</dt><dd>{v.phone || "未填"}</dd></div>
        <div><dt className="text-sm text-muted-foreground">地址</dt><dd>{v.address || "未填"}</dd></div>
        <div><dt className="text-sm text-muted-foreground">備註</dt><dd>{v.note || "未填"}</dd></div>
      </dl>}
      <VendorImageSection vendor={v} />
      <MenuSection vendorId={id} items={v.menuItems} canManage={v.canManage !== false} sharingGroupId={v.sharingGroupId ?? undefined} />
    </div>
  );
}
