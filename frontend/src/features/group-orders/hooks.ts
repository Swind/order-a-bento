import { useMutation, useQuery, useQueryClient, type QueryClient } from "@tanstack/react-query";
import { api, unwrap, ApiError } from "@/shared/api/client";
import { invalidateSharingAccessOnError } from "@/features/sharing-groups/hooks";
import type { GroupOrderDetail, GroupSummary, ParticipantRow, PatchGroupOrderBody } from "@/shared/api/types";

export interface CreateGroupOrderInput {
  sharingGroupId?: string | null;
  deliveryAddressId?: string;
  title: string;
  rosterId: string;
  vendorId: string;
  deadlineAt?: string;
  note?: string;
}

export const groupsKey = ["group-orders"] as const;
export const groupOrderKey = (id: string) => ["group-order", id] as const;
export const summaryKey = (id: string) => ["group-order", id, "summary"] as const;
export const participantsKey = (id: string) => ["group-order", id, "participants"] as const;
export const historyKey = (id: string, pid: string) =>
  ["group-order", id, "history", pid] as const;

/** Organizer sees new orders live while the group is open (TanStack pauses it in hidden tabs). */
export const LIVE_REFETCH_MS = 15_000;
const live = (isOpen: boolean) => (isOpen ? LIVE_REFETCH_MS : false);

// Child reads can discover a removal before the detail query's next refresh.
async function readOrder<T>(qc: QueryClient, id: string, request: () => Promise<T>, detail = false): Promise<T> {
  try {
    return await request();
  } catch (error) {
    if (error instanceof ApiError && error.status === 404) {
      if (detail) {
        const children = { queryKey: groupOrderKey(id), predicate: (query: { queryKey: readonly unknown[] }) => query.queryKey.length > 2 };
        await qc.cancelQueries(children);
        qc.removeQueries(children);
      } else {
        void qc.invalidateQueries({ queryKey: groupOrderKey(id), exact: true });
      }
      void qc.invalidateQueries({ queryKey: groupsKey });
      void qc.invalidateQueries({ queryKey: ["sharing-groups"] });
    }
    throw error;
  }
}

export function useCreateGroupOrder() {
  const qc = useQueryClient();
  return useMutation({
    onError: (error) => invalidateSharingAccessOnError(qc, error),
    mutationFn: async (body: CreateGroupOrderInput) =>
      unwrap(await api.POST("/api/v1/group-orders", { body })),
    onSuccess: (data) => {
      qc.setQueryData(groupOrderKey(data.id), data);
      qc.invalidateQueries({ queryKey: groupsKey });
    },
  });
}

export function useGroups() {
  return useQuery({
    queryKey: groupsKey,
    queryFn: async () => unwrap(await api.GET("/api/v1/group-orders")),
  });
}

export function useGroupOrder(id: string) {
  const qc = useQueryClient();
  return useQuery<GroupOrderDetail>({
    queryKey: groupOrderKey(id),
    retry: false,
    refetchInterval: (query) => query.state.error ? false : live(query.state.data?.status === "open"),
    queryFn: ({ signal }) => readOrder(qc, id, async () =>
      unwrap(await api.GET("/api/v1/group-orders/{id}", { params: { path: { id } }, signal })), true),
  });
}

export function useSummary(id: string, isOpen: boolean) {
  const qc = useQueryClient();
  return useQuery<GroupSummary>({
    queryKey: summaryKey(id),
    retry: false,
    refetchInterval: (query) => query.state.error ? false : live(isOpen),
    queryFn: ({ signal }) => readOrder(qc, id, async () =>
      unwrap(await api.GET("/api/v1/group-orders/{id}/summary", { params: { path: { id } }, signal }))),
  });
}

export function useParticipants(id: string, isOpen: boolean) {
  const qc = useQueryClient();
  return useQuery<ParticipantRow[]>({
    queryKey: participantsKey(id),
    retry: false,
    refetchInterval: (query) => query.state.error ? false : live(isOpen),
    queryFn: ({ signal }) => readOrder(qc, id, async () =>
      unwrap(await api.GET("/api/v1/group-orders/{id}/participants", { params: { path: { id } }, signal }))),
  });
}

export function useHistory(id: string, pid: string | null) {
  const qc = useQueryClient();
  return useQuery({
    queryKey: historyKey(id, pid ?? ""),
    enabled: !!pid,
    gcTime: 0,
    retry: false,
    queryFn: ({ signal }) => readOrder(qc, id, async () =>
      unwrap(
        await api.GET("/api/v1/group-orders/{id}/participants/{pid}/submissions", {
          params: { path: { id, pid: pid! } }, signal,
        }),
      )),
  });
}

