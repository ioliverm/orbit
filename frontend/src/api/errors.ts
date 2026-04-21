// API error model (ADR-010 §7 / SEC-051).
//
// The backend emits:
//   { "error": { "code": "<stable-id>", "message": "...", "details"?: {...} } }
//
// We surface every non-2xx as an AppError. Handlers switch on `code` only —
// `message` is a fallback; most UI copy is authored per-route in the i18n
// catalog keyed off the code.

export type AppErrorKind =
  | 'validation'
  | 'auth'
  | 'unauthenticated'
  | 'captcha_required'
  | 'csrf'
  | 'onboarding.required'
  | 'not_found'
  | 'rate_limited'
  | 'request_malformed'
  | 'not_implemented'
  | 'server_internal'
  | 'network'
  | 'unknown';

export interface AppErrorDetails {
  stage?: string;
  fields?: Array<{ field: string; code: string }>;
  [k: string]: unknown;
}

export class AppError extends Error {
  public readonly code: AppErrorKind | string;
  public readonly status: number;
  public readonly details: AppErrorDetails;
  public readonly retryAfterSecs: number | null;

  constructor(opts: {
    code: AppErrorKind | string;
    message: string;
    status: number;
    details?: AppErrorDetails;
    retryAfterSecs?: number | null;
  }) {
    super(opts.message);
    this.name = 'AppError';
    this.code = opts.code;
    this.status = opts.status;
    this.details = opts.details ?? {};
    this.retryAfterSecs = opts.retryAfterSecs ?? null;
  }

  isValidation(): boolean {
    return this.code === 'validation';
  }

  isUnauthenticated(): boolean {
    return this.code === 'unauthenticated';
  }

  isOnboardingRequired(): boolean {
    return this.code === 'onboarding.required';
  }

  isRateLimited(): boolean {
    return this.code === 'rate_limited';
  }

  isCaptchaRequired(): boolean {
    return this.code === 'captcha_required';
  }

  isNetwork(): boolean {
    return this.code === 'network';
  }

  /** Slice 3 OCC 409 per AC-10.5. */
  isStaleClientState(): boolean {
    return this.status === 409 || this.code === 'resource.stale_client_state';
  }

  /** First validation-field code for a given field name, or null. */
  fieldCode(field: string): string | null {
    const fields = this.details.fields ?? [];
    const hit = fields.find((f) => f.field === field);
    return hit ? hit.code : null;
  }
}

export function isAppError(e: unknown): e is AppError {
  return e instanceof AppError;
}
