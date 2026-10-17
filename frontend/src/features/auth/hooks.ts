import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { api, unwrap } from "@/shared/api/client";

export const meKey = ["me"] as const;

export function useMe() {
  return useQuery({
    queryKey: meKey,
    queryFn: async () => unwrap(await api.GET("/api/v1/me")),
    staleTime: 60_000,
  });
}

export function useAuthConfig() {
  return useQuery({
    queryKey: ["auth-config"],
    queryFn: async () => unwrap(await api.GET("/api/v1/auth/config")),
    staleTime: Infinity,
  });
}

export function useDevLogin() {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: async (body: { email: string; displayName?: string }) =>
      unwrap(await api.POST("/auth/dev-login", { body })),
    onSuccess: () => qc.invalidateQueries({ queryKey: meKey }),
  });
}

export function usePasswordLogin() {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: async (body: { email: string; password: string }) =>
      unwrap(await api.POST("/auth/password-login", { body })),
    onSuccess: () => qc.invalidateQueries({ queryKey: meKey }),
  });
}

export function useLogout() {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: async () => unwrap(await api.POST("/auth/logout")),
    onSettled: () => qc.clear(),
  });
}
