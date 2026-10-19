import { useState, type FormEvent } from "react";
import { toast } from "sonner";
import { ImportButton } from "@/features/ai-import/ImportDialog";
import { errorMessage } from "@/shared/api/errors";
import type { DeliveryAddress } from "@/shared/api/types";
import { ConfirmActionButton } from "@/shared/components/ConfirmActionButton";
import { Button } from "@/shared/ui/button";
import { Dialog, DialogContent, DialogDescription, DialogHeader, DialogTitle } from "@/shared/ui/dialog";
import { Input } from "@/shared/ui/input";
import { Label } from "@/shared/ui/label";
import { Skeleton } from "@/shared/ui/skeleton";
import { Textarea } from "@/shared/ui/textarea";
import { useDeleteDeliveryAddress, useDeliveryAddresses, useSaveDeliveryAddress } from "./hooks";

function DeliveryAddressForm({ initial, onSaved }: { initial?: DeliveryAddress; onSaved: () => void }) {
  const [name, setName] = useState(initial?.name ?? "");
  const [address, setAddress] = useState(initial?.address ?? "");
  const save = useSaveDeliveryAddress();

  function submit(e: FormEvent) {
    e.preventDefault();
    if (!name.trim() || !address.trim()) return;
    save.mutate({ id: initial?.id, name: name.trim(), address: address.trim() }, {
      onSuccess: () => { toast.success("已儲存送餐地址"); onSaved(); },
      onError: (err) => toast.error(errorMessage(err)),
    });
  }

  return (
    <form onSubmit={submit} className="space-y-4">
      <div className="space-y-1.5">
        <Label htmlFor="delivery-name">地點名稱</Label>
        <Input id="delivery-name" className="h-11" value={name} onChange={(e) => setName(e.target.value)} maxLength={50} required placeholder="例如：台北教室" />
      </div>
      <div className="space-y-1.5">
        <Label htmlFor="delivery-address">送餐地址</Label>
        <Textarea id="delivery-address" value={address} onChange={(e) => setAddress(e.target.value)} maxLength={200} required autoComplete="street-address" placeholder="請填寫完整地址、樓層及教室號碼" />
      </div>
      <Button type="submit" className="h-11" disabled={save.isPending || !name.trim() || !address.trim()}>儲存地址</Button>
    </form>
  );
}

export function DeliveryAddressesPage() {
  const query = useDeliveryAddresses();
  const remove = useDeleteDeliveryAddress();
  const [editing, setEditing] = useState<DeliveryAddress | "new" | null>(null);

  return (
    <div className="space-y-4">
      <div className="flex flex-wrap items-center justify-between gap-2">
        <h1 className="text-2xl font-bold">送餐地址</h1>
        <Button className="h-11" onClick={() => setEditing("new")}>新增地址</Button>
      </div>
      <p className="text-sm text-muted-foreground">儲存常用地點，開團時選擇。修改或刪除地址不會影響已建立的團。</p>
      <ImportButton label="📷 從照片匯入地址" mode="deliveryAddress" />
      {query.isPending && <Skeleton className="h-24 w-full" />}
      {query.isError && <div className="space-y-2"><p role="alert">{errorMessage(query.error)}</p><Button variant="outline" onClick={() => query.refetch()}>重試</Button></div>}
      {query.data?.length === 0 && <p className="rounded-xl border border-dashed p-8 text-center text-muted-foreground">還沒有送餐地址，可手動新增或從照片匯入。</p>}
      <ul className="space-y-3" aria-label="送餐地址清單">
        {query.data?.map((a) => (
          <li key={a.id} className="space-y-2 rounded-xl border p-4">
            <h2 className="break-words font-semibold">{a.name}</h2>
            <p className="whitespace-pre-wrap break-words">{a.address}</p>
            <div className="flex gap-2">
              <Button variant="outline" className="h-11" aria-label={`編輯 ${a.name}`} onClick={() => setEditing(a)}>編輯</Button>
              <ConfirmActionButton label={`刪除 ${a.name}`} message="確定刪除這個常用送餐地址？已建立的團仍會保留原地址。" pending={remove.isPending} onConfirm={() => remove.mutate(a.id, { onError: (e) => toast.error(errorMessage(e)) })} />
            </div>
          </li>
        ))}
      </ul>
      <Dialog open={editing !== null} onOpenChange={(open) => { if (!open) setEditing(null); }}>
        <DialogContent>
          <DialogHeader><DialogTitle>{editing === "new" ? "新增送餐地址" : "編輯送餐地址"}</DialogTitle><DialogDescription>請確認完整地址及樓層，避免送錯地點。</DialogDescription></DialogHeader>
          {editing && <DeliveryAddressForm key={editing === "new" ? "new" : editing.id} initial={editing === "new" ? undefined : editing} onSaved={() => setEditing(null)} />}
        </DialogContent>
      </Dialog>
    </div>
  );
}
