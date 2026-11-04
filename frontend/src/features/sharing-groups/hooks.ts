import { useMutation, useQueries, useQuery, useQueryClient } from "@tanstack/react-query";
import { api, unwrap } from "@/shared/api/client";
import { invalidateSharingAccessOnError, invalidateSharingQueries } from "@/shared/api/access";
import { listRosters, listVendors } from "@/shared/api/directory";
import type { components } from "@/shared/api/schema";
import type { RosterSummary, VendorSummary } from "@/shared/api/types";

export function useSharingGroups() {
  return useQuery({ queryKey: ["sharing-groups"], queryFn: async ({ signal }) => unwrap(await api.GET("/api/v1/sharing-groups", { signal })) });
}

export function useSharingGroup(id: string) {
  return useQuery({ queryKey: ["sharing-group", id], queryFn: async ({ signal }) => unwrap(await api.GET("/api/v1/sharing-groups/{groupId}", { params: { path: { groupId: id } }, signal })) });
}

export function useSharingMembers(id: string) {
  return useQuery({ queryKey: ["sharing-members", id], queryFn: async ({ signal }) => unwrap(await api.GET("/api/v1/sharing-groups/{groupId}/members", { params: { path: { groupId: id } }, signal })) });
}

type S = components["schemas"];
export type SharingCommand =
  | { type: "create"; body: S["GroupNameRequest"] }
  | { type: "rename"; groupId: string; body: S["GroupNameRequest"] }
  | { type: "addMember"; groupId: string; body: S["AddSharingMemberRequest"] }
  | { type: "changeRole"; groupId: string; userId: string; body: S["SharingRoleRequest"] }
  | { type: "removeMember"; groupId: string; userId: string };

export function useSharingMutation() {
  const qc = useQueryClient();
  return useMutation({
    onError: (error) => invalidateSharingAccessOnError(qc, error),
    mutationFn: async (command: SharingCommand) => {
      switch (command.type) {
        case "create": return unwrap(await api.POST("/api/v1/sharing-groups", { body: command.body }));
        case "rename": return unwrap(await api.PATCH("/api/v1/sharing-groups/{groupId}", { params: { path: { groupId: command.groupId } }, body: command.body }));
        case "addMember": return unwrap(await api.POST("/api/v1/sharing-groups/{groupId}/members", { params: { path: { groupId: command.groupId } }, body: command.body }));
        case "changeRole": return unwrap(await api.PATCH("/api/v1/sharing-groups/{groupId}/members/{userId}", { params: { path: { groupId: command.groupId, userId: command.userId } }, body: command.body }));
        case "removeMember": return unwrap(await api.DELETE("/api/v1/sharing-groups/{groupId}/members/{userId}", { params: { path: { groupId: command.groupId, userId: command.userId } } }));
      }
    },
    onSuccess: () => invalidateSharingQueries(qc),
  });
}

export function useSharingUserLookup(id: string) {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: async (email: string) => unwrap(await api.GET("/api/v1/sharing-groups/{groupId}/users/lookup", {
      params: { path: { groupId: id }, query: { email: email.trim() } },
    })),
    onError: (error) => invalidateSharingAccessOnError(qc, error),
  });
}

export function useAccessibleRosters() { return useAccessibleDirectory("rosters", listRosters); }
export function useAccessibleVendors() { return useAccessibleDirectory("vendors", listVendors); }

function useAccessibleDirectory<T extends RosterSummary | VendorSummary>(resource: "rosters" | "vendors", list: (scope?: string, signal?: AbortSignal) => Promise<T[]>) {
  const groups = useSharingGroups();
  const scopes = [undefined, ...(groups.data ?? []).map((group) => group.id)];
  const queries = useQueries({ queries: scopes.map((scope) => ({
    queryKey: [resource, scope ?? "personal"],
    queryFn: ({ signal }: { signal: AbortSignal }) => list(scope, signal),
  })) });
  return {
    data: queries.flatMap((query, index) => (query.isError ? [] : query.data ?? []).map((item) => ({
      ...item,
      sourceLabel: scopes[index] ? groups.data?.find((group) => group.id === scopes[index])?.name ?? "共用群組" : "個人",
    }))),
    isPending: groups.isPending || queries.some((query) => query.isPending),
    isError: groups.isError || queries.some((query) => query.isError),
    error: groups.error ?? queries.find((query) => query.error)?.error,
    refetch: () => { groups.refetch(); queries.forEach((query) => query.refetch()); },
  };
}
