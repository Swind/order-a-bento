import { createElement, type ReactNode } from "react";
import { act, renderHook, waitFor } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { http, HttpResponse } from "msw";
import { describe, expect, it } from "vitest";
import { server, problem } from "@/test/server";
import { useRoster, useRosters } from "@/features/rosters/hooks";
import { useVendor, useVendors } from "@/features/vendors/hooks";
import { useImportRun } from "@/features/ai-import/hooks";
import { useSharingGroup, useSharingGroups, useSharingMembers, useSharingMutation, useSharingUserLookup, type SharingCommand } from "@/features/sharing-groups/hooks";

function setup() {
  const qc = new QueryClient({ defaultOptions: { queries: { retry: false }, mutations: { retry: false } } });
  const wrapper = ({ children }: { children: ReactNode }) => createElement(QueryClientProvider, { client: qc }, children);
  return { qc, wrapper };
}

describe("query transport and sharing commands", () => {
  it.each([
    { path: "sharing-groups", key: ["sharing-groups"], useHook: () => useSharingGroups() },
    { path: "sharing-groups/g1", key: ["sharing-group", "g1"], useHook: () => useSharingGroup("g1") },
    { path: "sharing-groups/g1/members", key: ["sharing-members", "g1"], useHook: () => useSharingMembers("g1") },
    { path: "rosters", key: ["rosters", "g1"], useHook: () => useRosters("g1") },
    { path: "rosters/r1", key: ["roster", "r1"], useHook: () => useRoster("r1") },
    { path: "vendors", key: ["vendors", "g1"], useHook: () => useVendors("g1") },
    { path: "vendors/v1", key: ["vendor", "v1"], useHook: () => useVendor("v1") },
    { path: "ai/import-runs/run1", key: ["import-run", "run1"], useHook: () => useImportRun("run1") },
  ])("aborts $path and never caches its late response", async ({ path, key, useHook }) => {
    const { qc, wrapper } = setup();
    let received: Request | undefined;
    let release!: () => void;
    const gate = new Promise<void>((resolve) => { release = resolve; });
    server.use(http.get(`*/api/v1/${path}`, async ({ request }) => {
      received = request;
      await gate;
      return HttpResponse.json({ id: "late-private-data" });
    }));
    const hook = renderHook(() => { useHook(); }, { wrapper });
    await waitFor(() => expect(received).toBeDefined());
    await act(() => qc.cancelQueries({ queryKey: key }));
    expect(received?.signal.aborted).toBe(true);
    await act(async () => { release(); await gate; });
    expect(qc.getQueryData(key)).toBeUndefined();
    hook.unmount();
    qc.clear();
  });

  it.each<{ command: SharingCommand; method: "post" | "patch" | "delete"; path: string; body?: object }>([
    { command: { type: "create", body: { name: "Team" } }, method: "post", path: "sharing-groups", body: { name: "Team" } },
    { command: { type: "rename", groupId: "g1", body: { name: "Renamed" } }, method: "patch", path: "sharing-groups/g1", body: { name: "Renamed" } },
    { command: { type: "addMember", groupId: "g1", body: { userId: "u1", role: "member" } }, method: "post", path: "sharing-groups/g1/members", body: { userId: "u1", role: "member" } },
    { command: { type: "changeRole", groupId: "g1", userId: "u1", body: { role: "admin" } }, method: "patch", path: "sharing-groups/g1/members/u1", body: { role: "admin" } },
    { command: { type: "removeMember", groupId: "g1", userId: "u1" }, method: "delete", path: "sharing-groups/g1/members/u1" },
  ])("sends $command.type with the exact body and invalidates scoped data", async ({ command, method, path, body }) => {
    const { qc, wrapper } = setup();
    qc.setQueryData(["vendor", "cached"], {});
    let received = false;
    server.use(http[method](`*/api/v1/${path}`, async ({ request }) => {
      received = true;
      expect(request.signal.aborted).toBe(false);
      if (body) expect(await request.json()).toEqual(body);
      else expect(await request.text()).toBe("");
      return command.type === "create" || command.type === "rename"
        ? HttpResponse.json({ id: "g1", name: "Team", role: "admin" }, { status: command.type === "create" ? 201 : 200 })
        : new HttpResponse(null, { status: 204 });
    }));
    const hook = renderHook(() => useSharingMutation(), { wrapper });
    await act(async () => { await hook.result.current.mutateAsync(command); });
    expect(received).toBe(true);
    expect(qc.getQueryState(["vendor", "cached"])?.isInvalidated).toBe(true);
    hook.unmount();
    qc.clear();
  });

  it("looks up a trimmed email as an encoded query, not a path", async () => {
    const { qc, wrapper } = setup();
    server.use(http.get("*/api/v1/sharing-groups/g1/users/lookup", ({ request }) => {
      expect(new URL(request.url).searchParams.get("email")).toBe("ann+team@example.com");
      return HttpResponse.json([]);
    }));
    const hook = renderHook(() => useSharingUserLookup("g1"), { wrapper });
    await act(async () => { expect(await hook.result.current.mutateAsync(" ann+team@example.com ")).toEqual([]); });
    hook.unmount();
    qc.clear();
  });

  it.each([403, 404])("withdraws all access-dependent families on mutation %s", async (status) => {
    const { qc, wrapper } = setup();
    const families = ["sharing-groups", "sharing-group", "sharing-members", "rosters", "roster", "vendors", "vendor", "import-run", "group-orders", "group-order"];
    for (const family of families) qc.setQueryData([family, "cached"], {});
    server.use(http.delete("*/api/v1/sharing-groups/g1/members/u1", () => problem(status, "access_revoked")));
    const hook = renderHook(() => useSharingMutation(), { wrapper });
    await act(async () => {
      await expect(hook.result.current.mutateAsync({ type: "removeMember", groupId: "g1", userId: "u1" })).rejects.toMatchObject({ status });
    });
    for (const family of families) expect(qc.getQueryState([family, "cached"])?.isInvalidated).toBe(true);
    hook.unmount();
    qc.clear();
  });
});
