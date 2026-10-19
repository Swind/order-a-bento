import { setupServer } from "msw/node";
import { http, HttpResponse } from "msw";

/** Shared msw server; tests register handlers with `server.use(...)`. */
export const server = setupServer(
  http.get("*/api/v1/sharing-groups", () => HttpResponse.json([])),
);

export function problem(status: number, code: string, extra: Record<string, unknown> = {}) {
  return new Response(
    JSON.stringify({ type: "about:blank", title: "Error", status, detail: "English detail", code, ...extra }),
    { status, headers: { "content-type": "application/problem+json" } },
  );
}

export const ME = {
  id: "o1",
  email: "ann@example.com",
  displayName: "Ann",
  avatarUrl: null,
};
