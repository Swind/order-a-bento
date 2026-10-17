import { zodResolver } from "@hookform/resolvers/zod";
import { useForm } from "react-hook-form";
import { Link } from "react-router";
import { toast } from "sonner";
import { z } from "zod";
import { ApiError } from "@/shared/api/client";
import { errorMessage, fieldError } from "@/shared/api/errors";
import type { VendorDetail } from "@/shared/api/types";
import { FieldError } from "@/shared/components/FieldError";
import { Button } from "@/shared/ui/button";
import { Input } from "@/shared/ui/input";
import { Label } from "@/shared/ui/label";
import { Textarea } from "@/shared/ui/textarea";
import { usePatchVendor, type VendorFields } from "./hooks";

const schema = z.object({
  name: z.string().trim().min(1, "請輸入店家名稱").max(100, "店家名稱太長"),
  phone: z.string().trim().max(50, "電話太長"),
  address: z.string().trim().max(200, "地址太長"),
  note: z.string().trim().max(500, "備註太長"),
});
type Values = z.infer<typeof schema>;

const toValues = (v: VendorDetail): Values => ({
  name: v.name,
  phone: v.phone ?? "",
  address: v.address ?? "",
  note: v.note ?? "",
});

/** Only changed fields; a cleared optional field becomes `null`. */
export function diffVendor(original: VendorDetail, next: Values): Partial<VendorFields> {
  const orig = toValues(original);
  const out: Partial<VendorFields> = {};
  if (next.name !== orig.name) out.name = next.name;
  for (const k of ["phone", "address", "note"] as const) {
    if (next[k] !== orig[k]) out[k] = next[k] === "" ? null : next[k];
  }
  return out;
}

export function VendorInfoForm({ vendor }: { vendor: VendorDetail }) {
  const patch = usePatchVendor(vendor.id);
  const form = useForm<Values>({ resolver: zodResolver(schema), defaultValues: toValues(vendor) });
  const { register, handleSubmit, setError, reset, formState } = form;
  const errors = formState.errors;
  const existing = (patch.error instanceof ApiError && patch.error.existingId) || null;

  const onSubmit = handleSubmit((values) => {
    const body = diffVendor(vendor, values);
    if (Object.keys(body).length === 0) {
      toast.info("沒有變更");
      return;
    }
    patch.mutate(body, {
      onSuccess: (saved) => {
        toast.success("已儲存");
        reset(toValues(saved));
      },
      onError: (err) => {
        const f = fieldError(err, ["name", "phone", "address", "note"]);
        if (f) setError(f.field as keyof Values, { message: f.message });
        else toast.error(errorMessage(err));
      },
    });
  });

  return (
    <form onSubmit={onSubmit} className="space-y-3" aria-label="店家資料" noValidate>
      <div className="space-y-1.5">
        <Label htmlFor="v-name">店家名稱</Label>
        <Input id="v-name" {...register("name")} aria-invalid={!!errors.name} autoComplete="off" />
        <FieldError message={errors.name?.message} />
        {existing && (
          <Link to={`/vendors/${existing}`} className="text-sm text-primary underline">
            前往既有店家
          </Link>
        )}
      </div>
      <div className="grid gap-3 sm:grid-cols-2">
        <div className="space-y-1.5">
          <Label htmlFor="v-phone">電話</Label>
          <Input id="v-phone" type="tel" {...register("phone")} aria-invalid={!!errors.phone} />
          <FieldError message={errors.phone?.message} />
        </div>
        <div className="space-y-1.5">
          <Label htmlFor="v-address">地址</Label>
          <Input id="v-address" {...register("address")} aria-invalid={!!errors.address} />
          <FieldError message={errors.address?.message} />
        </div>
      </div>
      <div className="space-y-1.5">
        <Label htmlFor="v-note">備註</Label>
        <Textarea id="v-note" rows={3} {...register("note")} aria-invalid={!!errors.note} />
        <FieldError message={errors.note?.message} />
      </div>
      <Button type="submit" disabled={patch.isPending || !formState.isDirty}>
        儲存
      </Button>
    </form>
  );
}
