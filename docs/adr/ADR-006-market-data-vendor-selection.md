# ADR-006: Market-data vendor selection

- **Status:** Proposed
- **Date:** 2026-04-18
- **Deciders:** Ivan (owner)

## Context

§7.6 requires a vendor for **15-minute delayed US equity quotes** powering the sell-now calculator (US-013). Constraints (in priority order):

1. **Licensing** must permit redistribution of delayed quotes to end-users inside a paid SaaS UI. This is the most subtle and most likely to disqualify candidates.
2. **GDPR / data-plane posture (§7.6).** Vendor API requests must carry only ticker symbols + (optional) timestamps — never PII, never user IDs, never grant values. Vendor must be reachable from EU-hosted infrastructure without forced US-data-plane processing of user metadata.
3. **Cost.** Must fit within the <€200/mo total infra budget (ADR-002 leaves ~€130/mo headroom). Free tier or low single-digit-€/mo preferred during validation.
4. **Coverage.** US-listed equities (NYSE, NASDAQ); 15-min delayed sufficient. Historical EOD / intraday range needed for US-013 price-band methodology (OQ-14) and ESPP purchase-date FMV lookup (US-008 / OQ-15 fallback).
5. **Operational.** API stable, documented, rate-limited reasonably, sane error responses.

The four real candidates are **Finnhub, Twelve Data, Alpha Vantage, and Marketstack**. IEX Cloud is excluded — IEX Cloud was discontinued (the legacy IEX Cloud product was sunset; current IEX-branded data offerings are enterprise-priced and unsuitable for v1). Polygon.io and Tradier exist but are priced for higher usage tiers.

> **Honest uncertainty:** vendor licensing terms and pricing change frequently. The comparisons below reflect the architect's best understanding from training data and require **independent verification at contract time** by Ivan or the security-engineer. This ADR commits to a primary + fallback shape; specific vendor selection is conditional on that verification.

## Decision

**Primary vendor: Finnhub** (free tier for validation; upgrade to Finnhub Standard at ~$50/mo if validation succeeds and rate limits bind).

**Reasoning:**

- **Licensing:** Finnhub's free tier is documented as "free for personal and academic use." The **Standard plan** ($50/mo at last public pricing) explicitly permits commercial use including redistribution within a SaaS application. This crosses the licensing constraint that disqualifies Alpha Vantage's free tier (personal/hobby only) for a B2C-monetized Orbit. **Verification required:** Ivan must read Finnhub's current ToS at contract time and confirm SaaS-redistribution is in scope of the chosen plan.
- **Coverage:** US equities, 15-min delayed (real-time also available but explicitly out of scope for v1). Historical OHLC available for ESPP FMV lookup. Intraday day high/low available for OQ-14 price-band methodology.
- **Data plane:** REST API over HTTPS. Requests carry only `symbol` (and an API key in a header). No user metadata. API key is a single shared service credential, not per-user. EU egress to Finnhub's CDN is acceptable; **no PII leaves Orbit.**
- **Cost:** Free during validation (60 calls/min limit). $50/mo Standard plan if needed. Comfortably inside budget.
- **Operational:** Documented, stable, well-rate-limited, used widely by retail finance apps.

**Fallback vendor: Twelve Data** (stand-by; same data-plane posture, similar pricing).

Twelve Data is the architectural fallback because (a) similar API surface, (b) same delayed-equity coverage, (c) explicit commercial tier with redistribution rights, (d) EU-friendly. Switching from Finnhub to Twelve Data should be a one-day implementation effort given the abstraction in this ADR.

**Alpha Vantage is rejected** for v1 not on technical grounds but on licensing: their free tier's ToS explicitly restricts to "personal, non-commercial use," which is incompatible with Orbit being B2C monetized. Their commercial tier (~$50–250/mo) is acceptable but they are overall less feature-competitive than Finnhub at the same price.

**Marketstack** is rejected — EOD-focused, weaker intraday coverage, less responsive support reputation.

### Architectural shape (vendor-agnostic)

The vendor is wired behind a `MarketDataProvider` trait in the Rust backend:

```rust
pub trait MarketDataProvider: Send + Sync {
    fn vendor(&self) -> Vendor;

    fn quote(&self, ticker: &Ticker) -> Result<Quote, MarketDataError>;
    fn historical_close(&self, ticker: &Ticker, on: NaiveDate) -> Result<Quote, MarketDataError>;
    fn intraday_range(&self, ticker: &Ticker, on: NaiveDate) -> Result<IntradayRange, MarketDataError>;
}

pub struct Quote {
    pub ticker: Ticker,
    pub price_usd: Decimal,
    pub as_of: DateTime<Utc>,         // vendor's "last trade" timestamp
    pub delay_minutes: u8,            // documented vendor delay; 15 for free/standard tiers
    pub vendor: Vendor,
}

pub struct IntradayRange {
    pub ticker: Ticker,
    pub day_high: Decimal,
    pub day_low: Decimal,
    pub session_date: NaiveDate,
    pub vendor: Vendor,
}
```

`FinnhubProvider` is the production implementation; `TwelveDataProvider` is the standby implementation. A test `StubProvider` is used in CI.

### Caching

