import { Link } from "react-router";
import { errorMessage } from "@/shared/api/errors";
import { Button } from "@/shared/ui/button";
import { Label } from "@/shared/ui/label";
import { useDeliveryAddresses } from "./hooks";

export function DeliveryAddressSelect({ value, onChange, disabled = false }: {
  value: string;
  onChange: (id: string) => void;
  disabled?: boolean;
}) {
  const query = useDeliveryAddresses();
  return (
    <div className="space-y-1.5">
      <Label htmlFor="delivery-address-select">送餐地址（選填）</Label>
      <select
        id="delivery-address-select"
        value={value}
        onChange={(e) => onChange(e.target.value)}
        disabled={disabled || query.isPending || query.isError}
        className="h-11 w-full min-w-0 rounded-lg border border-input bg-background px-3 text-base outline-none focus-visible:border-ring focus-visible:ring-3 focus-visible:ring-ring/50 disabled:cursor-not-allowed disabled:bg-input/50 disabled:opacity-50"
      >
        <option value="">暫不設定</option>
        {query.data?.map((a) => <option key={a.id} value={a.id}>{a.name} — {a.address}</option>)}
      </select>
      {query.isError && (
        <div>
          <p role="alert" className="text-sm text-destructive">{errorMessage(query.error)}</p>
          <Button type="button" variant="outline" className="h-11" onClick={() => query.refetch()}>重試載入地址</Button>
        </div>
      )}
      <p className="text-sm text-muted-foreground">
        <Link to="/delivery-addresses" target="_blank" rel="noopener noreferrer" className="underline">
          管理地址／從照片匯入（另開分頁）
        </Link>
        ，儲存後回到此頁即可選用。
      </p>
    </div>
  );
}
