import { useState } from "react";
import { act, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { http, HttpResponse } from "msw";
import { describe, expect, it, vi } from "vitest";
import { server, problem } from "@/test/server";
import { VENDOR_RUN } from "./fixtures";
import { initState, type TargetState } from "./lib";
import { draftToState, stateToDraft, useDraftConversation, type DraftState, type DraftVendor } from "./draft";

const base = (): DraftState => ({ revision: 0, vendors: stateToDraft(initState(VENDOR_RUN.proposal)), turns: [], undo: null, pending: null, pendingUntil: null });
const undoable = (): DraftState => {
  const s = base();
  s.revision = 2;
  s.undo = structuredClone(s.vendors);
  s.vendors[0].menu[0].price = 7.5;
  s.turns = [{ id: "edit", instruction: "改價", reply: "完成", status: "completed", changes: ["價格 60 → 7.5"], undone: false }];
  return s;
};
function Harness({ initial, unlocked = false }: { initial?: DraftState; unlocked?: boolean }) {
  const [state, setState] = useState<TargetState[]>(() => initial ? draftToState(initial) : initState(VENDOR_RUN.proposal));
  const chat = useDraftConversation({ ...VENDOR_RUN, draft: initial }, state, setState);
  const first = state[0];
  if (first.type !== "vendor") return null;
  return <>
    <input aria-label="手動品名" disabled={!unlocked && chat.busy} value={first.menu[0].name} onChange={(e) => setState((s) => s.map((t) => t.type === "vendor" ? { ...t, menu: t.menu.map((m, i) => i === 0 ? { ...m, name: e.target.value } : m) } : t))} />
    <output aria-label="品項價格">{first.menu[0].price}</output>
    {chat.panel}
  </>;
}
async function send(text = "第一項改 7.5 元") {
  await userEvent.type(screen.getByLabelText(/後續指示/), text);
  await userEvent.click(screen.getByRole("button", { name: "送出修改指示" }));
}

describe("vendor draft conversation", () => {
  it("keeps undo after an identical save and a no-change question", async () => {
    let saved = undoable();
    server.use(
      http.put("*/api/v1/ai/import-runs/run1/draft", () => HttpResponse.json(saved = { ...saved, revision: saved.revision + 1 })),
      http.post("*/api/v1/ai/import-runs/run1/turns", async ({ request }) => {
        const body = await request.json() as { turnId: string; instruction: string };
        saved = { ...saved, revision: saved.revision + 2, turns: [...saved.turns, { id: body.turnId, instruction: body.instruction, reply: "請問哪一項？", status: "completed", changes: [], undone: false }] };
        return HttpResponse.json(saved);
      }),
    );
    render(<Harness initial={saved} />);
    await userEvent.click(screen.getByRole("button", { name: "儲存草稿" }));
    await waitFor(() => expect(screen.getByRole("button", { name: "復原上一輪" })).toBeEnabled());
    await send("請問價格？");
    await screen.findByText("AI：請問哪一項？");
    expect(screen.getByRole("button", { name: "復原上一輪" })).toBeEnabled();
    expect(screen.getByText(/僅提問不新增復原版本/)).toBeVisible();
  });

  it("disables old undo immediately on manual editing and keeps it disabled after save", async () => {
    let saved = undoable();
    const undo = vi.fn();
    server.use(
      http.put("*/api/v1/ai/import-runs/run1/draft", async ({ request }) => {
        const body = await request.json() as { vendors: DraftVendor[] };
        saved = { ...saved, vendors: body.vendors, undo: null, revision: saved.revision + 1 };
        return HttpResponse.json(saved);
      }),
      http.post("*/api/v1/ai/import-runs/run1/undo", undo),
    );
    render(<Harness initial={saved} />);
    expect(screen.getByRole("button", { name: "復原上一輪" })).toBeEnabled();
    await userEvent.clear(screen.getByLabelText("手動品名"));
    await userEvent.type(screen.getByLabelText("手動品名"), "最新手改不可丟");
    expect(screen.getByRole("button", { name: "復原上一輪" })).toBeDisabled();
    await userEvent.click(screen.getByRole("button", { name: "儲存草稿" }));
    await waitFor(() => expect(screen.getByRole("button", { name: "儲存草稿" })).toBeEnabled());
    expect(saved.vendors[0].menu[0].name).toBe("最新手改不可丟");
    expect(screen.getByLabelText("手動品名")).toHaveValue("最新手改不可丟");
    expect(screen.getByRole("button", { name: "復原上一輪" })).toBeDisabled();
    expect(undo).not.toHaveBeenCalled();
  });

  it("keeps old undo disabled when a new no-change turn synchronizes manual edits", async () => {
    const saved = undoable();
    server.use(http.post("*/api/v1/ai/import-runs/run1/turns", async ({ request }) => {
      const body = await request.json() as { vendors: DraftVendor[]; turnId: string; instruction: string };
      return HttpResponse.json({ ...saved, vendors: body.vendors, undo: null, revision: 4, turns: [...saved.turns, { id: body.turnId, instruction: body.instruction, reply: "請問哪一項？", status: "completed", changes: [], undone: false }] });
    }));
    render(<Harness initial={saved} />);
    await userEvent.clear(screen.getByLabelText("手動品名"));
    await userEvent.type(screen.getByLabelText("手動品名"), "新回合的手改");
    await send("請再確認");
    await screen.findByText("AI：請問哪一項？");
    expect(screen.getByLabelText("手動品名")).toHaveValue("新回合的手改");
    expect(screen.getByRole("button", { name: "復原上一輪" })).toBeDisabled();
  });

  it("sends latest manual values, preserves IDs and edits after undo using the restored state", async () => {
    let saved = base();
    const received: DraftVendor[][] = [];
    server.use(
      http.post("*/api/v1/ai/import-runs/run1/turns", async ({ request }) => {
        const body = await request.json() as { revision: number; vendors: DraftVendor[]; turnId: string; instruction: string };
        received.push(structuredClone(body.vendors));
        saved = { ...saved, revision: body.revision + 2, vendors: structuredClone(body.vendors), undo: structuredClone(body.vendors), turns: [...saved.turns, { id: body.turnId, instruction: body.instruction, reply: "已改價", status: "completed", changes: ["價格 60 → 110"], undone: false }] };
        saved.vendors[0].menu[0].price = 7.5;
        return HttpResponse.json(saved);
      }),
      http.post("*/api/v1/ai/import-runs/run1/undo", () => {
        saved = { ...saved, revision: saved.revision + 1, vendors: saved.undo!, undo: null, turns: saved.turns.map((t) => ({ ...t, undone: true })) };
        return HttpResponse.json(saved);
      }),
    );
    vi.spyOn(window, "confirm").mockReturnValue(true);
    render(<Harness />);
    await userEvent.clear(screen.getByLabelText("手動品名"));
    await userEvent.type(screen.getByLabelText("手動品名"), "手改保留");
    await send();
    await screen.findByText("AI：已改價");
    expect(received[0][0].menu[0].name).toBe("手改保留");
    expect(screen.getByLabelText("品項價格")).toHaveTextContent("7.5");
    await userEvent.click(screen.getByRole("button", { name: "復原上一輪" }));
    await screen.findByText("已復原");
    expect(screen.getByLabelText("手動品名")).toHaveValue("手改保留");
    expect(screen.getByLabelText("品項價格")).toHaveTextContent("60");
    await send("再改價格");
    await waitFor(() => expect(received).toHaveLength(2));
    expect(received[1][0].menu[0]).toMatchObject({ id: "v0-m0", name: "手改保留", price: 60 });
  });

  it("keeps manual edits on HTTP failure and retries the same turn ID without duplicate work", async () => {
    const bodies: { turnId: string; vendors: DraftVendor[] }[] = [];
    server.use(http.post("*/api/v1/ai/import-runs/run1/turns", async ({ request }) => {
      const body = await request.json() as typeof bodies[number]; bodies.push(body);
      if (bodies.length === 1) return problem(503, "ai_unavailable");
      return HttpResponse.json({ ...base(), revision: 2, vendors: body.vendors, turns: [{ id: body.turnId, instruction: "第一項改 110 元", reply: "請問是哪一類？", status: "completed", changes: [], undone: false }] });
    }));
    render(<Harness />);
    await userEvent.clear(screen.getByLabelText("手動品名"));
    await userEvent.type(screen.getByLabelText("手動品名"), "錯誤不丟");
    await send();
    await screen.findByRole("alert");
    expect(screen.getByLabelText("手動品名")).toHaveValue("錯誤不丟");
    await userEvent.click(screen.getByRole("button", { name: /重試本輪/ }));
    await screen.findByText("AI：請問是哪一類？");
    expect(bodies[1]).toEqual(bodies[0]);
    expect(screen.getByText("本輪未修改草稿")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "復原上一輪" })).toBeDisabled();
  });

  it("does not replace newer manual input with a late response", async () => {
    let release!: () => void;
    const gate = new Promise<void>((resolve) => { release = resolve; });
    server.use(http.post("*/api/v1/ai/import-runs/run1/turns", async () => { await gate; return HttpResponse.json({ ...base(), revision: 2 }); }));
    render(<Harness unlocked />);
    await send();
    await userEvent.clear(screen.getByLabelText("手動品名"));
    await userEvent.type(screen.getByLabelText("手動品名"), "較新手改");
    await act(async () => { release(); });
    await screen.findByText(/未覆蓋手動內容/);
    expect(screen.getByLabelText("手動品名")).toHaveValue("較新手改");
    expect(screen.getByRole("button", { name: "送出修改指示" })).toBeDisabled();
  });

  it("restores saved drafts and keeps values when undo conflicts", async () => {
    const s = base(); s.revision = 2; s.undo = structuredClone(s.vendors);
    s.vendors[0].menu[0].name = "已儲存手改";
    s.turns = [{ id: "one", instruction: "修改", reply: "完成", status: "completed", changes: ["改名"], undone: false }];
    server.use(http.post("*/api/v1/ai/import-runs/run1/undo", () => problem(409, "conflict")));
    vi.spyOn(window, "confirm").mockReturnValue(true);
    render(<Harness initial={s} />);
    expect(screen.getByLabelText("手動品名")).toHaveValue("已儲存手改");
    await userEvent.click(screen.getByRole("button", { name: "復原上一輪" }));
    await screen.findByRole("alert");
    expect(screen.getByLabelText("手動品名")).toHaveValue("已儲存手改");
  });
});
