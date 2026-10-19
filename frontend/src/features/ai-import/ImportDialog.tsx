import { Loader2 } from "lucide-react";
import { useEffect, useRef, useState, type FormEvent } from "react";
import { useNavigate } from "react-router";
import { ApiError } from "@/shared/api/client";
import { errorMessage } from "@/shared/api/errors";
import type { ImportMode } from "@/shared/api/types";
import { FieldError } from "@/shared/components/FieldError";
import { compressImage } from "@/shared/lib/compressImage";
import { Badge } from "@/shared/ui/badge";
import { Button } from "@/shared/ui/button";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogHeader,
  DialogTitle,
} from "@/shared/ui/dialog";
import { Label } from "@/shared/ui/label";
import { Textarea } from "@/shared/ui/textarea";
import { useAuthConfig } from "@/features/auth/hooks";
import { startImport } from "./hooks";

export interface ImportTarget {
  mode: ImportMode;
  targetRosterId?: string;
  targetVendorId?: string;
}

const MAX_INSTRUCTION = 200;
const ACCEPT = "image/jpeg,image/png,image/webp";

interface DialogProps extends ImportTarget {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  aiDemo: boolean;
}

/**
 * Upload dialog. While the request is pending the dialog cannot be dismissed; the only
 * way out is 「取消等待」, which aborts the HTTP request (the server run may still finish
 * and is simply never opened).
 */
