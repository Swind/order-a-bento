import { QueryClient } from "@tanstack/react-query";
import { describe, expect, it } from "vitest";
import { ApiError } from "@/shared/api/client";
import { invalidateSharingAccessOnError } from "./hooks";

const families = ["sharing-groups", "sharing-group", "sharing-members", "rosters", "roster", "vendors", "vendor", "import-run"];

describe("sharing access invalidation", () => {
  it.each([403, 404])("invalidates every access-dependent family on %s", async (status) => {
    const qc = new QueryClient();
    for (const family of families) qc.setQueryData([family, "cached"], {});
    qc.setQueryData(["group-orders"], []);
    await invalidateSharingAccessOnError(qc, new ApiError({ type: "", title: "", status }));
    for (const family of families) expect(qc.getQueryState([family, "cached"])?.isInvalidated).toBe(true);
    expect(qc.getQueryState(["group-orders"])?.isInvalidated).toBe(false);
    qc.clear();
  });

  it.each([409, 422, 500])("does not invalidate permissions on %s", async (status) => {
    const qc = new QueryClient();
    for (const family of families) qc.setQueryData([family, "cached"], {});
    await invalidateSharingAccessOnError(qc, new ApiError({ type: "", title: "", status }));
    for (const family of families) expect(qc.getQueryState([family, "cached"])?.isInvalidated).toBe(false);
    qc.clear();
  });
});
