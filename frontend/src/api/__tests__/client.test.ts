// API client unit tests (ADR-010 §7 envelope, SEC-188 CSRF).
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import {
  apiRequest,
  readCookie,
  setOnboardingRequiredSink,
  setUnauthenticatedSink,
} from '../client';
import { AppError } from '../errors';

interface MockResponseOpts {
  status: number;
  json?: unknown;
  headers?: Record<string, string>;
}

function mockFetchOnce(opts: MockResponseOpts): void {
  const body = opts.json !== undefined ? JSON.stringify(opts.json) : '';
  const res = new Response(body, {
    status: opts.status,
    headers: new Headers(opts.headers ?? {}),
  });
  vi.stubGlobal('fetch', vi.fn(async () => res));
}

describe('apiRequest', () => {
  beforeEach(() => {
    // Reset cookies between tests.
    document.cookie = 'orbit_csrf=; Max-Age=0; Path=/';
  });

  afterEach(() => {
    vi.unstubAllGlobals();
    setUnauthenticatedSink(null);
    setOnboardingRequiredSink(null);
  });

  it('attaches X-CSRF-Token on POST when cookie is present', async () => {
    document.cookie = 'orbit_csrf=abc123; Path=/';
    // JSDOM's Response disallows a body on 204; use 200 {} for the spy.
    const spy = vi.fn(async () => new Response('{}', { status: 200 }));
    vi.stubGlobal('fetch', spy);

    await apiRequest('POST', '/auth/signout');

    expect(spy).toHaveBeenCalledTimes(1);
    const call = spy.mock.calls[0] as unknown as [string, RequestInit];
    const init = call[1];
    const headers = init.headers as Record<string, string>;
    expect(headers['X-CSRF-Token']).toBe('abc123');
    expect(init.credentials).toBe('include');
  });

  it('does NOT attach X-CSRF-Token on GET', async () => {
    document.cookie = 'orbit_csrf=abc123; Path=/';
    const spy = vi.fn(async () => new Response('{}', { status: 200 }));
    vi.stubGlobal('fetch', spy);

    await apiRequest('GET', '/auth/me');
    const call = spy.mock.calls[0] as unknown as [string, RequestInit];
    const init = call[1];
    const headers = init.headers as Record<string, string>;
    expect(headers['X-CSRF-Token']).toBeUndefined();
  });

  it('parses the error envelope into AppError', async () => {
    mockFetchOnce({
      status: 422,
      json: { error: { code: 'validation', message: 'invalid', details: { fields: [{ field: 'email', code: 'required' }] } } },
    });

    await expect(apiRequest('POST', '/auth/signup', { email: '' })).rejects.toThrow(AppError);
    // Re-run to inspect properties
    mockFetchOnce({
      status: 422,
      json: { error: { code: 'validation', message: 'invalid', details: { fields: [{ field: 'email', code: 'required' }] } } },
    });
    try {
      await apiRequest('POST', '/auth/signup', { email: '' });
    } catch (e) {
      const err = e as AppError;
      expect(err.code).toBe('validation');
      expect(err.status).toBe(422);
      expect(err.fieldCode('email')).toBe('required');
    }
  });

  it('invokes the unauthenticated sink on 401 "unauthenticated"', async () => {
    mockFetchOnce({
      status: 401,
      json: { error: { code: 'unauthenticated', message: 'no session' } },
    });
    const sink = vi.fn();
    setUnauthenticatedSink(sink);

    try {
      await apiRequest('GET', '/auth/me');
    } catch {
      /* expected */
    }
    expect(sink).toHaveBeenCalledTimes(1);
  });

  it('invokes the onboarding-required sink on 403 "onboarding.required"', async () => {
    mockFetchOnce({
      status: 403,
      json: {
        error: {
          code: 'onboarding.required',
          message: 'complete disclaimer',
          details: { stage: 'disclaimer' },
        },
      },
    });
    const sink = vi.fn();
    setOnboardingRequiredSink(sink);

    try {
      await apiRequest('GET', '/grants');
    } catch {
      /* expected */
    }
    expect(sink).toHaveBeenCalledWith('disclaimer', expect.any(AppError));
  });

  it('surfaces network failures as AppError code=network', async () => {
    vi.stubGlobal(
      'fetch',
      vi.fn(async () => {
        throw new TypeError('offline');
      }),
    );
    try {
      await apiRequest('GET', '/auth/me');
      expect.fail('should have thrown');
    } catch (e) {
      const err = e as AppError;
      expect(err.isNetwork()).toBe(true);
      expect(err.status).toBe(0);
    }
  });

  it('reads Retry-After on 429', async () => {
    const headers = new Headers({ 'Retry-After': '42' });
    vi.stubGlobal(
      'fetch',
      vi.fn(
        async () =>
          new Response(JSON.stringify({ error: { code: 'rate_limited', message: 'slow' } }), {
            status: 429,
            headers,
          }),
      ),
    );
    try {
      await apiRequest('POST', '/auth/signin', { email: 'a@b.co', password: 'x' });
    } catch (e) {
      const err = e as AppError;
      expect(err.isRateLimited()).toBe(true);
      expect(err.retryAfterSecs).toBe(42);
    }
  });

  it('readCookie returns null when cookie is absent', () => {
    document.cookie = 'other=x; Path=/';
    expect(readCookie('orbit_csrf')).toBeNull();
  });

  it('readCookie returns decoded value', () => {
    document.cookie = `orbit_csrf=${encodeURIComponent('token with space')}; Path=/`;
    expect(readCookie('orbit_csrf')).toBe('token with space');
  });
});