function ImportDialogContent({ open, onOpenChange, aiDemo, ...target }: DialogProps) {
  const navigate = useNavigate();
  const [file, setFile] = useState<File | null>(null);
  const [preview, setPreview] = useState<string | null>(null);
  const [instruction, setInstruction] = useState("");
  const [pending, setPending] = useState(false);
  const [error, setError] = useState<{ message: string; retry: boolean } | null>(null);
  const abort = useRef<AbortController | null>(null);
  const input = useRef<HTMLInputElement>(null);

  useEffect(() => {
    if (!file || typeof URL.createObjectURL !== "function") {
      setPreview(null);
      return;
    }
    const url = URL.createObjectURL(file);
    setPreview(url);
    return () => URL.revokeObjectURL(url);
  }, [file]);

  useEffect(() => () => abort.current?.abort(), []);

  function reset() {
    setFile(null);
    setInstruction("");
    setError(null);
  }

  function change(next: boolean) {
    if (!next && pending) return;
    if (!next) reset();
    onOpenChange(next);
  }

  async function submit(e?: FormEvent) {
    e?.preventDefault();
    e?.stopPropagation(); // the dialog is portaled out of a parent <form>, but React events still bubble
    if (!file || pending) return;
    if ([...instruction].length > MAX_INSTRUCTION) return;
    setError(null);
    setPending(true);
    const ctrl = new AbortController();
    abort.current = ctrl;
    try {
      let image: Blob;
      try {
        image = await compressImage(file);
      } catch {
        setError({ message: errorMessage(new ApiError({ type: "", title: "", status: 422, code: "invalid_image" })), retry: false });
        return;
      }
      const run = await startImport({
        image,
        instruction: instruction.trim() || undefined,
        ...target,
        signal: ctrl.signal,
      });
      onOpenChange(false);
      reset();
      navigate(`/ai-import/${run.id}`);
    } catch (err) {
      if (ctrl.signal.aborted) return;
      setError({
        message: errorMessage(err),
        retry: err instanceof ApiError ? err.code === "ai_unavailable" : err instanceof TypeError,
      });
    } finally {
      abort.current = null;
      setPending(false);
    }
  }

  const tooLong = [...instruction].length > MAX_INSTRUCTION;
  const placeholder = target.mode === "deliveryAddress" ? "例如：辨識教室名稱、地址與樓層" : target.mode === "vendor" ? "例如：更新這間店的菜單" : "例如：加到業務部名單";

  return (
    <Dialog open={open} onOpenChange={change}>
      <DialogContent showCloseButton={!pending} className="max-h-[90dvh] overflow-y-auto">
        {pending ? (
          <div role="status" className="flex flex-col items-center gap-4 py-8 text-center">
            <DialogTitle className="sr-only">AI 辨識中</DialogTitle>
            <Loader2 className="size-10 animate-spin text-primary" aria-hidden />
            <p className="text-base font-medium">AI 辨識中…（通常 10–60 秒，模型較慢時可能需要數分鐘）</p>
            <div className="h-1.5 w-full overflow-hidden rounded-full bg-muted" aria-hidden>
              <div className="h-full w-1/3 animate-pulse rounded-full bg-primary" />
            </div>
            <p className="text-sm text-muted-foreground">請不要關閉此視窗。</p>
            <Button
              type="button"
              variant="outline"
              className="h-11"
              onClick={() => abort.current?.abort()}
            >
              取消等待
            </Button>
          </div>
        ) : (
          <form onSubmit={submit} className="grid gap-4">
            <DialogHeader>
              <DialogTitle>
                {target.mode === "deliveryAddress" ? "從照片匯入送餐地址" : target.mode === "vendor" ? "從傳單匯入菜單" : "從照片匯入名單"}
              </DialogTitle>
              <DialogDescription>
                選一張照片，AI 會整理成待確認的變更，確認後才會寫入。
              </DialogDescription>
              {aiDemo && (
                <p className="flex items-start gap-2 text-sm text-muted-foreground">
                  <Badge variant="secondary" className="shrink-0">示範模式</Badge>
                  <span>目前使用示範資料，辨識結果是範例，與照片內容無關。</span>
                </p>
              )}
            </DialogHeader>
            <div className="space-y-2">
              <input
                ref={input}
                type="file"
                accept={ACCEPT}
                aria-label="選擇照片檔案"
                className="hidden"
                onChange={(e) => {
                  setFile(e.target.files?.[0] ?? null);
                  setError(null);
                }}
              />
              <Button type="button" variant="outline" className="h-11 w-full" onClick={() => input.current?.click()}>
                {file ? "重新選擇照片" : "選擇照片"}
              </Button>
              {file && (
                <div className="flex items-center gap-3 rounded-lg border p-2">
                  {preview && (
                    <img src={preview} alt="照片預覽" className="size-20 shrink-0 rounded object-cover" />
                  )}
                  <span className="min-w-0 break-all text-sm text-muted-foreground">{file.name}</span>
                </div>
              )}
            </div>
            <div className="space-y-1.5">
              <Label htmlFor="import-instruction">指示（選填）</Label>
              <Textarea
                id="import-instruction"
                rows={2}
                value={instruction}
                onChange={(e) => setInstruction(e.target.value)}
                placeholder={placeholder}
                aria-invalid={tooLong}
              />
              <p className="text-right text-xs text-muted-foreground">
                {[...instruction].length}/{MAX_INSTRUCTION}
              </p>
              {tooLong && <FieldError message="指示文字不可超過 200 字" />}
            </div>
            {error && (
              <div className="space-y-2">
                <p role="alert" className="text-sm text-destructive">{error.message}</p>
                {error.retry && (
                  <Button type="button" variant="outline" className="h-11" onClick={() => submit()}>
                    重試
                  </Button>
                )}
              </div>
            )}
            <Button type="submit" className="h-11" disabled={!file || tooLong}>
              開始辨識
            </Button>
          </form>
        )}
      </DialogContent>
    </Dialog>
  );
}

interface ButtonProps extends ImportTarget {
  label: string;
  variant?: "outline" | "default";
}

/**
 * Entry point: opens the upload dialog when AI import is enabled; otherwise a disabled
 * button explaining that AI is not configured.
 */
export function ImportButton({ label, variant = "outline", ...target }: ButtonProps) {
  const config = useAuthConfig();
  const [open, setOpen] = useState(false);
  if (config.isPending) return null;
  if (!config.data?.aiImportEnabled) {
    return (
      <div className="flex items-center gap-2">
        <Button type="button" variant={variant} className="h-11" disabled>
          {label}
        </Button>
        <span className="text-sm text-muted-foreground">尚未設定 AI</span>
      </div>
    );
  }
  return (
    <>
      <Button type="button" variant={variant} className="h-11" onClick={() => setOpen(true)}>
        {label}
      </Button>
      <ImportDialogContent open={open} onOpenChange={setOpen} aiDemo={config.data.aiDemo} {...target} />
    </>
  );
}
