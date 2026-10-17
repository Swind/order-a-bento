import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { ApiError, api, unwrap } from "@/shared/api/client";
import type { SubmitOrderBody } from "@/shared/api/types";

export const publicGroupKey = (token: string) => ["public-group", token] as const;
export const effectiveOrderKey = (token: string, pid: string) =>
  ["public-order", token, pid] as const;

export function usePublicGroup(token: string) {
  return useQuery({
    queryKey: publicGroupKey(token),
    queryFn: async () =>
      unwrap(await api.GET("/api/v1/public/g/{token}", { params: { path: { token } } })),
    retry: (count, err) => !(err instanceof ApiError && err.status < 500) && count < 1,
    refetchOnWindowFocus: false,
  });
}

export function useEffectiveOrder(token: string, pid: string | null) {
  return useQuery({
    queryKey: effectiveOrderKey(token, pid ?? ""),
    enabled: !!pid,
    refetchOnWindowFocus: false,
    staleTime: 0,
    gcTime: 0,
    queryFn: async () =>
      unwrap(
        await api.GET("/api/v1/public/g/{token}/participants/{pid}/order", {
          params: { path: { token, pid: pid! } },
        }),
      ),
  });
}

export function useSubmitOrder(token: string, pid: string) {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: async (body: SubmitOrderBody) =>
      unwrap(
        await api.POST("/api/v1/public/g/{token}/participants/{pid}/submissions", {
          params: { path: { token, pid } },
          body,
        }),
      ),
    onSuccess: (data) => {
      qc.setQueryData(effectiveOrderKey(token, pid), data);
      return qc.invalidateQueries({ queryKey: publicGroupKey(token) });
    },
  });
}
