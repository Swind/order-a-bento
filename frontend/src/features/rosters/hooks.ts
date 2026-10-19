import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { api, unwrap } from "@/shared/api/client";
import type { MemberInput, RosterSummary, RosterDetail } from "@/shared/api/types";
import { invalidateSharingAccessOnError, scopePath, sharingRequest } from "@/features/sharing-groups/hooks";

const listKey = ["rosters"] as const;
const detailKey = (id: string) => ["roster", id] as const;

export function useRosters(sharingGroupId?: string) {
  return useQuery({
    queryKey: [...listKey, sharingGroupId || "personal"],
    queryFn: () => sharingRequest<RosterSummary[]>(scopePath("rosters", sharingGroupId)),
  });
}

export function useRoster(id: string) {
  return useQuery({
    queryKey: detailKey(id),
    enabled: !!id,
    queryFn: () => sharingRequest<RosterDetail>(`rosters/${id}`),
  });
}

export function useCreateRoster() {
  const qc = useQueryClient();
  return useMutation({
    onError: (error) => invalidateSharingAccessOnError(qc, error),
    mutationFn: (body: { name: string; members?: MemberInput[]; sharingGroupId?: string }) =>
      sharingRequest<RosterDetail>("rosters", "POST", body),
    onSuccess: () => qc.invalidateQueries({ queryKey: listKey }),
  });
}

export function useRenameRoster(id: string) {
  const qc = useQueryClient();
  return useMutation({
    onError: (error) => invalidateSharingAccessOnError(qc, error),
    mutationFn: async (name: string) =>
      unwrap(
        await api.PATCH("/api/v1/rosters/{rosterId}", {
          params: { path: { rosterId: id } },
          body: { name },
        }),
      ),
    onSuccess: (data) => {
      qc.setQueryData(detailKey(id), data);
      return qc.invalidateQueries({ queryKey: listKey });
    },
  });
}

export function useDeleteRoster(id: string) {
  const qc = useQueryClient();
  return useMutation({
    onError: (error) => invalidateSharingAccessOnError(qc, error),
    mutationFn: async () =>
      unwrap(await api.DELETE("/api/v1/rosters/{rosterId}", { params: { path: { rosterId: id } } })),
    onSuccess: () => {
      qc.removeQueries({ queryKey: detailKey(id) });
      return qc.invalidateQueries({ queryKey: listKey });
    },
  });
}

export function useUpsertMembers(id: string) {
  const qc = useQueryClient();
  return useMutation({
    onError: (error) => invalidateSharingAccessOnError(qc, error),
    mutationFn: async (members: MemberInput[]) =>
      unwrap(
        await api.POST("/api/v1/rosters/{rosterId}/members/upsert", {
          params: { path: { rosterId: id } },
          body: { members },
        }),
      ),
    onSuccess: (data) => {
      qc.setQueryData(detailKey(id), data.roster);
      return qc.invalidateQueries({ queryKey: listKey });
    },
  });
}

export function usePatchMember(id: string) {
  const qc = useQueryClient();
  return useMutation({
    onError: (error) => invalidateSharingAccessOnError(qc, error),
    mutationFn: async (v: { memberId: string; name?: string; note?: string | null }) => {
      const { memberId, ...body } = v;
      return unwrap(
        await api.PATCH("/api/v1/rosters/{rosterId}/members/{memberId}", {
          params: { path: { rosterId: id, memberId } },
          body,
        }),
      );
    },
    onSuccess: () => qc.invalidateQueries({ queryKey: detailKey(id) }),
  });
}

export function useDeleteMember(id: string) {
  const qc = useQueryClient();
  return useMutation({
    onError: (error) => invalidateSharingAccessOnError(qc, error),
    mutationFn: async (memberId: string) =>
      unwrap(
        await api.DELETE("/api/v1/rosters/{rosterId}/members/{memberId}", {
          params: { path: { rosterId: id, memberId } },
        }),
      ),
    onSuccess: () => {
      qc.invalidateQueries({ queryKey: listKey });
      return qc.invalidateQueries({ queryKey: detailKey(id) });
    },
  });
}
