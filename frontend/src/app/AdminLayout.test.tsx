import { screen } from "@testing-library/react";
import { http, HttpResponse } from "msw";
import { describe, expect, it } from "vitest";
import { ME, problem, server } from "@/test/server";
import { renderApp } from "@/test/render";

describe("AdminLayout auth gating", () => {
  it("redirects to /login with returnTo on 401", async () => {
    server.use(
      http.get("*/api/v1/me", () => problem(401, "unauthorized")),
      http.get("*/api/v1/auth/config", () =>
        HttpResponse.json({
          googleEnabled: true,
          passwordLoginEnabled: false,
          devLoginEnabled: false,
        }),
      ),
    );
    const { router } = renderApp("/rosters?x=1");
    await screen.findByRole("button", { name: "使用 Google 登入" });
    expect(router.state.location.pathname).toBe("/login");
    expect(router.state.location.search).toBe("?returnTo=%2Frosters%3Fx%3D1");
  });

  it("renders the page and the user menu on 200", async () => {
    server.use(
      http.get("*/api/v1/me", () => HttpResponse.json(ME)),
      http.get("*/api/v1/rosters", () => HttpResponse.json([])),
    );
    const { router } = renderApp("/rosters");
    expect(await screen.findByRole("heading", { name: "名單" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "帳號選單" })).toBeInTheDocument();
    expect(router.state.location.pathname).toBe("/rosters");
  });
});
