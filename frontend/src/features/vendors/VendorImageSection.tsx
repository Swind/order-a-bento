import { useEffect, useRef, useState } from "react";
import { useQueryClient } from "@tanstack/react-query";
import { toast } from "sonner";
import { api, ApiError, unwrap } from "@/shared/api/client";
import { errorMessage } from "@/shared/api/errors";
import type { VendorDetail } from "@/shared/api/types";
import { ConfirmDeleteButton } from "@/shared/components/ConfirmDeleteButton";
import { Button } from "@/shared/ui/button";
import { Dialog, DialogContent, DialogDescription, DialogHeader, DialogTitle, DialogTrigger } from "@/shared/ui/dialog";
import { useMe } from "@/features/auth/hooks";
import { invalidateSharingAccessOnError } from "@/features/sharing-groups/hooks";
import { useVendorImageMutation } from "./hooks";

const MAX_BYTES = 5 * 1024 * 1024;
const denied = (err: unknown) => err instanceof ApiError && [401, 403, 404].includes(err.status);
function imageError(err: unknown) {
  if (err instanceof ApiError && err.code === "invalid_image") return "圖片無效，請選擇靜態 JPEG 或 PNG（不支援動畫與 WebP）";
  if (err instanceof ApiError && err.code === "image_too_large") return "圖片太大（原檔上限 5 MiB），請選擇較小的照片";
  return errorMessage(err);
}

function useObjectUrl(file: Blob | null) {
  const [resource, setResource] = useState<{ file: Blob; url: string }>();
  useEffect(() => {
    const next = file ? { file, url: URL.createObjectURL(file) } : undefined;
    // Object URLs are external resources owned by this effect, not render-time values.
    // oxlint-disable-next-line react/set-state-in-effect
    setResource(next);
    return () => { if (next) URL.revokeObjectURL(next.url); };
  }, [file]);
  return resource?.file === file ? resource?.url : undefined;
}

function Photo({ src, local = false }: { src: string; local?: boolean }) {
  const title = local ? "待上傳照片預覽" : "參考照片預覽";
  return <Dialog>
    <DialogTrigger render={<Button variant="outline" className="h-auto min-h-11 min-w-11 max-w-full p-2" aria-label={title} />}>
      <img src={src} alt={local ? "待上傳照片" : "店家參考照片"} className="max-h-64 max-w-full object-contain" />
    </DialogTrigger>
    <DialogContent className="max-h-[90dvh] overflow-auto sm:max-w-4xl [&_[data-slot=dialog-close]]:min-h-11 [&_[data-slot=dialog-close]]:min-w-11">
      <DialogHeader><DialogTitle>{title}</DialogTitle><DialogDescription>點選關閉或按 Escape 返回。</DialogDescription></DialogHeader>
      <img src={src} alt={title} className="max-h-[75dvh] w-full object-contain" />
    </DialogContent>
  </Dialog>;
}

function SavedPhoto({ id, revision }: { id: string; revision: string }) {
  const qc = useQueryClient();
  const [attempt, setAttempt] = useState(0);
  const [blob, setBlob] = useState<Blob | null>(null);
  const [error, setError] = useState<unknown>(null);
  const url = useObjectUrl(blob);
  useEffect(() => {
    const controller = new AbortController();
    async function load() {
      try {
        const data = await unwrap(await api.GET("/api/v1/vendors/{vendorId}/image", {
          params: { path: { vendorId: id } }, parseAs: "blob", signal: controller.signal,
        }));
        if (!controller.signal.aborted) setBlob(data);
      } catch (err) {
        if (controller.signal.aborted) return;
        setError(err);
        if (denied(err)) {
          void qc.invalidateQueries({ queryKey: ["vendor", id], exact: true });
          void invalidateSharingAccessOnError(qc, err);
        }
      }
    }
    void load();
    return () => controller.abort();
  }, [id, revision, attempt, qc]);
  if (error) return <div className="space-y-2"><p role="alert">{imageError(error)}</p><Button variant="outline" onClick={() => { setError(null); setAttempt(attempt + 1); }}>重新載入照片</Button></div>;
  return url ? <Photo src={url} /> : <p role="status">照片載入中…</p>;
}

