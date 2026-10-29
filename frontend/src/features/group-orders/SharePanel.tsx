import { useRef } from "react";
import { Copy, ExternalLink, MessageCircle } from "lucide-react";
import { toast } from "sonner";
import { Button, buttonVariants } from "@/shared/ui/button";
import { Input } from "@/shared/ui/input";
import { Label } from "@/shared/ui/label";
import { lineShareUrl } from "./share";

export function SharePanel({ title, shareUrl }: { title: string; shareUrl: string }) {
  const inputRef = useRef<HTMLInputElement>(null);
  const detailsRef = useRef<HTMLDetailsElement>(null);

  async function copy() {
    try {
      if (!navigator.clipboard?.writeText) throw new Error("clipboard unavailable");
      await navigator.clipboard.writeText(shareUrl);
      toast.success("已複製連結");
    } catch {
      if (detailsRef.current) detailsRef.current.open = true;
      inputRef.current?.focus();
      inputRef.current?.select();
      toast.message("無法自動複製，已選取連結，請手動複製");
    }
  }

  return (
    <section aria-label="分享訂購頁" className="space-y-1">
      <div className="flex flex-wrap gap-2">
        <Button type="button" className="h-11" onClick={copy}>
          <Copy /> 複製連結
        </Button>
        <a href={lineShareUrl(title, shareUrl)} target="_blank" rel="noopener noreferrer"
          className={buttonVariants({ variant: "outline", className: "h-11" })}>
          <MessageCircle /> 分享到 LINE
        </a>
      </div>
      <details ref={detailsRef} className="text-sm">
        <summary className="min-h-11 w-fit cursor-pointer content-center rounded-lg px-2 py-2 text-muted-foreground focus-visible:outline-2 focus-visible:outline-ring">連結與其他操作</summary>
        <div className="space-y-2 p-2">
          <a href={shareUrl} target="_blank" rel="noopener noreferrer"
            className={buttonVariants({ variant: "ghost", className: "h-11" })}>
            <ExternalLink /> 開啟訂購頁
          </a>
          <Label htmlFor="share-url">分享連結</Label>
          <Input
            id="share-url"
            ref={inputRef}
            readOnly
            className="h-11"
            value={shareUrl}
            onFocus={(e) => e.currentTarget.select()}
          />
        </div>
      </details>
    </section>
  );
}
