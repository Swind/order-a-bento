import createClient from "openapi-fetch";
import type { paths } from "./schema";

// Absolute base so that `Request` construction also works outside a browser page (tests).
const baseUrl = typeof location !== "undefined" ? location.origin : "/";
export const api = createClient<paths>({
  baseUrl,
  credentials: "include",
  // Resolve fetch lazily so that test-time interception (msw) is honoured.
  fetch: (request) => globalThis.fetch(request),
});

/** RFC 9457 problem details body returned by the backend. */
export interface ProblemDetails {
  type: string;
  title: string;
  status: number;
  detail?: string;
  code?: string;
  /** Offending request field (validation / conflict errors). */
  field?: string;
  /** Id of the existing record behind a uniqueness conflict. */
  existingId?: string;
  /** Id of the AI import run behind a failed upload. */
  runId?: string;
  /** Per-entry errors (`invalid_changes`): `path` points into the request body. */
  errors?: ProblemFieldError[];
}

export interface ProblemFieldError {
  path: string;
  message: string;
}

export class ApiError extends Error {
  readonly type: string;
  readonly status: number;
  readonly detail?: string;
  readonly code?: string;
  readonly field?: string;
  readonly existingId?: string;
  readonly runId?: string;
  readonly errors?: ProblemFieldError[];

  constructor(problem: ProblemDetails) {
    super(problem.detail ?? problem.title);
    this.name = "ApiError";
    this.type = problem.type;
    this.status = problem.status;
    this.detail = problem.detail;
    this.code = problem.code;
    this.field = problem.field;
    this.existingId = problem.existingId;
    this.runId = problem.runId;
    this.errors = problem.errors;
  }
}

function fromBody(body: Partial<ProblemDetails>, response: Response): ApiError {
  return new ApiError({
    type: body.type ?? "about:blank",
    title: body.title ?? response.statusText,
    status: body.status ?? response.status,
    detail: body.detail,
    code: body.code,
    field: body.field,
    existingId: body.existingId,
    runId: body.runId ?? undefined,
    errors: body.errors ?? undefined,
  });
}

/** Build an ApiError from a non-ok Response (parses application/problem+json when present). */
export async function toApiError(response: Response): Promise<ApiError> {
  const contentType = response.headers.get("content-type") ?? "";
  if (contentType.includes("json")) {
    try {
      return fromBody((await response.json()) as Partial<ProblemDetails>, response);
    } catch {
      // fall through to generic error
    }
  }
  return new ApiError({
    type: "about:blank",
    title: response.statusText || "Request failed",
    status: response.status,
  });
}

/** Unwrap an openapi-fetch result: return data, or throw ApiError for non-ok responses. */
export async function unwrap<T>(result: {
  data?: T;
  error?: unknown;
  response: Response;
}): Promise<T> {
  if (result.response.ok) return result.data as T;
  // openapi-fetch has already consumed the body and exposes it as `error`.
  if (result.error && typeof result.error === "object") {
    throw fromBody(result.error as Partial<ProblemDetails>, result.response);
  }
  throw await toApiError(result.response);
}
