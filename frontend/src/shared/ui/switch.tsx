import { Switch as SwitchPrimitive } from "@base-ui/react/switch"
import { cn } from "cn"

function Switch({
  className,
  size = "default",
  ...props
}: SwitchPrimitive.Root.Props & {
  size?: "sm" | "default"
}) {
  return (
    <SwitchPrimitive.Root
      data-slot="switch"
      data-size={size}
      className={cn(
        // Track: checked = solid primary; unchecked = clearly visible gray with a darker border.
        "peer group/switch relative inline-flex shrink-0 items-center rounded-full border-2 transition-all outline-none after:absolute after:-inset-x-3 after:-inset-y-3 focus-visible:ring-3 focus-visible:ring-ring/50 aria-invalid:border-destructive aria-invalid:ring-3 aria-invalid:ring-destructive/20 data-[size=default]:h-6 data-[size=default]:w-11 data-[size=sm]:h-[18px] data-[size=sm]:w-8 data-checked:border-primary data-checked:bg-primary data-unchecked:border-zinc-400 data-unchecked:bg-zinc-300 dark:data-unchecked:border-zinc-500 dark:data-unchecked:bg-zinc-600 data-disabled:cursor-not-allowed data-disabled:opacity-50",
        className
      )}
      {...props}
    >
      <SwitchPrimitive.Thumb
        data-slot="switch-thumb"
        className="pointer-events-none block rounded-full bg-white shadow-sm ring-0 transition-transform group-data-[size=default]/switch:size-4 group-data-[size=sm]/switch:size-2.5 group-data-[size=default]/switch:data-checked:translate-x-[22px] group-data-[size=sm]/switch:data-checked:translate-x-[14px] group-data-[size=default]/switch:data-unchecked:translate-x-[2px] group-data-[size=sm]/switch:data-unchecked:translate-x-[1px]"
      />
    </SwitchPrimitive.Root>
  )
}

export { Switch }
