import { useState } from "react";
import { Button } from "@/shared/ui/button";

interface Props {
  label: string;
  /** Explains the consequence, shown while confirming. */
  message: string;
  onConfirm: () => void;
  pending?: boolean;
  variant?: "outline" | "destructive";
}

/** Inline two-step action: the button turns into a message with 確定 / 取消. */
export function ConfirmActionButton({
  label,
  message,
  onConfirm,
  pending = false,
  variant = "outline",
}: Props) {
  const [confirming, setConfirming] = useState(false);
  if (!confirming) {
    return (
      <Button type="button" variant={variant} className="h-11" disabled={pending} onClick={() => setConfirming(true)}>
        {label}
      </Button>
    );
  }
  return (
    <div
      role="group"
      aria-label={`確認${label}`}
      className="flex w-full flex-wrap items-center gap-2 rounded-lg border border-destructive/40 p-2"
    >
      <p className="w-full text-sm">{message}</p>
      <Button
        type="button"
        variant="destructive"
        className="h-11"
        disabled={pending}
        onClick={() => {
          onConfirm();
          setConfirming(false);
        }}
      >
        確定{label}
      </Button>
      <Button type="button" variant="outline" className="h-11" disabled={pending} onClick={() => setConfirming(false)}>
        取消
      </Button>
    </div>
  );
}
