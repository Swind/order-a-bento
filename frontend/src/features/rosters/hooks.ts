import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { api, unwrap } from "@/shared/api/client";
import type { MemberInput } from "@/shared/api/types";

const listKey = ["rosters"] as const;
const detailKey = (id: string) => ["roster", id] as const;

export function useRosters() {
  return useQuery({
    queryKey: listKey,
    queryFn: async () => unwrap(await api.GET("/api/v1/rosters")),
  });
}

export function useRoster(id: string) {
  return useQuery({
    queryKey: detailKey(id),
    queryFn: async () =>
      unwrap(await api.GET("/api/v1/rosters/{rosterId}", { params: { path: { rosterId: id } } })),
  });
}

export function useCreateRoster() {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: async (body: { name: string; members?: MemberInput[] }) =>
      unwrap(await api.POST("/api/v1/rosters", { body })),
    onSuccess: () => qc.invalidateQueries({ queryKey: listKey }),
  });
}

export function useRenameRoster(id: string) {
  const qc = useQueryClient();
  return useMutation({
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