Quotes are written to `market_quotes_cache` (ADR-005) keyed by `ticker`, with a **15-minute TTL**:

- API request from a Rust handler → cache check → if fresh, serve cached; if stale or absent, vendor fetch → write to cache → serve.
- Cache is **shared across users** (the quote for `AAPL` is the same for everyone). This caps vendor-call volume to roughly *(distinct tickers held by active sell-now users) × 4 calls/hour*, which is well inside Finnhub free-tier limits at validation scale.
- TTL of exactly 15 minutes aligns with the vendor's quote-delay window: serving anything older than 15 minutes from the same delayed source means we are 30 minutes behind real-time, which the staleness guard treats as stale.

### Staleness handling (per §7.6 and US-013 acceptance)

- Every `Quote` carries `as_of`. The UI displays it as *"Source: Finnhub, 15-min delayed, quote as of HH:MM CET"*.
- **Stale threshold: 30 minutes** from `as_of`. If the cached quote is older than this and a refresh fails, the sell-now calculator UI:
  - Shows a clear "quote unavailable — enter price manually or retry" state.
  - Does not silently compute with stale data.
  - Allows the user to override price manually (US-013 already requires the override field).
- US market hours are honored: outside US trading hours, the most recent close is the legitimate quote and is **not flagged stale** until the next session opens. Implementation detail: vendor `as_of` will reflect the last trade; UI labels weekend/closed-market quotes accordingly.

### Graceful degradation

- **Vendor returns an error / 5xx / times out:** retry once with exponential backoff (≤2 s). On second failure, fall back to most recent cached quote with a "stale, vendor unreachable" indicator. Sell-now compute is allowed if user explicitly acknowledges; otherwise blocked.
- **Vendor exhausts our rate limit / quota:** transparent failover to the standby provider (Twelve Data) if implemented. v1 launches with Finnhub only; Twelve Data integration is a v1.1 hardening item.
- **Both vendors down + no fresh cache + no historical fallback:** sell-now disabled with a clear error per US-013 acceptance ("quote unavailable — enter price manually"). The user can still proceed with a manually-entered override price.

### Security-engineer hand-off concerns

- API key for vendor stored in secrets store (OS-level for v1; rotated annually).
- Outbound vendor traffic should be allow-listed at the host firewall to vendor's published egress endpoints.
- No request to the vendor includes any user identifier; verify via integration test that the outbound request body/headers contain only the API key + ticker.
- DPA with vendor not strictly required (no PII transmitted) but vendor's privacy policy and data-handling stance should be reviewed.

## Alternatives considered

- **IEX Cloud.** Discontinued in its hobbyist-friendly form. Excluded.
- **Alpha Vantage.** Free tier is hobby-only; commercial tier is competitive but Finnhub edges it on intraday range coverage. Rejected on free-tier licensing being incompatible with B2C monetization.
- **Yahoo Finance (unofficial APIs).** Tempting on price (free) but ToS explicitly disallows redistribution and the API has no SLA. Reject for production; acceptable for occasional manual lookups.
- **Polygon.io / Tradier / Intrinio.** Higher minimum spend; over-spec for v1.
- **No vendor — user-input-only price.** Considered as a v1 cheap-mode. Rejected because US-013 acceptance requires pre-population with a delayed quote and explicit "stale, enter manually" fallback; not having a default vendor degrades the core sell-now UX.

## Consequences

**Positive:**
- Free tier covers validation entirely; first ~$50/mo only triggers on real paid-user volume.
- Vendor abstraction means a switch to Twelve Data (or any future vendor) is local code change, not architectural.
- No PII crosses the wire to a non-EU vendor; data-plane posture stays clean.
- Shared 15-min cache caps vendor-call volume sharply; we will not exhaust limits at v1 scale.

**Negative / risks:**
- **Vendor licensing risk (R-7) is real and not fully discharged by this ADR.** Ivan must verify Finnhub's current commercial-tier ToS at contract time. The architectural mitigation is the `MarketDataProvider` trait + Twelve Data standby.
- Finnhub is US-headquartered; while no PII is transmitted, security-engineer should document this in the GDPR processor map for completeness.
- Free-tier rate limits (60 req/min for Finnhub at last review) bind earlier than paid limits; the shared cache plus the 15-min delay both amortize calls but a popular ticker burst could trip limits. Mitigation: server-side coalescing of in-flight requests for the same ticker.
- The "stale, enter manually" UX is critical to get right; a sloppy implementation here directly causes R-9 (user confusion) by appearing to compute against fresh data when it isn't.

**Follow-ups:**
- **Ivan / security-engineer:** verify current Finnhub commercial-tier ToS explicitly permits SaaS redistribution of delayed quotes; document the verified language in a `vendor-licensing-finnhub.md` note.
- **Ivan / security-engineer:** same verification for Twelve Data as fallback before relying on the failover.
- **Implementation engineer:** implement `FinnhubProvider`; add integration test asserting outbound request contains no user identifier; implement in-flight-request coalescing.
- **Solution-architect (second pass):** specify the exact `intraday_range` methodology mapping to OQ-14 (day high/low vs prior-close ± 5% fallback).
- **Implementation engineer:** Twelve Data standby implementation as v1.1 hardening item, not v1 launch-blocker.
