import { useEffect, useRef, useState } from "react";
import type { components } from "@/shared/api/schema";
import type { ImportRun } from "@/shared/api/types";
import { ApiError } from "@/shared/api/client";
import { sharingRequest } from "@/features/sharing-groups/hooks";
import { errorMessage } from "@/shared/api/errors";
import { Button } from "@/shared/ui/button";
import { Textarea } from "@/shared/ui/textarea";
import { parsePrice, type TargetState } from "./lib";

export type DraftState = components["schemas"]["DraftStateDto"];
export type DraftVendor = components["schemas"]["DraftVendorDto"];

export function stateToDraft(state: TargetState[]): DraftVendor[] {
  if (state.some((t) => t.type === "vendor" && t.menu.some((m) => Number.isNaN(parsePrice(m.price))))) throw new Error("請先修正價格格式，手動內容尚未送出");
  return state.flatMap((t, ti) => t.type !== "vendor" ? [] : [{
    id: t.draftId ?? `v${ti}`, vendor: t.ref, selected: true, infoSelected: t.info.checked,
    info: { name: t.info.name, phone: t.info.phone || null, address: t.info.address || null, note: t.info.note || null },
    menu: t.menu.map((m, mi) => ({
      id: m.draftId ?? `v${ti}-m${mi}`, selected: m.checked && m.action !== "unchanged",
      name: m.name, price: parsePrice(m.price), category: m.category || null, description: m.description || null,
    })),
  }]);
}

export function draftToState(draft: DraftState): TargetState[] {
  return draft.vendors.map((v) => ({
    type: "vendor", draftId: v.id, ref: v.vendor,
    info: {
      ...v.info, phone: v.info.phone ?? "", address: v.info.address ?? "", note: v.info.note ?? "",
      action: v.vendor.kind === "new" ? "create" : "update", before: v.vendor.kind === "new" ? null : v.info,
      checked: v.selected && v.infoSelected,
    },
    menu: v.menu.map((m) => ({
      draftId: m.id, action: v.vendor.kind === "new" ? "create" : "update", before: null,
      confidence: "high", name: m.name, price: m.price == null ? "" : String(m.price),
      category: m.category ?? "", description: m.description ?? "", checked: v.selected && m.selected,
    })),
  }));
}

