import { screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { http, HttpResponse } from "msw";
import { beforeEach, describe, expect, it } from "vitest";
import { renderApp } from "@/test/render";
import { ME, server } from "@/test/server";

const item = (id: string, title: string, status: string) => ({
  id,
  title,
  status,
  vendorName: "好吃便當",
  deadlineAt: null,
  createdAt: "2026-01-01T00:00:00Z",
  closedAt: null,
  participantCount: 5,
  orderedCount: 3,
  totalAmount: 330,
});

beforeEach(() =>
  server.use(
    http.get("*/api/v1/me", () => HttpResponse.json(ME)),
    http.get("*/api/v1/group-orders", () =>
      HttpResponse.json([
        item("a", "進行團", "open"),
        item("b", "關閉團", "closed"),
        item("c", "封存團", "archived"),
      ]),
    ),
  ),
);

describe("GroupOrdersPage", () => {
  it("shows only active groups on the dashboard with an opening shortcut", async () => {
    renderApp("/");
    expect(await screen.findByRole("heading", { name: "今天訂什麼？" })).toBeInTheDocument();
    expect(await screen.findByRole("link", { name: /進行團/ })).toHaveAttribute("href", "/group-orders/a");
    expect(screen.queryByText("關閉團")).not.toBeInTheDocument();
    expect(screen.queryByText("封存團")).not.toBeInTheDocument();
    expect(screen.getByRole("link", { name: "開團" })).toHaveAttribute("href", "/group-orders/new");
    expect(screen.getByRole("heading", { name: /進行中的團\s*1/ })).toBeInTheDocument();
  });

  it("hides archived by default and filters with chips", async () => {
    renderApp("/group-orders");
    expect(await screen.findByText("進行團")).toBeInTheDocument();
    expect(screen.getByText("關閉團")).toBeInTheDocument();
    expect(screen.queryByText("封存團")).not.toBeInTheDocument();
    expect(screen.getAllByText("已訂 3/5 人")).toHaveLength(2);

    await userEvent.click(screen.getByRole("button", { name: "全部" }));
    expect(screen.getByText("封存團")).toBeInTheDocument();

    await userEvent.click(screen.getByRole("button", { name: "進行中" }));
    await userEvent.click(screen.getByRole("button", { name: "已關閉" }));
    expect(screen.queryByText("進行團")).not.toBeInTheDocument();
    expect(screen.queryByText("關閉團")).not.toBeInTheDocument();
    expect(screen.getByText("封存團")).toBeInTheDocument();
  });

  it("links a card to its detail page", async () => {
    renderApp("/group-orders");
    const link = await screen.findByRole("link", { name: /進行團/ });
    expect(link).toHaveAttribute("href", "/group-orders/a");
  });

  it("shows an empty state linking to 開團", async () => {
    server.use(http.get("*/api/v1/group-orders", () => HttpResponse.json([])));
    renderApp("/group-orders");
    expect(await screen.findByText("沒有符合的開團紀錄")).toBeInTheDocument();
    expect(screen.getAllByRole("link", { name: "開團" }).length).toBeGreaterThan(0);
  });
});