function ImageSection({ vendor }: { vendor: VendorDetail }) {
  const mutation = useVendorImageMutation(vendor.id);
  const [file, setFile] = useState<File | null>(null);
  const [error, setError] = useState<string>();
  const [accessLost, setAccessLost] = useState(false);
  const [busy, setBusy] = useState<"upload" | "delete" | null>(null);
  const input = useRef<HTMLInputElement>(null);
  const abort = useRef<AbortController | null>(null);
  const preview = useObjectUrl(file);
  useEffect(() => () => abort.current?.abort(), []);

  async function save(remove = false) {
    if (abort.current || (!remove && !file)) return;
    const controller = new AbortController();
    abort.current = controller;
    setBusy(remove ? "delete" : "upload");
    setError(undefined);
    try {
      await mutation.mutateAsync({ file: remove ? undefined : file!, signal: controller.signal });
      if (controller.signal.aborted) return;
      setFile(null);
      toast.success(remove ? "已刪除參考照片" : "已儲存參考照片");
    } catch (err) {
      if (controller.signal.aborted) return;
      setError(imageError(err));
      if (denied(err)) { setAccessLost(true); setFile(null); }
    } finally {
      if (!controller.signal.aborted) { abort.current = null; setBusy(null); }
    }
  }

  return <section aria-label="店家參考照片" aria-busy={!!busy} className="space-y-3 rounded-lg border p-4 [&_button]:min-h-11">
    <h2 className="text-lg font-semibold">參考照片</h2>
    {vendor.image && !accessLost
      ? <SavedPhoto key={vendor.image.revision} id={vendor.id} revision={vendor.image.revision} />
      : <p className="text-sm text-muted-foreground">{accessLost ? "照片存取權限已失效，請重新整理後再試" : "尚未上傳參考照片"}</p>}
    {vendor.canManage !== false && !accessLost && <>
      <p className="text-sm text-muted-foreground">僅靜態 JPEG／PNG，原檔上限 5 MiB；不支援 WebP。伺服器會轉為白底 JPEG，長邊最多 1568 像素。不會修改菜單。</p>
      <input ref={input} type="file" accept="image/jpeg,image/png" aria-label="選擇參考照片檔案" className="hidden" disabled={!!busy}
        onChange={(e) => {
          const next = e.target.files?.[0];
          e.target.value = ""; // Allow choosing the same file after cancel/error.
          if (!next) return;
          setError(undefined);
          if (!["image/jpeg", "image/png"].includes(next.type) || !next.size) {
            setError("請選擇非空白的 JPEG 或 PNG 照片（不支援 WebP）"); return;
          }
          if (next.size > MAX_BYTES) { setError("圖片太大（原檔上限 5 MiB），請選擇較小的照片"); return; }
          // Keep original bytes: browser recompression would hide APNG and lose decoder validation.
          setFile(next);
        }} />
      {file && preview && <div className="space-y-2"><Photo src={preview} local /><p className="text-sm">{vendor.image ? "確認後將取代現有照片。" : "確認上傳後才會儲存照片。"}</p></div>}
      <div className="flex flex-wrap gap-2">
        <Button variant="outline" disabled={!!busy} onClick={() => input.current?.click()}>{file ? "重新選擇照片" : "選擇照片"}</Button>
        {file && <>
          <Button disabled={!!busy} onClick={() => void save()}>{busy === "upload" ? "上傳中…" : vendor.image ? "確認替換照片" : "確認上傳照片"}</Button>
          <Button variant="outline" disabled={!!busy} onClick={() => { setFile(null); setError(undefined); }}>取消上傳</Button>
        </>}
        {vendor.image && <ConfirmDeleteButton label="刪除照片" showLabel disabled={!!busy} pending={!!busy} onConfirm={() => void save(true)} />}
      </div>
      {busy && <p role="status">{busy === "delete" ? "刪除中…" : "上傳中…"}離開頁面會取消等待，但伺服器可能已完成儲存。</p>}
    </>}
    {error && <p role="alert" className="text-destructive">{error}</p>}
  </section>;
}

export function VendorImageSection({ vendor }: { vendor: VendorDetail }) {
  const me = useMe();
  return <ImageSection key={`${me.data?.id ?? ""}:${vendor.id}:${vendor.canManage}`} vendor={vendor} />;
}
