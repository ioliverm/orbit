// Locale-aware number + date formatting helpers (G-13, G-14).
//
// Share counts are scaled bigints (1 unit = 1/10_000 share); for Slice 1
// we always surface whole shares, so `formatShares` floor-divides by
// SHARES_SCALE. ES: `30.000`, EN: `30,000`.
//
// Dates: storage ISO-8601; display in locale long form. ES `15 sep 2024`,
// EN `Sep 15, 2024`.

import { SHARES_SCALE } from './vesting';
import type { Locale } from '../i18n';

function intlLocale(locale: Locale): string {
  return locale === 'es-ES' ? 'es-ES' : 'en-US';
}

/** Format a scaled bigint share count as a locale-formatted whole integer. */
export function formatShares(scaled: bigint, locale: Locale): string {
  const whole = scaled / SHARES_SCALE;
  return new Intl.NumberFormat(intlLocale(locale)).format(Number(whole));
}

/** Format a plain integer (no scaling) per locale thousands separator. */
export function formatInt(n: number, locale: Locale): string {
  return new Intl.NumberFormat(intlLocale(locale)).format(n);
}

/** Parse an ISO-8601 date (YYYY-MM-DD) into a UTC Date. */
export function parseIsoDate(iso: string): Date {
  // `Date.UTC` avoids any local-tz shift.
  const [y, m, d] = iso.split('-').map((s) => Number(s));
  return new Date(Date.UTC(y!, (m ?? 1) - 1, d ?? 1));
}

/** Format a UTC Date as ISO-8601 (YYYY-MM-DD). */
export function toIsoDate(d: Date): string {
  const y = d.getUTCFullYear().toString().padStart(4, '0');
  const m = (d.getUTCMonth() + 1).toString().padStart(2, '0');
  const dd = d.getUTCDate().toString().padStart(2, '0');
  return `${y}-${m}-${dd}`;
}

/**
 * Format an ISO-8601 date (or Date) in the user's locale long form.
 * ES → `15 sep 2024`; EN → `Sep 15, 2024`.
 */
export function formatLongDate(source: string | Date, locale: Locale): string {
  const d = typeof source === 'string' ? parseIsoDate(source) : source;
  const fmt = new Intl.DateTimeFormat(intlLocale(locale), {
    day: 'numeric',
    month: 'short',
    year: 'numeric',
    timeZone: 'UTC',
  });
  return fmt.format(d);
}
