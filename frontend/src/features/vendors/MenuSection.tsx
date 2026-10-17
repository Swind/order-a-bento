import { Pencil } from "lucide-react";
import { useEffect, useRef, useState, type FormEvent } from "react";
import { toast } from "sonner";
import { errorMessage, fieldError } from "@/shared/api/errors";
import type { MenuItem } from "@/shared/api/types";
import { ImportButton } from "@/features/ai-import/ImportDialog";
import { ConfirmDeleteButton } from "@/shared/components/ConfirmDeleteButton";
import { FieldError } from "@/shared/components/FieldError";
import { ReportSummary, itemReportText } from "@/shared/components/ReportSummary";
import { formatMoney } from "@/shared/lib/format";
import { useMediaQuery } from "@/shared/lib/useMediaQuery";
import { cn } from "@/shared/lib/utils";
import { Button } from "@/shared/ui/button";
import { Input } from "@/shared/ui/input";
import { Label } from "@/shared/ui/label";
import { Switch } from "@/shared/ui/switch";
import { Table, TableBody, TableCell, TableHead, TableHeader, TableRow } from "@/shared/ui/table";
import { Textarea } from "@/shared/ui/textarea";
import {
  useDeleteMenuItem,
  usePatchMenuItem,
  useUpsertMenuItems,
  type ItemPatch,
} from "./hooks";
import { MAX_PRICE, parseMenuLines } from "./parseMenuLines";

/** Active items first, archived (dimmed) at the bottom; each group by sortOrder. */
export function sortMenu(items: MenuItem[]): MenuItem[] {
  return [...items].sort(
    (a, b) => Number(a.archived) - Number(b.archived) || a.sortOrder - b.sortOrder,
  );
}

function ItemEditForm({
  vendorId,
  item,
  onDone,
}: {
  vendorId: string;
  item: MenuItem;
  onDone: () => void;
}) {
  const patch = usePatchMenuItem(vendorId);
  const [name, setName] = useState(item.name);
  const [price, setPrice] = useState(String(item.price));
  const [description, setDescription] = useState(item.description ?? "");
  const [category, setCategory] = useState(item.category ?? "");
  const [errors, setErrors] = useState<Record<string, string>>({});

  function save(e: FormEvent) {
    e.preventDefault();
    const errs: Record<string, string> = {};
    const priceNum = Number(price);
    if (!name.trim()) errs.name = "請輸入品名";
    if (!/^\d+$/.test(price.trim()) || priceNum < 1 || priceNum > MAX_PRICE) {
      errs.price = `價格需為 1 到 ${MAX_PRICE.toLocaleString("en-US")} 的整數`;
    }
    setErrors(errs);
    if (Object.keys(errs).length) return;
    const body: ItemPatch = {};
    if (name.trim() !== item.name) body.name = name.trim();
    if (priceNum !== item.price) body.price = priceNum;
    if (description.trim() !== (item.description ?? ""))
      body.description = description.trim() || null;
    if (category.trim() !== (item.category ?? "")) body.category = category.trim() || null;
    if (Object.keys(body).length === 0) return onDone();
    patch.mutate(
      { itemId: item.id, ...body },
      {
        onSuccess: () => {
          toast.success("已更新品項");
          onDone();
        },
        onError: (err) => {
          const f = fieldError(err, ["name", "price", "description", "category"]);
          if (f) setErrors({ [f.field]: f.message });
          else toast.error(errorMessage(err));
        },
      },
    );
  }

  const id = (k: string) => `item-${item.id}-${k}`;
  return (
    <form onSubmit={save} className="grid gap-2 sm:grid-cols-2" aria-label={`編輯 ${item.name}`}>
      <div className="space-y-1">
        <Label htmlFor={id("name")}>品名</Label>
        <Input id={id("name")} value={name} onChange={(e) => setName(e.target.value)} aria-invalid={!!errors.name} />
        <FieldError message={errors.name} />
      </div>
      <div className="space-y-1">
        <Label htmlFor={id("price")}>價格</Label>
        <Input
          id={id("price")}
          inputMode="numeric"
          value={price}
          onChange={(e) => setPrice(e.target.value)}
          aria-invalid={!!errors.price}
        />
        <FieldError message={errors.price} />
      </div>
      <div className="space-y-1">
        <Label htmlFor={id("desc")}>說明</Label>
        <Input id={id("desc")} value={description} onChange={(e) => setDescription(e.target.value)} />
        <FieldError message={errors.description} />
      </div>
      <div className="space-y-1">
        <Label htmlFor={id("cat")}>分類</Label>
        <Input id={id("cat")} value={category} onChange={(e) => setCategory(e.target.value)} />
        <FieldError message={errors.category} />
      </div>
      <div className="flex gap-2 sm:col-span-2">
        <Button type="submit" size="sm" disabled={patch.isPending}>
          儲存
        </Button>
        <Button type="button" size="sm" variant="outline" onClick={onDone}>
          取消
        </Button>
      </div>
    </form>
  );
}

