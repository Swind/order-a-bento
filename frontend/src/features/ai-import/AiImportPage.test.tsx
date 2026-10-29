import { screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { http, HttpResponse } from "msw";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { ImportRun } from "@/shared/api/types";
import { renderApp } from "@/test/render";
import { ME, problem, server } from "@/test/server";
import { AUTH_CONFIG, ROSTER_RUN, VENDOR_RUN } from "./fixtures";

const vendorDetail = {
  id: "v1",
  name: "好吃便當",
  phone: null,
  address: null,
  note: null,
  updatedAt: "2026-01-01T00:00:00Z",
  menuItems: [],
};

beforeEach(() =>
  server.use(
    http.get("*/api/v1/me", () => HttpResponse.json(ME)),
    http.get("*/api/v1/auth/config", () => HttpResponse.json(AUTH_CONFIG)),
    http.get("*/api/v1/vendors/v1", () => HttpResponse.json(vendorDetail)),
    http.get("*/api/v1/vendors", () => HttpResponse.json([])),
    http.get("*/api/v1/rosters", () => HttpResponse.json([])),
  ),
);

const useRun = (run: object) =>
  server.use(http.get("*/api/v1/ai/import-runs/run1", () => HttpResponse.json(run)));

describe("AiImportPage vendor proposal", () => {
  const pendingRun = (): ImportRun => ({
    ...VENDOR_RUN,
    hasPendingPhoto: true,
    pendingPhotoExpiresAt: new Date(Date.now() + 86_400_000).toISOString(),
    proposal: {
      warnings: [],
      targets: VENDOR_RUN.proposal.targets.map((t) => t.type === "vendor" ? { ...t, menu: t.menu.filter((m) => m.price !== null) } : t),
    },
  });

  it.each([false, true])("reloads pending metadata and applies an explicit photo choice (optOut=%s)", async (optOut) => {
    const run = pendingRun();
    useRun(run);
    let body: { savePhoto: boolean } | undefined;
    const put = vi.fn(() => HttpResponse.json({}));
    server.use(
      http.get("*/api/v1/vendors/v1", () => HttpResponse.json({ ...vendorDetail, image: { revision: "old" } })),
      http.put("*/api/v1/vendors/v1/image", put),
      http.post("*/api/v1/ai/import-runs/run1/apply", async ({ request }) => {
        body = await request.json() as typeof body;
        return HttpResponse.json({ rosterIds: [], vendorIds: ["v1"] });
      }),
    );
    const first = renderApp("/ai-import/run1");
    expect(await screen.findByRole("checkbox", { name: /儲存菜單照片/ })).toBeChecked();
    first.unmount();
    // A fresh app/query client has no File or local store: GET alone restores the choice.
    const { router } = renderApp("/ai-import/run1");
    const choice = await screen.findByRole("checkbox", { name: /儲存菜單照片/ });
    expect(choice).toBeChecked();
    expect(await screen.findByText("將取代目前參考照片，可取消勾選。")).toBeInTheDocument();
    if (optOut) await userEvent.click(choice);
    await userEvent.click(screen.getByRole("button", { name: /套用 \d+ 筆變更/ }));
    await waitFor(() => expect(router.state.location.pathname).toBe("/vendors/v1"));
    expect(body!.savePhoto).toBe(!optOut);
    expect(put).not.toHaveBeenCalled();
  });

  it("initializes a new vendor's pending photo without requiring an existing vendor", async () => {
    const run = pendingRun();
    run.targetVendorId = null;
    run.proposal.targets = run.proposal.targets.map((t) => t.type === "vendor" ? { ...t, vendor: { kind: "new", name: "新店" } } : t);
    useRun(run);
    renderApp("/ai-import/run1");
    expect(await screen.findByRole("checkbox", { name: /儲存菜單照片/ })).toBeChecked();
    expect(screen.getByRole("button", { name: /套用 \d+ 筆變更/ })).toBeEnabled();
    expect(screen.queryByText("將取代目前參考照片，可取消勾選。")).not.toBeInTheDocument();
  });

  it("uses the actual proposal vendor, not the upload entry vendor, for replacement warnings", async () => {
    const run = pendingRun();
    run.targetVendorId = "unrelated";
    useRun(run);
    server.use(http.get("*/api/v1/vendors/v1", () => HttpResponse.json({ ...vendorDetail, image: { revision: "old" } })));
    renderApp("/ai-import/run1");
    expect(await screen.findByText("將取代目前參考照片，可取消勾選。")).toBeInTheDocument();
  });

  it("blocks expired photo saving while allowing data-only import after unchecking", async () => {
    const run = pendingRun();
    // Exercise expiry after the page is already open, without fake-timing userEvent/MSW.
    run.pendingPhotoExpiresAt = new Date(Date.now() + 800).toISOString();
    useRun(run);
    let body: { savePhoto: boolean } | undefined;
    server.use(http.post("*/api/v1/ai/import-runs/run1/apply", async ({ request }) => {
      body = await request.json() as typeof body;
      return HttpResponse.json({ rosterIds: [], vendorIds: ["v1"] });
    }));
    renderApp("/ai-import/run1");
    const choice = await screen.findByRole("checkbox", { name: /儲存菜單照片/ });
    expect(choice).toBeChecked();
    await screen.findByText(/沒有待存照片/);
    expect(screen.getByRole("button", { name: /套用 \d+ 筆變更/ })).toBeDisabled();
    await userEvent.click(choice);
    await userEvent.click(screen.getByRole("button", { name: /套用 \d+ 筆變更/ }));
    await waitFor(() => expect(body?.savePhoto).toBe(false));
  });

  it.each([false, true])("does not offer expired metadata as a saveable photo on refresh (hasPending=%s)", async (hasPendingPhoto) => {
    useRun({ ...pendingRun(), hasPendingPhoto, pendingPhotoExpiresAt: hasPendingPhoto ? new Date(Date.now() - 1).toISOString() : null });
    renderApp("/ai-import/run1");
    const choice = await screen.findByRole("checkbox", { name: /儲存菜單照片/ });
    expect(choice).not.toBeChecked();
    expect(choice).toBeDisabled();
    expect(screen.getByRole("button", { name: /套用 \d+ 筆變更/ })).toBeEnabled();
  });

  it("requires explicit opt-out for multiple targets, even with just one target accepted", async () => {
    const run = pendingRun();
    const t = run.proposal.targets[0];
    if (t.type !== "vendor") throw new Error("fixture");
    run.proposal.targets.push({ ...t, vendor: { kind: "existing", existingId: "v2" }, info: { ...t.info, action: "unchanged" }, menu: [] });
    useRun(run);
    let body: { savePhoto: boolean; accepted: unknown[] } | undefined;
    server.use(http.post("*/api/v1/ai/import-runs/run1/apply", async ({ request }) => {
      body = await request.json() as typeof body;
      return HttpResponse.json({ rosterIds: [], vendorIds: ["v1"] });
    }));
    renderApp("/ai-import/run1");
    const choice = await screen.findByRole("checkbox", { name: /儲存菜單照片/ });
    expect(choice).toBeChecked();
    expect(screen.getByText(/不會自動選擇第一家/)).toBeInTheDocument();
    expect(screen.getByRole("button", { name: /套用 \d+ 筆變更/ })).toBeDisabled();
    await userEvent.click(choice);
    await userEvent.click(screen.getByRole("button", { name: /套用 \d+ 筆變更/ }));
    await waitFor(() => expect(body?.savePhoto).toBe(false));
    expect(body?.accepted).toHaveLength(1);
  });

  it("does not apply photo-only when every data change is unchecked", async () => {
    useRun(pendingRun());
    renderApp("/ai-import/run1");
    await screen.findByRole("checkbox", { name: /儲存菜單照片/ });
    for (const label of ["套用店家資訊", "套用 滷肉飯", "套用 排骨飯"]) {
      await userEvent.click(screen.getByLabelText(label));
    }
    expect(screen.getByRole("button", { name: "套用 0 筆變更" })).toBeDisabled();
  });

  it("keeps edits after invalid_save_photo, refetches metadata, and never fakes success", async () => {
    const run = pendingRun();
    let calls = 0;
    let reads = 0;
    let body: { savePhoto: boolean } | undefined;
    server.use(
      http.get("*/api/v1/ai/import-runs/run1", () => {
        reads++;
        return HttpResponse.json(calls ? { ...run, hasPendingPhoto: false, pendingPhotoExpiresAt: null } : run);
      }),
      http.post("*/api/v1/ai/import-runs/run1/apply", async ({ request }) => {
        calls++;
        body = await request.json() as typeof body;
        return calls === 1 ? problem(422, "invalid_save_photo", { field: "savePhoto" }) : HttpResponse.json({ rosterIds: [], vendorIds: ["v1"] });
      }),
    );
    const { router } = renderApp("/ai-import/run1");
    const price = await screen.findByLabelText("排骨飯 價格");
    await userEvent.clear(price);
    await userEvent.type(price, "120");
    await userEvent.click(screen.getByRole("button", { name: /套用 \d+ 筆變更/ }));
    await screen.findByText(/沒有待存照片/);
    expect(reads).toBeGreaterThan(1);
    expect(router.state.location.pathname).toBe("/ai-import/run1");
    expect(screen.getByLabelText("排骨飯 價格")).toHaveValue("120");
    expect(screen.queryByRole("heading", { name: "已套用" })).not.toBeInTheDocument();
    await userEvent.click(screen.getByRole("checkbox", { name: /儲存菜單照片/ }));
    await userEvent.click(screen.getByRole("button", { name: /套用 \d+ 筆變更/ }));
    await waitFor(() => expect(router.state.location.pathname).toBe("/vendors/v1"));
    expect(body?.savePhoto).toBe(false);
  });

  it("reconciles a lost apply response through GET applied without reapplying", async () => {
    const run = pendingRun();
    let applied = false;
    const post = vi.fn(() => { applied = true; return HttpResponse.error(); });
    server.use(
      http.get("*/api/v1/ai/import-runs/run1", () => HttpResponse.json(applied ? { ...run, status: "applied", hasPendingPhoto: false, pendingPhotoExpiresAt: null } : run)),
      http.post("*/api/v1/ai/import-runs/run1/apply", post),
    );
    renderApp("/ai-import/run1");
    await userEvent.click(await screen.findByRole("button", { name: /套用 \d+ 筆變更/ }));
    expect(await screen.findByRole("heading", { name: "已套用" })).toBeInTheDocument();
    expect(post).toHaveBeenCalledOnce();
  });

  it("does not claim success if both apply and the result query fail", async () => {
    const run = pendingRun();
    let applied = false;
    server.use(
      http.get("*/api/v1/ai/import-runs/run1", () => applied ? HttpResponse.error() : HttpResponse.json(run)),
      http.post("*/api/v1/ai/import-runs/run1/apply", () => { applied = true; return HttpResponse.error(); }),
    );
    const { router } = renderApp("/ai-import/run1");
    const price = await screen.findByLabelText("排骨飯 價格");
    await userEvent.clear(price);
    await userEvent.type(price, "120");
    await userEvent.click(await screen.findByRole("button", { name: /套用 \d+ 筆變更/ }));
    expect(await screen.findByText(/目前無法確認匯入結果/)).toBeInTheDocument();
    expect(router.state.location.pathname).toBe("/ai-import/run1");
    expect(screen.queryByRole("heading", { name: "已套用" })).not.toBeInTheDocument();
    expect(screen.getByRole("button", { name: /套用 \d+ 筆變更/ })).toBeDisabled();
    expect(screen.getByLabelText("排骨飯 價格")).toHaveValue("120");
  });

  it("disables apply until the missing price is filled, and posts the expected body", async () => {
    useRun(VENDOR_RUN);
    let body: unknown;
    server.use(
      http.post("*/api/v1/ai/import-runs/run1/apply", async ({ request }) => {
        body = await request.json();
        return HttpResponse.json({ rosterIds: [], vendorIds: ["v1"] });
      }),
    );
    const { router } = renderApp("/ai-import/run1");
    const apply = await screen.findByRole("button", { name: /套用 \d+ 筆變更/ });
    expect(apply).toHaveTextContent("套用 4 筆變更");
    expect(apply).toBeDisabled();
    expect(screen.getByText(/價格被摺痕遮住/)).toBeInTheDocument();
    expect(screen.getByText(/請填寫價格/)).toBeInTheDocument();
    // unchanged row is folded away
    expect(screen.getByText("1 筆不變")).toBeInTheDocument();
    expect(screen.getByText(/\$100 →/)).toBeInTheDocument();

    await userEvent.type(screen.getByLabelText("雞腿飯 價格"), "85");
    expect(apply).toBeEnabled();

    await userEvent.click(screen.getByLabelText("套用 滷肉飯"));
    expect(apply).toHaveTextContent("套用 3 筆變更");

    await userEvent.click(apply);
    await waitFor(() => expect(router.state.location.pathname).toBe("/vendors/v1"));
    expect(body).toEqual({
      savePhoto: false,
      accepted: [
        {
          type: "vendor",
          vendor: { kind: "existing", existingId: "v1" },
          info: { phone: "02-1234", address: "台北市" },
          menu: [
            { name: "排骨飯", price: 105 },
            { name: "雞腿飯", price: 85 },
          ],
        },
      ],
    });
  });

  it("unchecking the row with the missing price unblocks apply", async () => {
    useRun(VENDOR_RUN);
    renderApp("/ai-import/run1");
    const apply = await screen.findByRole("button", { name: /套用/ });
    expect(apply).toBeDisabled();
    await userEvent.click(screen.getByLabelText("套用 雞腿飯"));
    expect(apply).toBeEnabled();
    expect(apply).toHaveTextContent("套用 3 筆變更");
  });

  it("highlights the row named by a 422 invalid_changes error", async () => {
    useRun(VENDOR_RUN);
    server.use(
      http.post("*/api/v1/ai/import-runs/run1/apply", () =>
        problem(422, "invalid_changes", {
          errors: [{ path: "accepted[0].menu[2].price", message: "價格需介於 1–100000" }],
        }),
      ),
    );
    renderApp("/ai-import/run1");
    await userEvent.type(await screen.findByLabelText("雞腿飯 價格"), "85");
    await userEvent.click(screen.getByRole("button", { name: /套用/ }));
    const msg = await screen.findByText(/價格需介於 1–100000/);
    const row = msg.closest("[data-testid=change-row]")!;
    expect(row).toHaveAttribute("data-state", "error");
    expect(within(row as HTMLElement).getByLabelText("雞腿飯 價格")).toBeInTheDocument();
    // other rows are untouched
    const rows = screen.getAllByTestId("change-row");
    expect(rows.filter((r) => r.getAttribute("data-state") === "error")).toHaveLength(1);
  });

  it("discards after inline confirmation", async () => {
    useRun(VENDOR_RUN);
    let discarded = false;
    server.use(
      http.post("*/api/v1/ai/import-runs/run1/discard", () => {
        discarded = true;
        return new HttpResponse(null, { status: 204 });
      }),
    );
    const { router } = renderApp("/ai-import/run1");
    await userEvent.click(await screen.findByRole("button", { name: "放棄" }));
    await userEvent.click(screen.getByRole("button", { name: "確定放棄" }));
    await waitFor(() => expect(router.state.location.pathname).toBe("/vendors/v1"));
    expect(discarded).toBe(true);
  });
});

describe("AiImportPage roster proposal", () => {
  it("lets the new roster name be edited and folds unchanged rows", async () => {
    useRun(ROSTER_RUN);
    let body: unknown;
    server.use(
      http.post("*/api/v1/ai/import-runs/run1/apply", async ({ request }) => {
        body = await request.json();
        return HttpResponse.json({ rosterIds: ["r9"], vendorIds: [] });
      }),
      http.get("*/api/v1/rosters/r9", () =>
        HttpResponse.json({ id: "r9", name: "新業務部", updatedAt: "2026-01-01T00:00:00Z", members: [] }),
      ),
    );
    const { router } = renderApp("/ai-import/run1");
    const name = await screen.findByDisplayValue("業務部");
    expect(screen.queryByRole("checkbox", { name: /儲存菜單照片/ })).not.toBeInTheDocument();
    expect(screen.getByText("1 筆不變")).toBeInTheDocument();
    expect(screen.getByText(/備註：會計 → 財務/)).toBeInTheDocument();
    await userEvent.clear(name);
    await userEvent.type(name, "新業務部");
    await userEvent.click(screen.getByRole("button", { name: "套用 2 筆變更" }));
    await waitFor(() => expect(router.state.location.pathname).toBe("/rosters/r9"));
    expect(body).toEqual({
      accepted: [
        {
          type: "roster",
          roster: { kind: "new", name: "新業務部" },
          members: [
            { name: "王小明", note: "業務" },
            { name: "李大華", note: "財務" },
          ],
        },
      ],
    });
  });

  it("blocks apply when the new roster name is emptied", async () => {
    useRun(ROSTER_RUN);
    renderApp("/ai-import/run1");
    await userEvent.clear(await screen.findByDisplayValue("業務部"));
    expect(screen.getByRole("button", { name: /套用/ })).toBeDisabled();
  });

  it("refreshes import access after apply is forbidden", async () => {
    let revoked = false;
    server.use(
      http.get("*/api/v1/ai/import-runs/run1", () => revoked ? problem(403, "forbidden") : HttpResponse.json({ ...ROSTER_RUN, sharingGroupId: "s1" })),
      http.post("*/api/v1/ai/import-runs/run1/apply", () => {
        revoked = true;
        return problem(403, "forbidden");
      }),
    );
    renderApp("/ai-import/run1");
    await userEvent.click(await screen.findByRole("button", { name: /套用 \d+ 筆變更/ }));
    expect(await screen.findByRole("alert")).toHaveTextContent("沒有權限");
    expect(screen.queryByRole("button", { name: /套用 \d+ 筆變更/ })).not.toBeInTheDocument();
  });
});

describe("AiImportPage other statuses", () => {
  it.each(["roster", "vendor"])("preserves scope in the completed new %s fallback after reload", async (mode) => {
    const proposal = mode === "roster" ? ROSTER_RUN.proposal : {
      warnings: [],
      targets: [{ type: "vendor", vendor: { kind: "new", name: "新店家" }, info: {}, menu: [] }],
    };
    useRun({ ...ROSTER_RUN, mode, proposal, status: "applied", sharingGroupId: "s1" });
    renderApp("/ai-import/run1");
    const label = mode === "roster" ? "前往名單" : "前往店家";
    const path = mode === "roster" ? "/rosters" : "/vendors";
    expect(await screen.findByRole("link", { name: label })).toHaveAttribute("href", `${path}?sharingGroupId=s1`);
  });

  it("shows applied with links", async () => {
    useRun({ ...VENDOR_RUN, status: "applied" });
    renderApp("/ai-import/run1");
    expect(await screen.findByRole("heading", { name: "已套用" })).toBeInTheDocument();
    expect(screen.getByRole("link", { name: "查看店家" })).toHaveAttribute("href", "/vendors/v1");
  });

  it("shows discarded", async () => {
    useRun({ ...VENDOR_RUN, status: "discarded" });
    renderApp("/ai-import/run1");
    expect(await screen.findByRole("heading", { name: "已放棄" })).toBeInTheDocument();
  });

  it("shows failed with a re-upload link back to the origin page", async () => {
    useRun({ ...VENDOR_RUN, status: "failed", error: "辨識逾時" });
    renderApp("/ai-import/run1");
    expect(await screen.findByText("辨識逾時")).toBeInTheDocument();
    expect(screen.getByRole("link", { name: "重新上傳" })).toHaveAttribute("href", "/vendors/v1");
  });
});
