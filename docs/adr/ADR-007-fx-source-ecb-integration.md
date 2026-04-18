# ADR-007: FX source — ECB daily reference rate integration

- **Status:** Proposed
- **Date:** 2026-04-18
- **Deciders:** Ivan (owner)

## Context

§7.7 fixes the canonical FX mid-rate source: **ECB daily reference rate**, USD↔EUR primary in v1. This is a free, authoritative, EU-published source — no licensing concern, no vendor dependency, no PII concern. The architectural work is the ingestion mechanism, fallback behavior, and how user-overrides are persisted for audit (§7.9).

Key ECB facts that drive the design:

- **Source URL:** `https://www.ecb.europa.eu/stats/eurofxref/eurofxref-daily.xml` (the historical-90-day file is `eurofxref-hist-90d.xml`; the full historical file is `eurofxref-hist.xml`).
- **Publication time:** ~16:00 CET on TARGET business days. (Sometimes slips to 16:30; rarely later.)
- **Non-publication days:** weekends + TARGET2 holidays. TARGET holidays are: 1 January, Good Friday, Easter Monday, 1 May (Labour Day), 25 December, 26 December. (Note: TARGET no longer observes national holidays beyond these; the list is small and stable.)
- **Format:** XML with `<Cube currency="USD" rate="X.XXXX">` entries quoted as **EUR base** (1 EUR = X foreign units). USD↔EUR conversion is `eur = usd / rate_usd`, `usd = eur * rate_usd`.
- **Rate basis:** mid-rate, no spread. Spread is applied separately per §7.7 (default 1.5%, user-adjustable, sensitivity bands at 0% and 3%).

## Decision

### Ingestion

The Rust worker binary (`orbit worker`) runs a scheduled job:

- **Cadence:** daily at **17:00 Europe/Madrid** (which is 17:00 CET / CEST as Madrid follows the same offset). This is ~1 hour after the typical ECB publication window — gives ECB time for the occasional 16:30 slip.
- **Mechanism:** the worker has an internal scheduler (`tokio-cron-scheduler` or equivalent — implementation detail). No external cron dependency required.
- **Fetch:** HTTP GET to the daily XML URL. Parse with a small `quick-xml` deserializer. Extract today's rate for each currency we care about (v1: USD only; future-proofed by storing all rates ECB publishes).
- **Persistence:** Append-only `INSERT` into `fx_rates` (ADR-005) with:
  - `date` = ECB publication date (parsed from the XML's `<Cube time="YYYY-MM-DD">`)
  - `base_currency` = EUR
  - `quote_currency` = USD (and any others published)
  - `rate` = ECB rate (Decimal, full precision as published)
  - `source` = `ecb_daily_reference`
  - `fetched_at` = now()
  - `ecb_publication_date` = parsed XML date

- **Idempotency:** `(source, date, base_currency, quote_currency)` is a unique key. Re-running the fetch is a no-op if today's rate is already stored. This is important for retry behavior and for the "fetch on demand" path described below.

### Non-publication days (weekends, TARGET holidays)

When the worker runs on a weekend or TARGET holiday:

- The fetch returns yesterday's (or last business day's) XML — ECB serves the most recent published file. The worker recognizes this by comparing the XML's `<Cube time>` attribute to today's date.
- If the date is older than today, the worker **does not insert a new row** (the unique key would conflict, and inserting would falsely imply a fresh publication).
- The application's FX-lookup code (see below) has a separate fallback policy for "no rate published today."

### Application-side FX lookup

When a calculation needs a USD↔EUR rate for a given date:

```rust
fn lookup_rate(quote_currency: Currency, on: NaiveDate) -> FxLookupResult {
    // 1. Try exact match for `on`.
    if let Some(r) = fetch_fx_rate(EUR, quote_currency, on, "ecb_daily_reference") {
        return FxLookupResult::Fresh(r);
    }
    // 2. Walk backwards up to N business days for last-published rate.
    for offset in 1..=MAX_FALLBACK_DAYS {
        let d = on - chrono::Duration::days(offset);
        if let Some(r) = fetch_fx_rate(EUR, quote_currency, d, "ecb_daily_reference") {
            return FxLookupResult::Stale {
                rate: r,
                published_on: d,
                age_days: offset,
            };
        }
    }
    // 3. No rate within fallback window.
    FxLookupResult::Unavailable
}
```

`MAX_FALLBACK_DAYS = 7` (covers the longest realistic non-publication gap: a Christmas/New Year window with weekends).

`FxLookupResult::Stale` is **not silently substituted**. The UI surfaces:

- "ECB rate as of YYYY-MM-DD applied (today not yet published)" with a staleness indicator on every numeric output that depends on FX.
- Stamped on the calculation row's `result` JSONB so exports (ADR-008) carry the same disclosure.

`FxLookupResult::Unavailable` is treated like the market-quote-unavailable case in ADR-006: sell-now compute is blocked unless the user provides an FX override. Scenario / annual-IRPF compute either uses a user-provided rate or fails with a clear error.