function ItemActions({ vendorId, item, onEdit }: { vendorId: string; item: MenuItem; onEdit: () => void }) {
  const patch = usePatchMenuItem(vendorId);
  const del = useDeleteMenuItem(vendorId);
  return (
    <div className="flex items-center gap-1">
      <label className="flex items-center gap-1.5 text-sm text-muted-foreground">
        <Switch
          size="sm"
          checked={item.archived}
          disabled={patch.isPending}
          aria-label={`封存 ${item.name}`}
          onCheckedChange={(archived) =>
            patch.mutate(
              { itemId: item.id, archived },
              { onError: (err) => toast.error(errorMessage(err)) },
            )
          }
        />
        封存
      </label>
      <Button variant="ghost" size="icon" aria-label={`編輯 ${item.name}`} onClick={onEdit}>
        <Pencil />
      </Button>
      <ConfirmDeleteButton
        label={`刪除 ${item.name}`}
        pending={del.isPending}
        onConfirm={() =>
          del.mutate(item.id, {
            onSuccess: () => toast.success("已刪除品項"),
            onError: (err) => toast.error(errorMessage(err)),
          })
        }
      />
    </div>
  );
}

function ItemCard({ vendorId, item, highlighted }: { vendorId: string; item: MenuItem; highlighted: boolean }) {
  const [editing, setEditing] = useState(false);
  return (
    <li
      className={cn(
        "rounded-lg border p-3 transition-colors duration-700",
        item.archived && "opacity-60",
        highlighted && "border-primary bg-primary/10",
      )}
    >
      {editing ? (
        <ItemEditForm vendorId={vendorId} item={item} onDone={() => setEditing(false)} />
      ) : (
        <div className="space-y-1">
          <div className="flex items-baseline justify-between gap-2">
            <p className="min-w-0 break-words font-medium">{item.name}</p>
            <p className="shrink-0 font-medium">{formatMoney(item.price)}</p>
          </div>
          {(item.description || item.category) && (
            <p className="break-words text-sm text-muted-foreground">
              {[item.category, item.description].filter(Boolean).join(" · ")}
            </p>
          )}
          <ItemActions vendorId={vendorId} item={item} onEdit={() => setEditing(true)} />
        </div>
      )}
    </li>
  );
}

function ItemTableRow({ vendorId, item, highlighted }: { vendorId: string; item: MenuItem; highlighted: boolean }) {
  const [editing, setEditing] = useState(false);
  const cls = cn(item.archived && "opacity-60", highlighted && "bg-primary/10");
  if (editing) {
    return (
      <TableRow className={cls}>
        <TableCell colSpan={5}>
          <ItemEditForm vendorId={vendorId} item={item} onDone={() => setEditing(false)} />
        </TableCell>
      </TableRow>
    );
  }
  return (
    <TableRow className={cls}>
      <TableCell className="font-medium whitespace-normal">{item.name}</TableCell>
      <TableCell className="text-right">{formatMoney(item.price)}</TableCell>
      <TableCell className="whitespace-normal text-muted-foreground">{item.description}</TableCell>
      <TableCell className="text-muted-foreground">{item.category}</TableCell>
      <TableCell>
        <ItemActions vendorId={vendorId} item={item} onEdit={() => setEditing(true)} />
      </TableCell>
    </TableRow>
  );
}

