import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { api, toApiError, unwrap } from "@/shared/api/client";
import type { AcceptedChange, ImportMode, ImportRun } from "@/shared/api/types";

const runKey = (id: string) => ["import-run", id] as const;

export function useImportRun(runId: string) {
  return useQuery({
    queryKey: runKey(runId),
    queryFn: async () =>
      unwrap(await api.GET("/api/v1/ai/import-runs/{runId}", { params: { path: { runId } } })),
  });
}

export interface StartImportInput {
  image: Blob;
  instruction?: string;
  mode: ImportMode;
  targetRosterId?: string;
  targetVendorId?: string;
  signal?: AbortSignal;
}

/** Multipart upload; non-ok responses become `ApiError` (with `runId` / `errors`). */
export async function startImport(input: StartImportInput): Promise<ImportRun> {
  const fd = new FormData();
  fd.append("image", input.image, "photo.jpg");
  if (input.instruction) fd.append("instruction", input.instruction);
  fd.append("mode", input.mode);
  if (input.targetRosterId) fd.append("targetRosterId", input.targetRosterId);
  if (input.targetVendorId) fd.append("targetVendorId", input.targetVendorId);
  const base = typeof location !== "undefined" ? location.origin : "";
  const res = await globalThis.fetch(`${base}/api/v1/ai/import-runs`, {
    method: "POST",
    body: fd,
    credentials: "include",
    signal: input.signal,
  });
  if (!res.ok) throw await toApiError(res);
  return (await res.json()) as ImportRun;
}

export function useStartImport() {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: startImport,
    onSuccess: (run) => qc.setQueryData(runKey(run.id), run),
  });
}

export function useApplyImport(runId: string) {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: async (accepted: AcceptedChange[]) =>
      unwrap(
        await api.POST("/api/v1/ai/import-runs/{runId}/apply", {
          params: { path: { runId } },
          body: { accepted },
        }),
      ),
    onSuccess: () => {
      qc.invalidateQueries({ queryKey: runKey(runId) });
      qc.invalidateQueries({ queryKey: ["rosters"] });
      qc.invalidateQueries({ queryKey: ["roster"] });
      qc.invalidateQueries({ queryKey: ["vendors"] });
      qc.invalidateQueries({ queryKey: ["vendor"] });
    },
  });
}

export function useDiscardImport(runId: string) {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: async () =>
      unwrap(
        await api.POST("/api/v1/ai/import-runs/{runId}/discard", { params: { path: { runId } } }),
      ),
    onSuccess: () => qc.invalidateQueries({ queryKey: runKey(runId) }),
  });
}