function useInvalidateAfterChange(id: string) {
  const qc = useQueryClient();
  return () =>
    Promise.all([
      qc.invalidateQueries({ queryKey: groupsKey }),
      qc.invalidateQueries({ queryKey: summaryKey(id) }),
      qc.invalidateQueries({ queryKey: participantsKey(id) }),
    ]);
}

export function usePatchGroup(id: string) {
  const qc = useQueryClient();
  const invalidate = useInvalidateAfterChange(id);
  return useMutation({
    onError: (error) => invalidateSharingAccessOnError(qc, error),
    mutationFn: async (body: PatchGroupOrderBody) =>
      unwrap(
        await api.PATCH("/api/v1/group-orders/{id}", { params: { path: { id } }, body }),
      ),
    onSuccess: (data) => {
      qc.setQueryData(groupOrderKey(id), data);
      return invalidate();
    },
  });
}

export function useRotateShareToken(id: string) {
  const qc = useQueryClient();
  return useMutation({
    onError: (error) => invalidateSharingAccessOnError(qc, error),
    mutationFn: async () =>
      unwrap(
        await api.POST("/api/v1/group-orders/{id}/share-token/rotate", {
          params: { path: { id } },
        }),
      ),
    onSuccess: (data) => qc.setQueryData(groupOrderKey(id), data),
  });
}

export function useAddParticipant(id: string) {
  const qc = useQueryClient();
  const invalidate = useInvalidateAfterChange(id);
  return useMutation({
    onError: (error) => invalidateSharingAccessOnError(qc, error),
    mutationFn: async (name: string) =>
      unwrap(
        await api.POST("/api/v1/group-orders/{id}/participants", {
          params: { path: { id } },
          body: { name },
        }),
      ),
    onSuccess: (data) => {
      qc.setQueryData(groupOrderKey(id), data);
      return invalidate();
    },
  });
}

export function useSetSoldOut(id: string) {
  const qc = useQueryClient();
  return useMutation({
    onError: (error) => invalidateSharingAccessOnError(qc, error),
    mutationFn: async (v: { itemId: string; soldOut: boolean }) =>
      unwrap(
        await api.PATCH("/api/v1/group-orders/{id}/offered-items/{itemId}", {
          params: { path: { id, itemId: v.itemId } },
          body: { soldOut: v.soldOut },
        }),
      ),
    onSuccess: (data) => qc.setQueryData(groupOrderKey(id), data),
  });
}

/** Optimistic pickup / payment toggle with rollback on error. */
export function useSettlement(id: string) {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: async (v: { pid: string; pickedUp: boolean; paid: boolean }) =>
      unwrap(
        await api.PUT("/api/v1/group-orders/{id}/participants/{pid}/settlement", {
          params: { path: { id, pid: v.pid } },
          body: { pickedUp: v.pickedUp, paid: v.paid },
        }),
      ),
    onMutate: async (v) => {
      await qc.cancelQueries({ queryKey: participantsKey(id) });
      const previous = qc.getQueryData<ParticipantRow[]>(participantsKey(id));
      qc.setQueryData<ParticipantRow[]>(participantsKey(id), (rows) =>
        rows?.map((r) =>
          r.id !== v.pid
            ? r
            : {
                ...r,
                pickedUp: v.pickedUp,
                paid: v.paid,
                paidAmount: v.paid ? (r.paid ? r.paidAmount : r.amountDue) : null,
                balance: v.paid ? (r.paid ? r.balance : 0) : null,
              },
        ),
      );
      return { previous };
    },
    onError: (error, _v, ctx) => {
      if (ctx?.previous) qc.setQueryData(participantsKey(id), ctx.previous);
      return invalidateSharingAccessOnError(qc, error);
    },
    onSettled: () =>
      Promise.all([
        qc.invalidateQueries({ queryKey: participantsKey(id) }),
        qc.invalidateQueries({ queryKey: summaryKey(id) }),
      ]),
  });
}

export function useOrganizerSubmit(id: string, pid: string) {
  const qc = useQueryClient();
  const invalidate = useInvalidateAfterChange(id);
  return useMutation({
    onError: (error) => invalidateSharingAccessOnError(qc, error),
    mutationFn: async (body: {
      lines: { itemId: string; quantity: number; note?: string }[];
      note?: string;
      idempotencyKey: string;
    }) =>
      unwrap(
        await api.POST("/api/v1/group-orders/{id}/participants/{pid}/submissions", {
          params: { path: { id, pid } },
          body,
        }),
      ),
    onSuccess: () =>
      Promise.all([invalidate(), qc.invalidateQueries({ queryKey: historyKey(id, pid) })]),
  });
}
