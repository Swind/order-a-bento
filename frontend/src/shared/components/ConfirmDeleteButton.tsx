import { Trash2 } from "lucide-react";
import { useState } from "react";
import { Button } from "@/shared/ui/button";

interface Props {
  onConfirm: () => void;
  /** Accessible label of the initial button, e.g. 「刪除名單」. */
  label?: string;
  /** Show the label text next to the icon (otherwise icon only). */
  showLabel?: boolean;
  pending?: boolean;
  disabled?: boolean;
}

/** Inline two-step delete: the button turns into 「確定刪除？」 confirm / cancel. */
export function ConfirmDeleteButton({
  onConfirm,
  label = "刪除",
  showLabel = false,
  pending = false,
  disabled = false,
}: Props) {
  const [confirming, setConfirming] = useState(false);
  if (!confirming) {
    return (
      <Button
        type="button"
        variant="ghost"
        size={showLabel ? "default" : "icon"}
        aria-label={label}
        disabled={disabled}
        onClick={() => setConfirming(true)}
      >
        <Trash2 />
        {showLabel && label}
      </Button>
    );
  }
  return (
    <span className="inline-flex flex-wrap items-center gap-1" role="group" aria-label="確認刪除">
      <span className="text-sm text-destructive">確定刪除？</span>
      <Button
        type="button"
        variant="destructive"
        size="sm"
        disabled={pending}
        onClick={() => {
          onConfirm();
          setConfirming(false);
        }}
      >
        確定
      </Button>
      <Button type="button" variant="outline" size="sm" onClick={() => setConfirming(false)}>
        取消
      </Button>
    </span>
  );
}