function BatchAddItems({ vendorId, onAffected }: { vendorId: string; onAffected: (ids: string[]) => void }) {
  const upsert = useUpsertMenuItems(vendorId);
  const [text, setText] = useState("");
  const [summary, setSummary] = useState<string | null>(null);
  const { items, errors } = parseMenuLines(text);

  function submit(e: FormEvent) {
    e.preventDefault();
    if (items.length === 0 || errors.length > 0) return;
    upsert.mutate(items, {
      onSuccess: ({ report }) => {
        setSummary(itemReportText(report));
        setText("");
        onAffected(report.outcomes.filter((o) => o.outcome !== "unchanged").map((o) => o.itemId));
      },
      onError: (err) => {
        setSummary(null);
        toast.error(errorMessage(err));
      },
    });
  }

  return (
    <form onSubmit={submit} className="space-y-2">
      <Label htmlFor="batch-items">批次新增品項</Label>
      <Textarea
        id="batch-items"
        rows={5}
        value={text}
        onChange={(e) => setText(e.target.value)}
        placeholder={"一行一項：品名 價格，或 品名,價格,說明\n雞腿飯 85\n排骨便當,90,附滷蛋"}
      />
      {errors.length > 0 && (
        <ul role="alert" className="space-y-0.5 text-sm text-destructive">
          {errors.map((e) => (
            <li key={e.line}>
              第 {e.line} 行：{e.reason}
            </li>
          ))}
        </ul>
      )}
      <div className="flex flex-wrap items-center gap-2">
        <Button type="submit" disabled={items.length === 0 || errors.length > 0 || upsert.isPending}>
          新增 {items.length} 項
        </Button>
        <ImportButton label="📷 從傳單匯入" mode="vendor" targetVendorId={vendorId} />
      </div>
      {summary && <ReportSummary text={summary} />}
    </form>
  );
}

export function MenuSection({ vendorId, items }: { vendorId: string; items: MenuItem[] }) {
  const desktop = useMediaQuery("(min-width: 768px)");
  const [highlight, setHighlight] = useState<Set<string>>(new Set());
  const timer = useRef<number | undefined>(undefined);
  useEffect(() => () => window.clearTimeout(timer.current), []);
  function flash(ids: string[]) {
    setHighlight(new Set(ids));
    window.clearTimeout(timer.current);
    timer.current = window.setTimeout(() => setHighlight(new Set()), 3000);
  }
  const sorted = sortMenu(items);

  return (
    <section className="space-y-4" aria-label="菜單">
      <h2 className="text-lg font-semibold">菜單（{items.length}）</h2>
      <BatchAddItems vendorId={vendorId} onAffected={flash} />
      {sorted.length === 0 ? (
        <p className="rounded-xl border border-dashed py-8 text-center text-muted-foreground">
          還沒有品項，從上方貼上菜單吧。
        </p>
      ) : desktop ? (
        <Table>
          <TableHeader>
            <TableRow>
              <TableHead>品名</TableHead>
              <TableHead className="text-right">價格</TableHead>
              <TableHead>說明</TableHead>
              <TableHead>分類</TableHead>
              <TableHead>操作</TableHead>
            </TableRow>
          </TableHeader>
          <TableBody>
            {sorted.map((it) => (
              <ItemTableRow
                key={`${it.id}-${it.name}-${it.price}-${it.description}-${it.category}-${it.archived}`}
                vendorId={vendorId}
                item={it}
                highlighted={highlight.has(it.id)}
              />
            ))}
          </TableBody>
        </Table>
      ) : (
        <ul className="space-y-2">
          {sorted.map((it) => (
            <ItemCard
              key={`${it.id}-${it.name}-${it.price}-${it.description}-${it.category}-${it.archived}`}
              vendorId={vendorId}
              item={it}
              highlighted={highlight.has(it.id)}
            />
          ))}
        </ul>
      )}
    </section>
  );
}
