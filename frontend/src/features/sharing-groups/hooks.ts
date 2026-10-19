import { useMutation, useQueries, useQuery, useQueryClient, type QueryClient } from "@tanstack/react-query";
import { ApiError, toApiError } from "@/shared/api/client";
import type { RosterSummary, VendorSummary, SharingGroup, SharingGroupDetail, SharingMember } from "@/shared/api/types";

export interface DirectoryScope {
  sharingGroupId?: string | null;
  canManage?: boolean;
}

export async function sharingRequest<T>(path: string, method = "GET", body?: unknown): Promise<T> {
  const response = await fetch(`${location.origin}/api/v1/${path}`, {
    method,
    credentials: "include",
    ...(body === undefined ? {} : { headers: { "Content-Type": "application/json" }, body: JSON.stringify(body) }),
  });
  if (!response.ok) throw await toApiError(response);
  return response.status === 204 ? undefined as T : response.json();
}

export function scopePath(resource: string, sharingGroupId?: string) {
  return `${resource}${sharingGroupId ? `?sharingGroupId=${encodeURIComponent(sharingGroupId)}` : ""}`;
}

export function useSharingGroups() {
  return useQuery({ queryKey: ["sharing-groups"], queryFn: () => sharingRequest<SharingGroup[]>("sharing-groups") });
}

export function useSharingGroup(id: string) {
  return useQuery({ queryKey: ["sharing-group", id], queryFn: () => sharingRequest<SharingGroupDetail>(`sharing-groups/${id}`) });
}

export function useSharingMembers(id: string) {
  return useQuery({ queryKey: ["sharing-members", id], queryFn: () => sharingRequest<SharingMember[]>(`sharing-groups/${id}/members`) });
}

function invalidateSharingQueries(qc: QueryClient) {
  // Membership changes affect listings, already-open details, and import permissions.
  return Promise.all(
    ["sharing-groups", "sharing-group", "sharing-members", "rosters", "roster", "vendors", "vendor", "import-run"]
      .map((key) => qc.invalidateQueries({ queryKey: [key] })),
  );
}

export function invalidateSharingAccessOnError(qc: QueryClient, error: unknown) {
  if (error instanceof ApiError && (error.status === 403 || error.status === 404)) {
    return invalidateSharingQueries(qc);
  }
}

export function useSharingMutation(id?: string) {
  const qc = useQueryClient();
  return useMutation({
    onError: (error) => invalidateSharingAccessOnError(qc, error),
    mutationFn: ({ suffix = "", method = "POST", body }: { suffix?: string; method?: string; body?: unknown }) =>
      sharingRequest<SharingGroup>(`sharing-groups${id ? `/${id}` : ""}${suffix}`, method, body),
    onSuccess: () => invalidateSharingQueries(qc),
  });
}

export function useAccessibleDirectory<T extends RosterSummary | VendorSummary>(resource: "rosters" | "vendors") {
  const groups = useSharingGroups();
  const scopes = [undefined, ...(groups.data ?? []).map((group) => group.id)];
  const queries = useQueries({ queries: scopes.map((scope) => ({
    queryKey: [resource, scope ?? "personal"],
    queryFn: () => sharingRequest<(T & DirectoryScope)[]>(scopePath(resource, scope)),
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
