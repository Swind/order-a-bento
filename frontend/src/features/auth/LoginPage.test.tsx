import { screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { http, HttpResponse } from "msw";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { ME, problem, server } from "@/test/server";
import { renderApp } from "@/test/render";

function mockConfig(devLoginEnabled: boolean, oidcEnabled = true, passwordLoginEnabled = false) {
  server.use(
    http.get("*/api/v1/auth/config", () =>
      HttpResponse.json({ oidcEnabled, devLoginEnabled, passwordLoginEnabled }),
    ),
  );
}

beforeEach(() => server.use(http.get("*/api/v1/me", () => problem(401, "unauthorized"))));
afterEach(() => vi.unstubAllGlobals());

describe("LoginPage", () => {
  it.each(["/login", "/login?error=provider_error"])("keeps %s free of administrator controls without redirect loops", async (path) => {
    const replace = vi.fn();
    vi.stubGlobal("location", { ...window.location, replace });
    mockConfig(false, true, true);
    renderApp(path);
    expect(await screen.findByRole("button", { name: "使用單一登入（OIDC）" })).toBeEnabled();
    expect(screen.queryByLabelText("密碼")).not.toBeInTheDocument();
    expect(screen.queryByText(/管理員/)).not.toBeInTheDocument();
    expect(screen.queryByRole("link")).not.toBeInTheDocument();
    expect(replace).not.toHaveBeenCalled();
  });
  it("shows the dev form only when devLoginEnabled", async () => {
    mockConfig(false);
    const { unmount } = renderApp("/login");
    await screen.findByRole("button", { name: "使用單一登入（OIDC）" });
    expect(screen.queryByLabelText("Email")).not.toBeInTheDocument();
    unmount();

    mockConfig(true);
    renderApp("/login");
    expect(await screen.findByLabelText("Email")).toBeInTheDocument();
  });

  it("hides the OIDC button when OIDC login is disabled", async () => {
    mockConfig(true, false);
    renderApp("/login");
    await screen.findByRole("button", { name: "開發用登入" });
    expect(screen.queryByRole("button", { name: "使用單一登入（OIDC）" })).not.toBeInTheDocument();
  });

  it("shows an explanation when no login method is enabled", async () => {
    mockConfig(false, false, false);
    renderApp("/login");
    expect(await screen.findByText(/目前沒有可用的登入方式/)).toBeInTheDocument();
    expect(screen.queryByRole("button")).not.toBeInTheDocument();
  });

  it("shows only the password form at the independent administrator URL", async () => {
    mockConfig(false, false, true);
    renderApp("/ops/bento-access");
    const email = await screen.findByLabelText("Email");
    expect(email).toHaveAttribute("autocomplete", "username");
    expect(screen.getByLabelText("密碼")).toHaveAttribute("autocomplete", "current-password");
    expect(screen.queryByRole("button", { name: "使用單一登入（OIDC）" })).not.toBeInTheDocument();
    expect(screen.queryByRole("form", { name: "開發用登入" })).not.toBeInTheDocument();
  });

  it("does not expose backup login on the ordinary page even when it is the only enabled method", async () => {
    mockConfig(false, false, true);
    renderApp("/login");
    expect(await screen.findByText(/目前沒有可用的登入方式/)).toBeInTheDocument();
    expect(screen.queryByLabelText("密碼")).not.toBeInTheDocument();
    expect(screen.queryByRole("link")).not.toBeInTheDocument();
    expect(screen.queryByText("管理員備援登入")).not.toBeInTheDocument();
  });

  it("does not show a password form when backup login is disabled", async () => {
    mockConfig(true, true, false);
    renderApp("/ops/bento-access");
    expect(await screen.findByText(/目前沒有可用的登入方式/)).toBeInTheDocument();
    expect(screen.queryByLabelText("密碼")).not.toBeInTheDocument();
    expect(screen.queryByRole("button")).not.toBeInTheDocument();
  });

  it("keeps the administrator page independent even when OIDC and dev login are enabled", async () => {
    mockConfig(true, true, true);
    renderApp("/ops/bento-access");
    await screen.findByLabelText("密碼");
    expect(screen.queryByRole("button", { name: "使用單一登入（OIDC）" })).not.toBeInTheDocument();
    expect(screen.queryByRole("form", { name: "開發用登入" })).not.toBeInTheDocument();
    expect(screen.getByRole("heading", { name: "管理員備援登入" })).toBeInTheDocument();
  });

  it("password login posts credentials and navigates to returnTo", async () => {
    mockConfig(false, false, true);
    let body: unknown;
    server.use(
      http.post("*/auth/password-login", async ({ request }) => {
        body = await request.json();
        server.use(http.get("*/api/v1/me", () => HttpResponse.json(ME)));
        return new HttpResponse(null, { status: 204 });
      }),
      http.get("*/api/v1/vendors", () => HttpResponse.json([])),
    );
    const { router } = renderApp("/ops/bento-access?returnTo=%2Fvendors");
    await userEvent.type(await screen.findByLabelText("Email"), " admin@example.com ");
    await userEvent.type(screen.getByLabelText("密碼"), "correct horse");
    await userEvent.click(screen.getByRole("button", { name: "登入" }));
    await waitFor(() => expect(router.state.location.pathname).toBe("/vendors"));
    expect(body).toEqual({ email: "admin@example.com", password: "correct horse" });
  });

  it("shows a friendly error for invalid credentials and for rate limiting", async () => {
    mockConfig(false, false, true);
    let status = 401;
    server.use(
      http.post("*/auth/password-login", () =>
        status === 401 ? problem(401, "invalid_credentials") : problem(429, "rate_limited"),
      ),
    );
    renderApp("/ops/bento-access");
    await userEvent.type(await screen.findByLabelText("Email"), "a@b.co");
    await userEvent.type(screen.getByLabelText("密碼"), "wrong password");
    await userEvent.click(screen.getByRole("button", { name: "登入" }));
    expect(await screen.findByRole("alert")).toHaveTextContent("Email 或密碼不正確");

    status = 429;
    await userEvent.click(screen.getByRole("button", { name: "登入" }));
    await waitFor(() => expect(screen.getByRole("alert")).toHaveTextContent("操作太頻繁"));
  });

  it("shows a friendly alert for ?error=access_denied", async () => {
    mockConfig(false);
    renderApp("/login?error=access_denied");
    expect(await screen.findByRole("alert")).toHaveTextContent("取消");
  });

  it("dev login navigates to returnTo, falling back to / for //evil.com", async () => {
    mockConfig(true);
    let body: unknown;
    server.use(
      http.post("*/auth/dev-login", async ({ request }) => {
        body = await request.json();
        // After login /me succeeds.
        server.use(http.get("*/api/v1/me", () => HttpResponse.json(ME)));
        return new HttpResponse(null, { status: 204 });
      }),
      http.get("*/api/v1/rosters", () => HttpResponse.json([])),
      http.get("*/api/v1/vendors", () => HttpResponse.json([])),
    );
    const { router } = renderApp("/login?returnTo=//evil.com");
    await userEvent.type(await screen.findByLabelText("Email"), "a@b.co");
    await userEvent.click(screen.getByRole("button", { name: "開發用登入" }));
    await waitFor(() => expect(router.state.location.pathname).toBe("/"));
    expect(body).toEqual({ email: "a@b.co" });
  });

  it.each(["/login", "/ops/bento-access"])("honours a safe returnTo at %s when already logged in", async (path) => {
    mockConfig(false);
    server.use(
      http.get("*/api/v1/me", () => HttpResponse.json(ME)),
      http.get("*/api/v1/vendors", () => HttpResponse.json([])),
    );
    const { router } = renderApp(`${path}?returnTo=%2Fvendors`);
    await screen.findByRole("heading", { name: "店家" });
    expect(router.state.location.pathname).toBe("/vendors");
  });
});
