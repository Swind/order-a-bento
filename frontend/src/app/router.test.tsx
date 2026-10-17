import { render, screen, within } from "@testing-library/react";
import { RouterProvider } from "react-router";
import { http, HttpResponse } from "msw";
import { beforeEach, describe, expect, it } from "vitest";
import { ME, server } from "@/test/server";
import { Providers } from "./providers";
import { createTestRouter } from "./router";

function renderAt(path: string) {
  return render(
    <Providers>
      <RouterProvider router={createTestRouter([path])} />
    </Providers>,
  );
}

describe("router", () => {
  beforeEach(() => server.use(http.get("*/api/v1/me", () => HttpResponse.json(ME))));
  it("renders admin layout with nav at /", async () => {
    server.use(http.get("*/api/v1/group-orders", () => HttpResponse.json([])));
    renderAt("/");
    const nav = await screen.findByRole("navigation", { name: "主選單" });
    for (const label of ["首頁", "名單", "店家", "開團紀錄"]) {
      expect(within(nav).getByRole("link", { name: label })).toBeInTheDocument();
    }
  });

  it("renders public page without admin nav at /g/abc", async () => {
    server.use(
      http.get("*/api/v1/public/g/abc", () =>
        HttpResponse.json({
          title: "週五午餐",
          status: "open",
          acceptingOrders: true,
          deadlineAt: null,
          note: null,
          vendor: { name: "好吃便當", phone: null, address: null },
          participants: [],
          menu: [],
        }),
      ),
    );
    renderAt("/g/abc");
    expect(await screen.findByRole("heading", { name: "週五午餐" })).toBeInTheDocument();
    expect(screen.queryByRole("navigation")).not.toBeInTheDocument();
  });

  it("lazy admin routes resolve through the route fallback", async () => {
    server.use(http.get("*/api/v1/rosters", () => HttpResponse.json([])));
    renderAt("/rosters");
    expect(await screen.findByRole("heading", { name: "名單" })).toBeInTheDocument();
    expect(await screen.findByText(/還沒有名單/)).toBeInTheDocument();
  });

  it("renders 404 for unknown paths", async () => {
    renderAt("/nope");
    expect(await screen.findByRole("heading", { name: "找不到頁面" })).toBeInTheDocument();
  });
});
