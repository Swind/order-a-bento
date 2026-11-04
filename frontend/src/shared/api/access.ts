import type { QueryClient } from "@tanstack/react-query";
import { ApiError } from "./client";

export function invalidateSharingQueries(qc: QueryClient) {
  // Membership changes affect listings, already-open details, and import permissions.
  return Promise.all(
    ["sharing-groups", "sharing-group", "sharing-members", "rosters", "roster", "vendors", "vendor", "import-run", "group-orders", "group-order"]
      .map((key) => qc.invalidateQueries({ queryKey: [key] })),
  );
}

export function invalidateSharingAccessOnError(qc: QueryClient, error: unknown) {
  if (error instanceof ApiError && (error.status === 403 || error.status === 404)) {
    return invalidateSharingQueries(qc);
  }
}
