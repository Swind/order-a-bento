import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { api, unwrap } from "@/shared/api/client";

const key = ["delivery-addresses"] as const;

export function useDeliveryAddresses() {
  return useQuery({
    queryKey: key,
    staleTime: 0, // Refresh when returning from the address-management tab.
    queryFn: async () => unwrap(await api.GET("/api/v1/delivery-addresses")),
  });
}

export function useSaveDeliveryAddress() {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: async ({ id, ...body }: { id?: string; name: string; address: string }) =>
      id
        ? unwrap(await api.PUT("/api/v1/delivery-addresses/{id}", { params: { path: { id } }, body }))
        : unwrap(await api.POST("/api/v1/delivery-addresses", { body })),
    onSuccess: () => qc.invalidateQueries({ queryKey: key }),
  });
}

export function useDeleteDeliveryAddress() {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: async (id: string) =>
      unwrap(await api.DELETE("/api/v1/delivery-addresses/{id}", { params: { path: { id } } })),
    onSuccess: () => qc.invalidateQueries({ queryKey: key }),
  });
}
