import { screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { http, HttpResponse } from "msw";
import { afterEach, describe, expect, it, vi } from "vitest";
import { ME, problem, server } from "@/test/server";
import { renderApp } from "@/test/render";

describe("AdminLayout auth gating", () => {
  afterEach(() => vi.unstubAllGlobals());

  it("starts the default server login with returnTo on 401, keeping a backup link", async () => {
    const replace = vi.fn();
    vi.stubGlobal("location", { ...window.location, replace });
    server.use(
      http.get("*/api/v1/me", () => problem(401, "unauthorized")),
      http.get("*/api/v1/auth/config", () =>
        HttpResponse.json({
          oidcEnabled: true,
          passwordLoginEnabled: false,
          devLoginEnabled: false,
        }),
      ),
    );
    renderApp("/rosters?x=1");
    await screen.findByRole("status");
    expect(replace).toHaveBeenCalledWith("/auth/login?returnTo=%2Frosters%3Fx%3D1");
    expect(screen.getByRole("link", { name: "管理員備援登入" })).toHaveAttribute("href", "/login?returnTo=%2Frosters%3Fx%3D1");
  });

  it("opens the account menu and logs out to the backup login page", async () => {
    server.use(
      http.get("*/api/v1/me", () => HttpResponse.json(ME)),
      http.get("*/api/v1/rosters", () => HttpResponse.json([])),
      http.get("*/api/v1/auth/config", () => HttpResponse.json({
        oidcEnabled: true, passwordLoginEnabled: true, devLoginEnabled: false,
      })),
      http.post("*/auth/logout", () => {
        server.use(http.get("*/api/v1/me", () => problem(401, "unauthorized")));
        return new HttpResponse(null, { status: 204 });
      }),
    );
    const { router } = renderApp("/rosters");
    expect(await screen.findByRole("heading", { name: "名單" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "帳號選單" })).toBeInTheDocument();
    expect(router.state.location.pathname).toBe("/rosters");
    await userEvent.click(screen.getByRole("button", { name: "帳號選單" }));
    expect(await screen.findByRole("menuitem", { name: "登出" })).toBeInTheDocument();
    expect(screen.getByRole("group", { name: ME.email })).toBeInTheDocument();
    await userEvent.click(screen.getByRole("menuitem", { name: "登出" }));
    expect(await screen.findByRole("heading", { name: "管理員備援登入" })).toBeInTheDocument();
    expect(router.state.location.pathname).toBe("/login");
  });
});
