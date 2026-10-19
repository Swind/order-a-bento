import { screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { http, HttpResponse } from "msw";
import { beforeEach, describe, expect, it } from "vitest";
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
