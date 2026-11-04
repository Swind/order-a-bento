import { describe, expect, it, vi } from "vitest";
import { http, HttpResponse } from "msw";
import { server, problem } from "@/test/server";
import { api, ApiError, unwrap } from "./client";
import { listRosters, listVendors } from "./directory";
import type { SharingCommand } from "@/features/sharing-groups/hooks";

describe("typed API contract", () => {
  it("rejects wrong endpoint, path, body and commands at compile time", () => {
    const contracts = () => {
      // @ts-expect-error Only generated paths are accepted.
      void api.GET("/api/v1/not-an-endpoint");
      // @ts-expect-error A vendor path requires vendorId, not rosterId.
      void api.GET("/api/v1/vendors/{vendorId}", { params: { path: { rosterId: "r1" } } });
      // @ts-expect-error Sharing create takes a name, not a delivery address.
      void api.POST("/api/v1/sharing-groups", { body: { address: "wrong" } });
      // @ts-expect-error The scope is a query parameter, not a path parameter.
      void api.GET("/api/v1/rosters", { params: { path: { sharingGroupId: "g1" } } });
      // @ts-expect-error A turn requires its frozen revision and vendors.
      void api.POST("/api/v1/ai/import-runs/{runId}/turns", { params: { path: { runId: "run1" } }, body: { instruction: "edit", turnId: "t1" } });
      // @ts-expect-error Arbitrary suffix/method mutations are not commands.
      const suffix: SharingCommand = { suffix: "/members", method: "POST", body: {} };
      // @ts-expect-error A role mutation requires a generated role value.
      const role: SharingCommand = { type: "changeRole", groupId: "g1", userId: "u1", body: { role: "owner" } };
      return [suffix, role];
    };
    expect(contracts).toBeTypeOf("function");
  });

  it.each([
    { resource: "rosters", list: listRosters },
    { resource: "vendors", list: listVendors },
  ])("encodes $resource scope and includes credentials", async ({ resource, list }) => {
    const scope = "group/+?&中文";
    server.use(http.get(`*/api/v1/${resource}`, ({ request }) => {
      expect(new URL(request.url).searchParams.get("sharingGroupId")).toBe(scope);
      return HttpResponse.json([]);
    }));
    const fetch = vi.spyOn(globalThis, "fetch");
    try {
      expect(await list(scope)).toEqual([]);
      // MSW reconstructs handler requests; inspect the real fetch boundary instead.
      expect(fetch.mock.calls[0][0]).toHaveProperty("credentials", "include");
    } finally { fetch.mockRestore(); }
  });

  it("unwraps an empty 204 without trying to parse JSON", async () => {
    server.use(http.delete("*/api/v1/sharing-groups/g1/members/u1", () => new HttpResponse(null, { status: 204 })));
    expect(await unwrap(await api.DELETE("/api/v1/sharing-groups/{groupId}/members/{userId}", {
      params: { path: { groupId: "g1", userId: "u1" } },
    }))).toBeUndefined();
  });

  it.each([401, 403, 404, 409, 429, 503])("retains problem fields on %s", async (status) => {
    const extra = { field: "name", existingId: "v1", runId: "run1", errors: [{ path: "vendors[0].name", message: "invalid name" }] };
    server.use(http.get("*/api/v1/vendors/v1", () => problem(status, "contract_error", extra)));
    await expect(unwrap(await api.GET("/api/v1/vendors/{vendorId}", { params: { path: { vendorId: "v1" } } })))
      .rejects.toMatchObject({ name: "ApiError", status, code: "contract_error", ...extra });
  });

  it("keeps a non-JSON upstream failure as ApiError", async () => {
    server.use(http.get("*/api/v1/vendors/v1", () => new HttpResponse("upstream down", { status: 502, statusText: "Bad Gateway", headers: { "Content-Type": "text/plain" } })));
    await expect(unwrap(await api.GET("/api/v1/vendors/{vendorId}", { params: { path: { vendorId: "v1" } } })))
      .rejects.toBeInstanceOf(ApiError);
  });
});
