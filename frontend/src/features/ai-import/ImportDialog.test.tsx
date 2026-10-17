import { screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { http, HttpResponse } from "msw";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { renderApp } from "@/test/render";
import { ME, problem, server } from "@/test/server";
import { AUTH_CONFIG, ROSTER_RUN } from "./fixtures";

vi.mock("@/shared/lib/compressImage", () => ({
  compressImage: async () => new Blob(["jpegdata"], { type: "image/jpeg" }),
}));

const roster = { id: "r1", name: "午餐團", updatedAt: "2026-01-01T00:00:00Z", members: [] };

function setup(config: Partial<typeof AUTH_CONFIG> = {}) {
  server.use(
    http.get("*/api/v1/me", () => HttpResponse.json(ME)),
    http.get("*/api/v1/auth/config", () => HttpResponse.json({ ...AUTH_CONFIG, ...config })),
    http.get("*/api/v1/rosters/r1", () => HttpResponse.json(roster)),
    http.get("*/api/v1/rosters", () => HttpResponse.json([])),
    http.get("*/api/v1/ai/import-runs/run1", () => HttpResponse.json(ROSTER_RUN)),
    http.get("*/api/v1/rosters/r1", () => HttpResponse.json(roster)),
  );
}

const photo = () => new File(["x"], "menu.png", { type: "image/png" });

beforeEach(() => vi.clearAllMocks());

describe("ImportDialog", () => {
  it("shows a disabled 尚未設定 AI button when AI import is off", async () => {
    setup({ aiImportEnabled: false });
    renderApp("/rosters/r1");
    expect(await screen.findByText("尚未設定 AI")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "📷 從照片匯入" })).toBeDisabled();
  });

  it("posts FormData with mode and target id, then opens the confirm screen", async () => {
    setup();
    let form: FormData | undefined;
    server.use(
      http.post("*/api/v1/ai/import-runs", async ({ request }) => {
        form = await request.formData();
        return HttpResponse.json(ROSTER_RUN, { status: 201 });
      }),
    );
    const { router } = renderApp("/rosters/r1");
    await userEvent.click(await screen.findByRole("button", { name: "📷 從照片匯入" }));
    expect(screen.getByRole("button", { name: "開始辨識" })).toBeDisabled();
    await userEvent.upload(screen.getByLabelText("選擇照片檔案"), photo());
    await userEvent.type(screen.getByLabelText("指示（選填）"), "加到業務部名單");
    await userEvent.click(screen.getByRole("button", { name: "開始辨識" }));
    await waitFor(() => expect(router.state.location.pathname).toBe("/ai-import/run1"));
    expect(form!.get("mode")).toBe("roster");
    expect(form!.get("targetRosterId")).toBe("r1");
    expect(form!.get("instruction")).toBe("加到業務部名單");
    expect(form!.get("targetVendorId")).toBeNull();
    expect((form!.get("image") as File).type).toBe("image/jpeg");
  });

  it("shows the refusal message", async () => {
    setup();
    server.use(
      http.post("*/api/v1/ai/import-runs", () => problem(422, "ai_refused", { runId: "run9" })),
    );
    renderApp("/rosters/r1");
    await userEvent.click(await screen.findByRole("button", { name: "📷 從照片匯入" }));
    await userEvent.upload(screen.getByLabelText("選擇照片檔案"), photo());
    await userEvent.click(screen.getByRole("button", { name: "開始辨識" }));
    expect(await screen.findByRole("alert")).toHaveTextContent("這張照片無法辨識，請改用手動輸入");
    expect(screen.queryByRole("button", { name: "重試" })).not.toBeInTheDocument();
  });

  it("offers retry when the AI is unavailable and shows the demo badge", async () => {
    setup({ aiDemo: true });
    server.use(http.post("*/api/v1/ai/import-runs", () => problem(502, "ai_unavailable")));
    renderApp("/rosters/r1");
    await userEvent.click(await screen.findByRole("button", { name: "📷 從照片匯入" }));
    expect(screen.getByText("示範模式")).toBeInTheDocument();
    await userEvent.upload(screen.getByLabelText("選擇照片檔案"), photo());
    await userEvent.click(screen.getByRole("button", { name: "開始辨識" }));
    expect(await screen.findByRole("alert")).toHaveTextContent("AI 服務暫時無法使用，請稍後再試");
    expect(screen.getByRole("button", { name: "重試" })).toBeInTheDocument();
  });

  it("is offered on the rosters list without a target", async () => {
    setup();
    let form: FormData | undefined;
    server.use(
      http.post("*/api/v1/ai/import-runs", async ({ request }) => {
        form = await request.formData();
        return problem(413, "image_too_large");
      }),
    );
    renderApp("/rosters");
    await userEvent.click(await screen.findByRole("button", { name: "📷 從照片建立名單" }));
    await userEvent.upload(screen.getByLabelText("選擇照片檔案"), photo());
    await userEvent.click(screen.getByRole("button", { name: "開始辨識" }));
    expect(await screen.findByRole("alert")).toHaveTextContent("圖片太大");
    expect(form!.get("mode")).toBe("roster");
    expect(form!.get("targetRosterId")).toBeNull();
  });
});
