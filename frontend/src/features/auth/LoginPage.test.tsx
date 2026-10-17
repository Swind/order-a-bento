import { screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { http, HttpResponse } from "msw";
import { beforeEach, describe, expect, it } from "vitest";
import { ME, problem, server } from "@/test/server";
import { renderApp } from "@/test/render";

function mockConfig(devLoginEnabled: boolean, googleEnabled = true, passwordLoginEnabled = false) {
  server.use(
    http.get("*/api/v1/auth/config", () =>
      HttpResponse.json({ googleEnabled, devLoginEnabled, passwordLoginEnabled }),
    ),
  );
}

beforeEach(() => server.use(http.get("*/api/v1/me", () => problem(401, "unauthorized"))));

describe("LoginPage", () => {
  it("shows the dev form only when devLoginEnabled", async () => {
    mockConfig(false);
    const { unmount } = renderApp("/login");
    await screen.findByRole("button", { name: "使用 Google 登入" });
    expect(screen.queryByLabelText("Email")).not.toBeInTheDocument();
    unmount();

    mockConfig(true);
    renderApp("/login");
    expect(await screen.findByLabelText("Email")).toBeInTheDocument();
  });

  it("hides the Google button when Google login is disabled", async () => {
    mockConfig(true, false);
    renderApp("/login");
    await screen.findByRole("button", { name: "開發用登入" });
    expect(screen.queryByRole("button", { name: "使用 Google 登入" })).not.toBeInTheDocument();
  });

  it("shows an explanation when no login method is enabled", async () => {
    mockConfig(false, false, false);
    renderApp("/login");
    expect(await screen.findByText(/目前沒有可用的登入方式/)).toBeInTheDocument();
    expect(screen.queryByRole("button")).not.toBeInTheDocument();
  });

  it("shows only the password form when only password login is enabled", async () => {
    mockConfig(false, false, true);
    renderApp("/login");
    const email = await screen.findByLabelText("Email");
    expect(email).toHaveAttribute("autocomplete", "username");
    expect(screen.getByLabelText("密碼")).toHaveAttribute("autocomplete", "current-password");
    expect(screen.queryByRole("button", { name: "使用 Google 登入" })).not.toBeInTheDocument();
    expect(screen.queryByRole("form", { name: "開發用登入" })).not.toBeInTheDocument();
  });

  it("shows password and Google together, without the dev form", async () => {
    mockConfig(false, true, true);
    renderApp("/login");
    await screen.findByLabelText("密碼");
    expect(screen.getByRole("button", { name: "使用 Google 登入" })).toBeEnabled();
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
    const { router } = renderApp("/login?returnTo=%2Fvendors");
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
    renderApp("/login");
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

  it("honours a safe returnTo when already logged in", async () => {
    mockConfig(false);
    server.use(
      http.get("*/api/v1/me", () => HttpResponse.json(ME)),
      http.get("*/api/v1/vendors", () => HttpResponse.json([])),
    );
    const { router } = renderApp("/login?returnTo=%2Fvendors");
    await screen.findByRole("heading", { name: "店家" });
    expect(router.state.location.pathname).toBe("/vendors");
  });
});
