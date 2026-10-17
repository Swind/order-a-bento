import { screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { http, HttpResponse } from "msw";
import { beforeEach, describe, expect, it } from "vitest";
import { ME, problem, server } from "@/test/server";
import { renderApp } from "@/test/render";

function mockConfig(devLoginEnabled: boolean, googleEnabled = true) {
  server.use(
    http.get("*/api/v1/auth/config", () => HttpResponse.json({ googleEnabled, devLoginEnabled })),
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

  it("disables Google login with an explanation when not configured", async () => {
    mockConfig(true, false);
    renderApp("/login");
    expect(await screen.findByRole("button", { name: "使用 Google 登入" })).toBeDisabled();
    expect(screen.getByText(/尚未設定 Google 登入/)).toBeInTheDocument();
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
