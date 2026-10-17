import { screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { http, HttpResponse } from "msw";
import { beforeEach, describe, expect, it } from "vitest";
import { ME, problem, server } from "@/test/server";
import { renderApp } from "@/test/render";

const roster = (members: object[]) => ({
  id: "r1",
  name: "午餐團",
  updatedAt: "2026-01-01T00:00:00Z",
  members,
});
const m = (id: string, name: string, sortOrder: number, note: string | null = null) => ({
  id,
  name,
  note,
  sortOrder,
});

beforeEach(() => server.use(http.get("*/api/v1/me", () => HttpResponse.json(ME))));

describe("RosterDetailPage", () => {
  it("batch add posts the parsed payload and shows the report", async () => {
    let state = roster([m("a", "老王", 1)]);
    let posted: unknown;
    server.use(
      http.get("*/api/v1/rosters/r1", () => HttpResponse.json(state)),
      http.post("*/api/v1/rosters/r1/members/upsert", async ({ request }) => {
        posted = await request.json();
        state = roster([m("a", "老王", 1), m("b", "王小明", 2, "業務部"), m("c", "李大華", 3)]);
        return HttpResponse.json({
          roster: state,
          report: {
            created: 2,
            updated: 0,
            unchanged: 1,
            outcomes: [
              { input: "王小明", outcome: "created", memberId: "b" },
              { input: "李大華", outcome: "created", memberId: "c" },
              { input: "老王", outcome: "unchanged", memberId: "a" },
            ],
          },
        });
      }),
    );
    renderApp("/rosters/r1");
    const box = await screen.findByLabelText("批次新增成員");
    await userEvent.type(box, "王小明，業務部{Enter}{Enter}李大華{Enter}老王");
    await userEvent.click(screen.getByRole("button", { name: "新增 3 位" }));
    expect(await screen.findByRole("status")).toHaveTextContent(
      "新增 2 位、更新 0 位、未變更 1 位",
    );
    expect(posted).toEqual({
      members: [{ name: "王小明", note: "業務部" }, { name: "李大華" }, { name: "老王" }],
    });
    const row = (await screen.findByText("王小明")).closest("li")!;
    expect(within(row).getByText("業務部")).toBeInTheDocument();
    expect(row).toHaveAttribute("data-highlighted");
  });

  it("shows name_taken on the name field when editing a member", async () => {
    server.use(
      http.get("*/api/v1/rosters/r1", () => HttpResponse.json(roster([m("a", "老王", 1)]))),
      http.patch("*/api/v1/rosters/r1/members/a", () =>
        problem(409, "name_taken", { field: "name" }),
      ),
    );
    renderApp("/rosters/r1");
    await userEvent.click(await screen.findByRole("button", { name: "編輯 老王" }));
    const input = screen.getByLabelText("姓名");
    await userEvent.clear(input);
    await userEvent.type(input, "小李");
    await userEvent.click(screen.getByRole("button", { name: "儲存" }));
    expect(await screen.findByRole("alert")).toHaveTextContent("已有同名項目");
  });
});