/** Manual values travel with every turn. HTTP failures leave the editor untouched. */
export function useDraftConversation(run: ImportRun, state: TargetState[], replace: (s: TargetState[]) => void) {
  const [draft, setDraft] = useState<DraftState | null>(run.draft ?? null);
  const [instruction, setInstruction] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  const [uncertain, setUncertain] = useState(false);
  const mounted = useRef(true);
  const request = useRef<{ revision: number; turnId: string; instruction: string; vendors: DraftVendor[] } | null>(null);
  const latest = useRef(state);
  latest.current = state;
  useEffect(() => { mounted.current = true; return () => { mounted.current = false; }; }, []);
  const path = `ai/import-runs/${run.id}`;
  const invalidPrices = state.some((t) => t.type === "vendor" && t.menu.some((m) => Number.isNaN(parsePrice(m.price))));

  function adopt(next: DraftState) {
    setDraft(next);
    replace(draftToState(next));
  }
  async function perform(action: () => Promise<DraftState>) {
    if (busy) return;
    setBusy(true); setError("");
    const sentState = latest.current;
    try {
      const next = await action();
      if (!mounted.current) return;
      // Defensive even if a field outside the disabled editor changes during a request.
      if (latest.current !== sentState) { setError("草稿已在等待期間修改；未覆蓋手動內容，請先重新載入確認伺服器版本。"); setUncertain(true); return; }
      adopt(next); setUncertain(false);
      if (!next.pending && request.current && next.turns.some((t) => t.id === request.current?.turnId)) { request.current = null; setInstruction(""); }
    } catch (err) {
      if (mounted.current) {
        setError(errorMessage(err));
        if (!(err instanceof ApiError) || err.status === 409 || err.status >= 500) setUncertain(true);
        else request.current = null;
      }
    } finally { if (mounted.current) setBusy(false); }
  }
  function send() {
    if (!instruction.trim() || [...instruction].length > 500) return;
    return perform(() => {
      const body = request.current ?? { revision: draft?.revision ?? 0, turnId: crypto.randomUUID(), instruction: instruction.trim(), vendors: stateToDraft(state) };
      request.current = body;
      // Even a quota/validation rejection may have initialized the server draft.
      if (!draft) setDraft({ revision: 0, vendors: body.vendors, turns: [], undo: null, pending: null, pendingUntil: null });
      return sharingRequest<DraftState>(`${path}/turns`, "POST", body);
    });
  }
  async function persist(): Promise<number | undefined> {
    if (run.mode !== "vendor") return undefined;
    if (!draft && !uncertain) return undefined;
    if (uncertain || busy || draft?.pending) throw new Error("請先確認本輪結果，草稿尚未送出");
    const next = await sharingRequest<DraftState>(`${path}/draft`, "PUT", { revision: draft?.revision ?? 0, vendors: stateToDraft(latest.current) });
    setDraft(next);
    return next.revision;
  }
  const undoTurn = draft?.undo ? [...draft.turns].reverse().find((t) => t.status === "completed" && t.changes.length && !t.undone) : undefined;
  const manualChanged = !!draft && (invalidPrices || JSON.stringify(stateToDraft(state)) !== JSON.stringify(stateToDraft(draftToState(draft))));
  const panel = run.mode !== "vendor" ? null : (
    <section aria-label="草稿對話" className="space-y-3 rounded-lg border p-3">
      <h2 className="font-semibold">用文字修改這份草稿</h2>
      <p className="text-sm text-muted-foreground">每輪包含目前手動編輯；只修改草稿，最後套用才正式儲存。可說「雞腿飯改為 110 元」或「刪除今日特餐」。不明確時 AI 會先提問。</p>
      <p className="text-sm text-muted-foreground">手動修改請按「儲存草稿」以供重新開啟；送出對話也會先保存。原圖僅於有暫存且未過 24 小時時供 AI 參考。</p>
      <p className="text-sm text-muted-foreground">復原上一輪指最近一次有變更的 AI 回合；僅提問不新增復原版本。手動編輯後停用舊回合的復原，避免取代手改。</p>
      {run.model === "demo" && <p className="text-sm text-muted-foreground">示範模式可輸入「示範改價」測試第一項加 5 元；其他指示只會提問。</p>}
      <ol aria-label="對話紀錄" className="space-y-3">
        {draft?.turns.map((t) => <li key={t.id} className="rounded bg-muted p-3 text-sm">
          <p>你：{t.instruction}</p><p>AI：{t.reply || "處理中…"}</p>
          <p>{t.undone ? "已復原" : t.status === "failed" ? "本輪失敗，未修改草稿" : t.changes.length ? "本輪變更：" : t.status === "completed" ? "本輪未修改草稿" : ""}</p>
          <ul>{t.changes.map((c, i) => <li key={i}>{c}</li>)}</ul>
        </li>)}
      </ol>
      <label htmlFor="draft-instruction" className="text-sm font-medium">後續指示（最多 500 字）</label>
      <Textarea id="draft-instruction" value={instruction} disabled={busy || uncertain || !!draft?.pending} maxLength={1000} onChange={(e) => { setInstruction(e.target.value); request.current = null; }} />
      {error && <p role="alert" className="text-sm text-destructive">{error}（畫面上的草稿仍保留）</p>}
      {invalidPrices && <p role="alert" className="text-sm text-destructive">請先修正價格格式再保存或送出指示；手動內容尚未送出。</p>}
      <div className="flex flex-wrap gap-2">
        <Button type="button" disabled={busy || uncertain || invalidPrices || !!draft?.pending || !instruction.trim() || [...instruction].length > 500 || (draft?.turns.length ?? 0) >= 20} onClick={() => void send()}>{busy ? "處理中…" : "送出修改指示"}</Button>
        <Button type="button" variant="outline" disabled={busy || uncertain || invalidPrices || !!draft?.pending} onClick={() => void perform(() => sharingRequest<DraftState>(`${path}/draft`, "PUT", { revision: draft?.revision ?? 0, vendors: stateToDraft(state) }))}>儲存草稿</Button>
        <Button type="button" variant="outline" disabled={busy || uncertain || !undoTurn || manualChanged || !!draft?.pending} onClick={() => {
          if (manualChanged || !undoTurn) return;
          if (!window.confirm("復原最近一次有變更的 AI 回合，回到該輪送出前的草稿（包含當時手動編輯）。確定復原？")) return;
          void perform(() => sharingRequest<DraftState>(`${path}/undo`, "POST", { revision: draft!.revision, turnId: undoTurn!.id }));
        }}>復原上一輪</Button>
        {uncertain && request.current && <Button type="button" variant="outline" disabled={busy} onClick={() => void perform(() => sharingRequest<DraftState>(`${path}/turns`, "POST", request.current))}>重試本輪（不重複計費）</Button>}
        {(uncertain || draft?.pending) && <Button type="button" variant="outline" disabled={busy} onClick={() => {
          if (!window.confirm("重新讀取伺服器草稿會取代尚未送出的手動修改。確定？")) return;
          void perform(() => sharingRequest<DraftState>(`${path}/draft`));
        }}>重新讀取草稿</Button>}
      </div>
    </section>
  );
  return { panel, busy, blocked: uncertain || !!draft?.pending, persist };
}