### Fetch-on-demand fallback

The scheduled job is the primary path. As a defence against worker downtime around publication time, **the application also fetches on first need** if today's rate is missing and current time is past 17:00 Madrid. This is a synchronous fetch in the request path, with a hard 5-second timeout, and it writes to the same `fx_rates` table on success. If it fails, the request proceeds with `FxLookupResult::Stale` (using yesterday's rate) and the UI staleness indicator engages.

### ECB unreachable for multiple days

If ECB itself is unreachable (HTTP errors, DNS, timeout) for multiple consecutive days:

- Application keeps serving the last-known rate as `Stale` per the fallback policy.
- The UI staleness indicator escalates beyond N days: at 1 day it's a small footnote; at 3+ days it becomes a prominent banner ("FX rates have not refreshed for N days; calculations may be materially off — review with your gestor").
- An operational alert fires at 2 consecutive failed fetch attempts; on-call (i.e., Ivan) investigates.

ECB has high availability historically; sustained multi-day outages are not a realistic worry, but the UX must degrade honestly.

### User overrides

Per §7.7, the user may override the mid-rate as well as the spread. Stored as a separate field on the `sell_now_calculations` / `scenarios` row, not as a write to `fx_rates`:

- `user_fx_mid_override_usd_per_eur: Option<Decimal>`
- `user_fx_spread_bps: Option<i32>` (default 150 bps = 1.5%)

When either is set, the calculation's `result` and the export footer (ADR-008) explicitly note: *"User-overridden FX mid: 1 EUR = 1.0850 USD (entered by user). User-set spread: 2.0%. ECB reference rate as of 2026-04-17 was 1 EUR = 1.0823 USD for comparison."*

`audit_log` records the override event (`fx.override_applied`) per §7.9 audit-log policy; **the override values themselves are stored on the calculation row, not in the audit log payload** to honor the §7.2 data-minimization constraint (audit-log payloads must not contain numeric inputs/outputs).

### Spread + sensitivity bands

The mid-rate from `fx_rates` is the central input. The sell-now calculator (and any FX-dependent scenario) computes outputs at three FX rates per §7.7 / US-013 acceptance:

- `mid * (1 - 0.00)` — best-case wholesale (0% spread)
- `mid * (1 - user_spread)` — user's chosen spread (default 1.5%)
- `mid * (1 - 0.03)` — worst-case retail (3% spread)

These three rates produce three EUR-equivalent outputs; the UI presents them as a band per §7.4.

## Alternatives considered

- **Use a commercial FX API (Open Exchange Rates, Fixer, exchangerate.host).** Rejected. ECB is free, authoritative, and the canonical source the gestor expects to see in tax worksheets. A commercial API adds dependency, cost, and creates a "why didn't you use ECB?" question on every export.
- **Monthly average instead of daily.** OQ-09's existing default chose daily; this ADR preserves that choice. Daily aligns with how AEAT actually expects FX in many contexts (Modelo 720 valuation date, sale-date FX). Monthly average is available as a future enhancement — store daily, derive monthly trivially.
- **External cron triggering an HTTP endpoint.** Rejected; adds infra surface. The worker binary's internal scheduler is sufficient at this scale.
- **Pre-load ECB historical 90-day file on first deploy + boot, then daily incremental.** Yes — adopt this. On worker startup, if the `fx_rates` table is empty or missing recent rows, ingest the 90-day historical file. Cheap, idempotent, gives the system a useful history immediately.

## Consequences

**Positive:**
- Zero licensing risk, zero vendor cost, no PII data-plane concerns.
- ECB is the source a Spanish gestor expects; export footers can cite it confidently.
- Append-only `fx_rates` is naturally an audit trail: any historical calculation can be replayed against the same rate.
- Fallback behavior is honest — staleness is surfaced, never silently absorbed.

**Negative / risks:**
- ECB does not publish weekends/holidays; users computing a Saturday sell-now see a "Friday rate applied" indicator. This is correct but requires good UX copy to avoid confusion.
- Internal worker scheduler means worker downtime around 17:00 Madrid risks missing same-day publication; mitigated by the fetch-on-demand fallback in the request path.
- Decimal precision: ECB publishes 4 decimal places; calculations should preserve full precision and only round at display. Trivial but easy to get wrong.

**Follow-ups:**
- **Implementation engineer:** implement the worker scheduler + fetch + parser; on worker startup, ingest the ECB 90-day historical file if `fx_rates` is sparse.
- **Implementation engineer:** implement the fetch-on-demand request-path fallback with 5-second timeout.
- **Implementation engineer:** UI copy for staleness indicator (1 day = subtle, 3+ days = prominent banner).
- **Solution-architect (second pass):** confirm exactly which non-USD currencies (if any) v1 needs (probably none; deferred to v1.1+ as Orbit goes multi-jurisdiction).
- **Operational:** monitoring alert on 2 consecutive failed ECB fetches.
