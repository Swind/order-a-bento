import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { api, unwrap } from "@/shared/api/client";
import type { MenuItemInput, VendorSummary, VendorDetail } from "@/shared/api/types";
import { invalidateSharingAccessOnError, scopePath, sharingRequest } from "@/features/sharing-groups/hooks";

const listKey = ["vendors"] as const;
const detailKey = (id: string) => ["vendor", id] as const;

export interface VendorFields {
  name: string;
  phone?: string | null;
  address?: string | null;
  note?: string | null;
}

export function useVendors(sharingGroupId?: string) {
  return useQuery({
    queryKey: [...listKey, sharingGroupId || "personal"],
    queryFn: () => sharingRequest<VendorSummary[]>(scopePath("vendors", sharingGroupId)),
  });
}

export function useVendor(id: string) {
  return useQuery({
    queryKey: detailKey(id),
    queryFn: () => sharingRequest<VendorDetail>(`vendors/${id}`),
  });
}

export function useCreateVendor() {
  const qc = useQueryClient();
  return useMutation({
    onError: (error) => invalidateSharingAccessOnError(qc, error),
    mutationFn: (body: VendorFields & { sharingGroupId?: string }) => sharingRequest<VendorDetail>("vendors", "POST", body),
    onSuccess: () => qc.invalidateQueries({ queryKey: listKey }),
  });
}

export function usePatchVendor(id: string) {
  const qc = useQueryClient();
  return useMutation({
    onError: (error) => invalidateSharingAccessOnError(qc, error),
    mutationFn: async (body: Partial<VendorFields>) =>
      unwrap(
        await api.PATCH("/api/v1/vendors/{vendorId}", {
          params: { path: { vendorId: id } },
          body,
        }),
      ),
    onSuccess: (data) => {
      qc.setQueryData(detailKey(id), data);
      return qc.invalidateQueries({ queryKey: listKey });
    },
  });
}

export function useDeleteVendor(id: string) {
  const qc = useQueryClient();
  return useMutation({
    onError: (error) => invalidateSharingAccessOnError(qc, error),
    mutationFn: async () =>
      unwrap(await api.DELETE("/api/v1/vendors/{vendorId}", { params: { path: { vendorId: id } } })),
    onSuccess: () => {
      qc.removeQueries({ queryKey: detailKey(id) });
      return qc.invalidateQueries({ queryKey: listKey });
    },
  });
}

export function useUpsertMenuItems(id: string) {
  const qc = useQueryClient();
  return useMutation({
    onError: (error) => invalidateSharingAccessOnError(qc, error),
    mutationFn: async (items: MenuItemInput[]) =>
      unwrap(
        await api.POST("/api/v1/vendors/{vendorId}/menu-items/upsert", {
          params: { path: { vendorId: id } },
          body: { items },
        }),
      ),
    onSuccess: (data) => {
      qc.setQueryData(detailKey(id), data.vendor);
      return qc.invalidateQueries({ queryKey: listKey });
    },
  });
}

export interface ItemPatch {
  name?: string;
  price?: number;
  description?: string | null;
  category?: string | null;
  archived?: boolean;
}

export function usePatchMenuItem(id: string) {
  const qc = useQueryClient();
  return useMutation({
    onError: (error) => invalidateSharingAccessOnError(qc, error),
    mutationFn: async (v: { itemId: string } & ItemPatch) => {
      const { itemId, ...body } = v;
      return unwrap(
        await api.PATCH("/api/v1/vendors/{vendorId}/menu-items/{itemId}", {
          params: { path: { vendorId: id, itemId } },
          body,
        }),
      );
    },
    onSuccess: () => qc.invalidateQueries({ queryKey: detailKey(id) }),
  });
}

export function useDeleteMenuItem(id: string) {
  const qc = useQueryClient();
  return useMutation({
    onError: (error) => invalidateSharingAccessOnError(qc, error),
    mutationFn: async (itemId: string) =>
      unwrap(
        await api.DELETE("/api/v1/vendors/{vendorId}/menu-items/{itemId}", {
          params: { path: { vendorId: id, itemId } },
        }),
      ),
    onSuccess: () => {
      qc.invalidateQueries({ queryKey: listKey });
      return qc.invalidateQueries({ queryKey: detailKey(id) });
    },
  });
}
