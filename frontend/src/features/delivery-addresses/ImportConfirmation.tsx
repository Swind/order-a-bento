import { useState } from "react";
import { useNavigate } from "react-router";
import { toast } from "sonner";
import type { ImportRun } from "@/shared/api/types";
import { errorMessage } from "@/shared/api/errors";
import { ConfirmActionButton } from "@/shared/components/ConfirmActionButton";
import { Badge } from "@/shared/ui/badge";
import { Button } from "@/shared/ui/button";
import { Input } from "@/shared/ui/input";
import { Label } from "@/shared/ui/label";
import { Textarea } from "@/shared/ui/textarea";
import { useApplyImport, useDiscardImport } from "@/features/ai-import/hooks";

function validAddress(r: { name: string; address: string }) {
  return r.name.trim() && [...r.name.trim()].length <= 50 && r.address.trim() && [...r.address.trim()].length <= 200;
}

export function DeliveryAddressImportConfirmation({ run, demo }: { run: ImportRun; demo: boolean }) {
  const navigate = useNavigate();
  const apply = useApplyImport(run.id);
  const discard = useDiscardImport(run.id);
  const [rows, setRows] = useState(() =>
    run.proposal.targets.filter((t) => t.type === "deliveryAddress").map((t) => ({ ...t, checked: true })),
  );
  const selected = rows.filter((r) => r.checked);
  const pending = apply.isPending || discard.isPending;
  const valid = selected.every(validAddress);

  function update(i: number, patch: { name?: string; address?: string; checked?: boolean }) {
    setRows((prev) => prev.map((row, j) => j === i ? { ...row, ...patch } : row));
  }

  function submit() {
    apply.mutate(
      selected.map((r) => ({ type: "deliveryAddress", name: r.name.trim(), address: r.address.trim() })),
      {
        onSuccess: () => { toast.success("已匯入送餐地址"); navigate("/delivery-addresses"); },
        onError: (e) => toast.error(errorMessage(e)),
      },
    );
  }

  return (
    <div className="mx-auto max-w-xl space-y-4">
      <h1 className="text-2xl font-bold">送餐地址匯入確認</h1>
      {demo && <Badge variant="secondary">示範資料</Badge>}
      <p className="text-sm text-muted-foreground">尚未儲存。請核對並補齊地址、樓層及教室號碼，再確認匯入。</p>
      {run.summary && <p>{run.summary}</p>}
      {run.proposal.warnings.map((w, i) => (
        <p key={i} role="alert" className="rounded-lg border border-yellow-500 p-3">⚠ {w}</p>
      ))}
      {rows.length === 0 && <p>AI 沒有找到送餐地址，請改用手動輸入或重新上傳照片。</p>}
      {rows.map((r, i) => (
        <fieldset key={i} disabled={pending} className="space-y-3 rounded-xl border p-4">
          <legend className="px-1">送餐地點 {i + 1}</legend>
          <label className="flex min-h-11 items-center gap-2">
            <input type="checkbox" checked={r.checked} onChange={(e) => update(i, { checked: e.target.checked })} />
            匯入此地址
          </label>
          {r.confidence !== "high" && (
            <p className="text-sm text-muted-foreground">⚠ 辨識信心{r.confidence === "low" ? "較低" : "中等"}，請仔細核對。</p>
          )}
          <Label htmlFor={`address-import-name-${i}`}>地點名稱</Label>
          <Input id={`address-import-name-${i}`} value={r.name} onChange={(e) => update(i, { name: e.target.value })} />
          <Label htmlFor={`address-import-address-${i}`}>送餐地址</Label>
          <Textarea id={`address-import-address-${i}`} value={r.address} onChange={(e) => update(i, { address: e.target.value })} />
          {r.checked && !validAddress(r) && (
            <p role="alert" className="text-sm text-destructive">地點名稱需為 1–50 字，送餐地址需為 1–200 字。</p>
          )}
        </fieldset>
      ))}
      <div className="flex flex-wrap gap-2">
        <ConfirmActionButton
          label="放棄"
          message="放棄這次辨識結果？不會儲存任何地址。"
          pending={pending}
          onConfirm={() => discard.mutate(undefined, {
            onSuccess: () => navigate("/delivery-addresses"),
            onError: (e) => toast.error(errorMessage(e)),
          })}
        />
        <Button className="h-11" disabled={pending || !selected.length || !valid} onClick={submit}>
          確認匯入 {selected.length} 個地址
        </Button>
      </div>
    </div>
  );
}
