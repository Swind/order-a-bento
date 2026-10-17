import { screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { http, HttpResponse } from "msw";
import { beforeEach, describe, expect, it } from "vitest";
import { renderApp } from "@/test/render";
import { ME, problem, server } from "@/test/server";

const detail = {
  id: "g1",
  title: "T",
  status: "open",
  deadlineAt: null,
  note: null,
  shareUrl: "https://x.test/g/tok",
  vendor: { name: "好吃便當", phone: null, address: null },
  participants: [],
  offeredItems: [],
  createdAt: "2026-01-01T00:00:00Z",
  closedAt: null,
};

beforeEach(() =>
  server.use(
    http.get("*/api/v1/me", () => HttpResponse.json(ME)),
    http.get("*/api/v1/rosters", () =>
      HttpResponse.json([
        { id: "r0", name: "空名單", memberCount: 0, updatedAt: "2026-01-01T00:00:00Z" },
        { id: "r1", name: "業務部", memberCount: 5, updatedAt: "2026-01-01T00:00:00Z" },
      ]),
    ),
    http.get("*/api/v1/vendors", () =>
      HttpResponse.json([
        { id: "v0", name: "空店", itemCount: 0, updatedAt: "2026-01-01T00:00:00Z" },
        { id: "v1", name: "好吃便當", itemCount: 3, updatedAt: "2026-01-01T00:00:00Z" },
      ]),
    ),
  ),
);

describe("NewGroupOrderPage", () => {
  it("disables empty rosters and vendors", async () => {
    renderApp("/group-orders/new");
    expect(await screen.findByRole("button", { name: /空名單/ })).toBeDisabled();
    await userEvent.click(screen.getByRole("button", { name: /業務部/ }));
    await userEvent.click(screen.getByRole("button", { name: "下一步" }));
    expect(await screen.findByRole("button", { name: /空店/ })).toBeDisabled();
    expect(screen.getByRole("button", { name: "下一步" })).toBeDisabled();
  });

  it("walks the 3 steps, posts the body and navigates to the group", async () => {
    let posted: Record<string, unknown> | undefined;
    server.use(
      http.post("*/api/v1/group-orders", async ({ request }) => {
        posted = (await request.json()) as Record<string, unknown>;
        return HttpResponse.json(detail, { status: 201 });
      }),
      http.get("*/api/v1/group-orders/g1", () => HttpResponse.json(detail)),
    );
    const { router } = renderApp("/group-orders/new");
    await userEvent.click(await screen.findByRole("button", { name: /業務部/ }));
    await userEvent.click(screen.getByRole("button", { name: "下一步" }));
    await userEvent.click(await screen.findByRole("button", { name: /好吃便當/ }));
    await userEvent.click(screen.getByRole("button", { name: "下一步" }));
    const title = await screen.findByLabelText("標題");
    expect((title as HTMLInputElement).value).toMatch(/^\d+\/\d+ 午餐 · 好吃便當$/);
    await userEvent.clear(title);
    await userEvent.type(title, "週五午餐");
    await userEvent.click(screen.getByRole("button", { name: "1 小時後" }));
    await userEvent.click(screen.getByRole("button", { name: "開團" }));
    expect(await screen.findByText("分享連結")).toBeInTheDocument();
    expect(router.state.location.pathname).toBe("/group-orders/g1");
    expect(posted).toMatchObject({ title: "週五午餐", rosterId: "r1", vendorId: "v1" });
    const delta = new Date(posted!.deadlineAt as string).getTime() - Date.now();
    expect(delta).toBeGreaterThan(55 * 60_000);
    expect(delta).toBeLessThan(65 * 60_000);
  });

  it("shows server field errors for the title", async () => {
    server.use(
      http.post("*/api/v1/group-orders", () => problem(422, "invalid_title", { field: "title" })),
    );
    renderApp("/group-orders/new");
    await userEvent.click(await screen.findByRole("button", { name: /業務部/ }));
    await userEvent.click(screen.getByRole("button", { name: "下一步" }));
    await userEvent.click(await screen.findByRole("button", { name: /好吃便當/ }));
    await userEvent.click(screen.getByRole("button", { name: "下一步" }));
    await userEvent.click(await screen.findByRole("button", { name: "開團" }));
    expect(await screen.findByText("標題格式不正確")).toBeInTheDocument();
  });

  it("dashboard links to the wizard", async () => {
    renderApp("/");
    expect(await screen.findByRole("link", { name: /開團$/ })).toHaveAttribute("href", "/group-orders/new");
  });
});
