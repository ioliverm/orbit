// Slim API fetch wrapper (ADR-009 follow-up, ADR-010 §API contract shape).
//
// Responsibilities:
//  - Adds credentials: "include" so session/refresh/csrf cookies ride along.
//  - On state-changing verbs, mirrors the `orbit_csrf` cookie into an
//    `X-CSRF-Token` header (SEC-188 double-submit).
//  - Parses the JSON error envelope and throws a typed AppError.
//  - Surfaces 401 `unauthenticated` and 403 `onboarding.required` to an
//    injectable sink so the router can redirect without this module owning
//    the router reference.
//  - Network failures surface as AppError { code: "network" }.
//
// Kept intentionally small (~120 LOC target per ADR-010 §4). No generated
// client — types live in src/api/auth.ts etc.

import { AppError, type AppErrorDetails } from './errors';

export type HttpMethod = 'GET' | 'POST' | 'PUT' | 'PATCH' | 'DELETE';

export const STATE_CHANGING_METHODS: ReadonlySet<HttpMethod> = new Set<HttpMethod>([
  'POST',
  'PUT',
  'PATCH',
  'DELETE',
]);

const CSRF_COOKIE = 'orbit_csrf';

export interface ApiRequestOpts {
  signal?: AbortSignal;
  /** Override the default Content-Type: application/json when body is non-JSON. */
  contentType?: string;
}

/** Sink invoked when an endpoint returns 401 `unauthenticated`. */
type UnauthenticatedSink = (err: AppError) => void;
/** Sink invoked when an endpoint returns 403 `onboarding.required`. */
type OnboardingSink = (stage: string, err: AppError) => void;

let unauthenticatedSink: UnauthenticatedSink | null = null;
let onboardingSink: OnboardingSink | null = null;

export function setUnauthenticatedSink(sink: UnauthenticatedSink | null): void {
  unauthenticatedSink = sink;
}

export function setOnboardingRequiredSink(sink: OnboardingSink | null): void {
  onboardingSink = sink;
}

/** Read a cookie value by name. Returns null if absent. */
export function readCookie(name: string): string | null {
  if (typeof document === 'undefined') return null;
  const raw = document.cookie;
  if (!raw) return null;
  const prefix = `${name}=`;
  for (const part of raw.split(';')) {
    const trimmed = part.trim();
    if (trimmed.startsWith(prefix)) {
      return decodeURIComponent(trimmed.slice(prefix.length));
    }
  }
  return null;
}

/**
 * Issue a JSON request against the backend API.
 *
 * - `path` is relative to `/api/v1` (e.g. `/auth/me`). Absolute paths
 *   starting with `/api/` are also accepted as-is.
 * - `body` is JSON-serialised when provided.
 */
export async function apiRequest<T>(
  method: HttpMethod,
  path: string,
  body?: unknown,
  opts: ApiRequestOpts = {},
): Promise<T> {
  const url = path.startsWith('/api/') ? path : `/api/v1${path.startsWith('/') ? '' : '/'}${path}`;

  const headers: Record<string, string> = {
    Accept: 'application/json',
  };
  if (body !== undefined) {
    headers['Content-Type'] = opts.contentType ?? 'application/json';
  }
  if (STATE_CHANGING_METHODS.has(method)) {
    const csrf = readCookie(CSRF_COOKIE);
    if (csrf) headers['X-CSRF-Token'] = csrf;
  }

  // exactOptionalPropertyTypes: build init conditionally rather than
  // setting keys to undefined.
  const init: RequestInit = { method, credentials: 'include', headers };
  if (body !== undefined) init.body = JSON.stringify(body);
  if (opts.signal) init.signal = opts.signal;

  let response: Response;
  try {
    response = await fetch(url, init);
  } catch (cause) {
    throw new AppError({
      code: 'network',
      message: cause instanceof Error ? cause.message : 'Network error',
      status: 0,
    });
  }

  if (response.status === 204) {
    return undefined as T;
  }

  const text = await response.text();
  const payload = text ? safeJson(text) : null;

  if (!response.ok) {
    const err = buildAppError(response, payload);
    dispatchSideEffects(err);
    throw err;
  }

  return (payload ?? {}) as T;
}

function safeJson(text: string): unknown {
  try {
    return JSON.parse(text);
  } catch {
    return null;
  }
}

function buildAppError(response: Response, payload: unknown): AppError {
  const envelope = (payload as { error?: { code?: string; message?: string; details?: AppErrorDetails } } | null)
    ?.error;
  const retryAfterHeader = response.headers.get('Retry-After');
  const retryAfterSecs = retryAfterHeader ? parseInt(retryAfterHeader, 10) : null;

  return new AppError({
    code: envelope?.code ?? inferCodeFromStatus(response.status),
    message: envelope?.message ?? response.statusText ?? 'Request failed',
    status: response.status,
    details: envelope?.details ?? {},
    retryAfterSecs: Number.isFinite(retryAfterSecs as number) ? retryAfterSecs : null,
  });
}

function inferCodeFromStatus(status: number): string {
  switch (status) {
    case 400:
      return 'request_malformed';
    case 401:
      return 'unauthenticated';
    case 403:
      return 'csrf';
    case 404:
      return 'not_found';
    case 422:
      return 'validation';
    case 429:
      return 'rate_limited';
    case 501:
      return 'not_implemented';
    default:
      return status >= 500 ? 'server_internal' : 'unknown';
  }
}

function dispatchSideEffects(err: AppError): void {
  if (err.code === 'unauthenticated' && unauthenticatedSink) {
    unauthenticatedSink(err);
    return;
  }
  if (err.code === 'onboarding.required' && onboardingSink) {
    const stage = typeof err.details.stage === 'string' ? err.details.stage : '';
    onboardingSink(stage, err);
  }
}
