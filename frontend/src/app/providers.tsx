import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { useState, type ReactNode } from "react";
import { ApiError } from "@/shared/api/client";
import { Toaster } from "@/shared/ui/sonner";

const TOAST_OFFSET = { top: "calc(3.5rem + env(safe-area-inset-top, 0px) + 8px)" };

export function Providers({ children }: { children: ReactNode }) {
  const [queryClient] = useState(
    () => new QueryClient({ defaultOptions: {
          queries: {
            staleTime: 15_000,
            // Client errors (401/404/…) will not fix themselves; only retry network/5xx once.
            retry: (count, err) => !(err instanceof ApiError && err.status < 500) && count < 1,
          },
        } }),
  );
  return (
    <QueryClientProvider client={queryClient}>
      {children}
      {/* Toasts: top-center, pushed below the 56 px sticky admin header (+ notch inset) so they never
          cover it; never at the bottom, where the public page's sticky order bar lives. */}
      <Toaster position="top-center" offset={TOAST_OFFSET} mobileOffset={TOAST_OFFSET} />
    </QueryClientProvider>
  );
}
