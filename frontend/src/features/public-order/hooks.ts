import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { createContext, useContext, useEffect, useState } from "react";
import { ApiError, api, unwrap } from "@/shared/api/client";
import type { SubmitOrderBody } from "@/shared/api/types";

export const publicGroupKey = (token: string) => ["public-group", token] as const;
export const publicOrdersKey = (token: string) => ["public-orders", token] as const;
export const PublicAccessContext = createContext<{ isBlocked: () => boolean; onError: (error: unknown) => void }>({
  isBlocked: () => false,
  onError: (_error: unknown) => {},
});
const retryPublic = (count: number, err: unknown) =>
  !(err instanceof ApiError && err.status < 500) && count < 1;

function usePublicRequest() {
  const access = useContext(PublicAccessContext);
  return async <T,>(request: () => Promise<T>): Promise<T> => {
    if (access.isBlocked()) throw new DOMException("Link blocked", "AbortError");
    try {
      const data = await request();
      if (access.isBlocked()) throw new DOMException("Link blocked", "AbortError");
      return data;
    } catch (error) {
      access.onError(error);
      throw error;
    }
  };
}
export const effectiveOrderKey = (token: string, pid: string) =>
  ["public-order", token, pid] as const;
export const orderSubmissionKey = (token: string, pid: string) =>
  ["public-order-submit", token, pid] as const;

export function usePublicGroup(token: string) {
  const request = usePublicRequest();
  return useQuery({
    queryKey: publicGroupKey(token),
    queryFn: ({ signal }) => request(async () =>
      unwrap(await api.GET("/api/v1/public/g/{token}", { params: { path: { token } }, signal }))),
    retry: retryPublic,
    refetchOnWindowFocus: false,
  });
}

export function usePublicOrders(token: string, active: boolean) {
  const qc = useQueryClient();
  const request = usePublicRequest();
  const [visible, setVisible] = useState(() => document.visibilityState !== "hidden");
  useEffect(() => {
    const update = () => setVisible(document.visibilityState !== "hidden");
    document.addEventListener("visibilitychange", update);
    return () => document.removeEventListener("visibilitychange", update);
  }, []);
  const query = useQuery({
    queryKey: publicOrdersKey(token),
    enabled: active && visible,
    staleTime: 0,
    gcTime: 0,
    retry: retryPublic,
    refetchOnWindowFocus: false,
    refetchInterval: active && visible ? 30_000 : false,
    queryFn: ({ signal }) => request(async () => {
      // Every overview refresh also revalidates menu, picker and group status.
      await qc.cancelQueries({ queryKey: publicGroupKey(token), exact: true });
      void qc.invalidateQueries({ queryKey: publicGroupKey(token), exact: true });
      return unwrap(await api.GET("/api/v1/public/g/{token}/orders", {
        params: { path: { token } }, signal,
      }));
    }),
  });
  useEffect(() => {
    if (!active || !visible) void qc.cancelQueries({ queryKey: publicOrdersKey(token), exact: true });
  }, [active, visible, qc, token]);
  return { ...query, refresh: async () => {
    await qc.cancelQueries({ queryKey: publicOrdersKey(token), exact: true });
    return query.refetch();
  } };
}

export function useEffectiveOrder(token: string, pid: string | null) {
  const request = usePublicRequest();
  return useQuery({
    queryKey: effectiveOrderKey(token, pid ?? ""),
    enabled: !!pid,
    refetchOnWindowFocus: false,
    staleTime: 0,
    gcTime: 0,
    retry: retryPublic,
    queryFn: ({ signal }) => request(async () =>
      unwrap(
        await api.GET("/api/v1/public/g/{token}/participants/{pid}/order", {
          params: { path: { token, pid: pid! } },
          signal,
        }),
      )),
  });
}

export function useSubmitOrder(token: string, pid: string) {
  const request = usePublicRequest();
  const access = useContext(PublicAccessContext);
  const qc = useQueryClient();
  const queryKey = effectiveOrderKey(token, pid);
  return useMutation({
    mutationKey: orderSubmissionKey(token, pid),
    onMutate: () => qc.cancelQueries({ queryKey, exact: true }),
    mutationFn: (body: SubmitOrderBody) => request(async () => {
      const data = await unwrap(
        await api.POST("/api/v1/public/g/{token}/participants/{pid}/submissions", {
          params: { path: { token, pid } },
          body,
        }),
      );
      // A successful write must include its authoritative snapshot; never cache a false empty order.
      if (!data.order) throw new Error("未取得已儲存的訂單，請重試");
      return data;
    }),
    onSuccess: async (data) => {
      // Cutoff can start another GET while POST is pending. Cancel it before publishing success.
      await qc.cancelQueries({ queryKey, exact: true });
      if (!access.isBlocked()) qc.setQueryData(queryKey, data);
    },
    onSettled: async () => {
      if (access.isBlocked()) return;
      // Revalidate after either outcome, never reuse a read started before the write settled.
      await qc.cancelQueries({ queryKey, exact: true });
      await qc.cancelQueries({ queryKey: publicOrdersKey(token), exact: true });
      await qc.cancelQueries({ queryKey: publicGroupKey(token), exact: true });
      await Promise.all([
        qc.invalidateQueries({ queryKey, exact: true }),
        qc.invalidateQueries({ queryKey: publicGroupKey(token) }),
        qc.invalidateQueries({ queryKey: publicOrdersKey(token), exact: true }),
      ]);
    },
  });
}
