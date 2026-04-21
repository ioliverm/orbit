# Orbit v1 — Slice 3 acceptance criteria

| Field       | Value                                                      |
|-------------|------------------------------------------------------------|
| Version     | 1.0                                                        |
| Date        | 2026-04-21                                                 |
| Owner       | requirements-analyst (Ivan Oliver)                         |
| Slice       | Slice 3 — "FX pipeline + Modelo 720 passive UX + FMV capture" (see `v1-slice-plan.md` v1.4) |
| Boundary    | ECB FX ingestion pipeline · dashboard paper-gains tile (EUR, bands 0 % / 1.5 % / 3 %) · Modelo 720 threshold alert (US-007 ACs 1/2/4 — banner only) · rule-set chip in footer on FX-dependent surfaces · per-vest FMV capture on `vesting_events` · editable past vesting events on grant-detail with override preservation · user-entered current price per ticker + per-grant override. **No Stripe. No billing. No paid/free gating. No tax math. No IRPF projection. No Modelo 720 worksheet PDF. No scenarios. No sell-now. No market-data vendor. No NSO exercise-FMV capture. No Art. 7.p eligibility evaluation. No `sessions.country_iso2` population.** |
| Related     | Spec US-007 (ACs 1, 2, 4 — banner only; AC 3 worksheet is Slice 6), spec L319/L334 (RSU basis = FMV-at-vest; ESPP basis = FMV-at-purchase); spec US-013 (sell-now, deferred to Slice 5 — referenced as the downstream consumer of the FMV data captured here); Slice-1 US-001/US-003/US-006 continuity; Slice-2 US-008 (ESPP purchases — basis-lookup continuity with this slice's FMV capture); UX screens `dashboard.html` (paper-gains tile target + M720 banner slot), `multi-grant-dashboard.html` (Slice-2 stacked envelope baseline, unchanged), `grant-detail.html` ("Precios de vesting" section host + per-grant override surface), `dashboard-slice-1.html` (starting baseline). ADR-005 entity outline (`fx_rates`), ADR-007 (ECB pipeline), ADR-016 §9.2 (superseded `sessions.country_iso2` note — see Q2 below), ADR-017 (Slice-3 technical design / DDL + override-preservation authority — authored in parallel by solution-architect). |
| v1.4 notes  | Product-owner decisions 2026-04-21, pre-locked for this slice: **(Q1)** current-price input is **per-ticker** in the user's portfolio with a per-grant override affordance on grant-detail; grants without a ticker (unlisted-company ESPP/NSO) fall back to a per-grant field. **(Q2)** `sessions.country_iso2` GeoIP population is **deferred to Slice 9**, not Slice 3. The Slice-2 column stays `NULL` through Slice 8; ADR-016 §9.2's "Slice 3 populates" note is superseded. UI's "ubicación desconocida" branch remains the default. **(Q3)** Paper-gains tile with missing FMV renders a **partial result + "complete your data" banner** listing the grants needing data; no all-or-nothing fallback, no basis-proxy substitution. **(Q4)** Bulk-fill "Aplicar FMV a todos" **SKIPS** rows that already carry a manual FMV; the confirmation dialog reports explicit skip-count copy. Five additional defaults already locked (rule-set chip copy, default `fmv_currency`, `clearOverride: true` semantics, grant-edit share-count 422, audit-log payload shape) — each surfaced in its section below. T-shirt bumped M → L by the FMV + editability expansion. |

This document is implementation-ready. Every AC below is testable as-written. Where a tester needs a specific screen state, the UX reference HTML is cited by filename. ADR-017 authors the DDL, API shapes, and state-machine details that this document intentionally does not duplicate.

## 1. In-scope stories

| Story | In Slice 3? | Notes |
|-------|-------------|-------|
| **US-001 — Create and manage grants manually** | Already in Slice 1 | Grant-CRUD logic unchanged; edit form gains a warning banner when any vesting-event override exists (AC-8.8). |
| **US-003 — Visualise vesting incl. double-trigger** | Already in Slices 1 + 2 | The cumulative envelope on grant-detail and the stacked multi-grant dashboard envelope (Slice-2 AC-8.2.8) are **unchanged** in Slice 3. Overridden rows do **not** surface a visual badge on the timeline itself — badge affordance is a Slice-4 polish concern. Tester do-not-flag. |
| **US-006 — Autonomía selection** | Already in Slice 1 | No Slice-3 change. |
| **US-007 — Modelo 720 threshold alert** | **ACs 1, 2, 4** | Banner-only surface on Dashboard + Profile when any category's currently-open row (bank accounts, real estate, derived securities) exceeds €50 000 or when the aggregate exceeds €50 000. **AC 3 worksheet PDF is Slice 6.** |
| **US-008 — ESPP Spanish-tax treatment** | Basis-lookup continuity only | `espp_purchases.fmv_at_purchase` (captured in Slice 2) is the ESPP basis input to the paper-gains tile. No tax math in Slice 3; rendimiento-del-trabajo at purchase and ahorro-base routing ship in Slices 4/5. |
| **Spec L319/L334 — RSU basis = FMV-at-vest** | Yes | `vesting_events.fmv_at_vest` columns + editable past rows on grant-detail are the net-new Slice-3 surface that gives the Slice-4 tax engine and Slice-5 sell-now calculator the basis data they need. |
| **US-013 — Sell-now calculator** | **No** (Slice 5) | Referenced as framing only: Slice-3 FMV capture exists because Slice-5 sell-now consumes it. The Slice-3 paper-gains tile is a **display-only** gains estimator on the dashboard; it is not the sell-now calculator. Tester do-not-flag: no NSO exercise-FMV capture, no `nso_exercises` table, no net-EUR landing. |
| **US-011 — GDPR DSR self-service** | Partial (data-minimization extension only) | Full DSR self-service ships in Slice 7. Slice 3 extends the data-minimization posture to FMV values, FX response bodies in logs, and override-flag counts in analytics. |
| **US-002 — CSV import** | No | Slice 8. |
| **US-004, US-006 AC #3, US-009, US-010, US-012, US-005 calculation** | No | Later slices. |

## 2. Persona & demo context

- Primary tester persona: **María**, operating across both legs of Persona B's lifecycle per spec §2.1. By Slice 3, María has completed the Slice-1 wizard + Slice-2 portfolio-fullness demo and holds a realistic multi-grant portfolio — a **pre-IPO leg** (double-trigger RSUs on an employer that has not yet IPO'd; FMV at vest is `NULL` or user-estimated from 409A) plus a **post-IPO leg** (vested RSUs on a publicly-traded US employer + one ESPP grant with two recorded purchases from Slice 2).
- Device: 14″ laptop (1440×900), Chrome + Safari. Mobile must render but is not the primary acceptance surface; mobile-specific assertions are in §11.
- Locale acceptance: **ES primary**, EN fallback. Every user-visible string passes through the i18n layer.
- Environment: local-only per ADR-015 §0a + v1.1 (`http://localhost:<port>`). No cloud URL is relevant until Slice 9. The ECB `eurofxref-daily.xml` endpoint is the **first external network call** Orbit ever makes — see AC-4.4 for the allowlist-entry expectation.

## 3. Global ACs (apply to every screen this slice ships)

Slice 3 inherits **all** Slice-1 and Slice-2 global ACs (G-1 through G-32) without re-litigation. The deltas below extend, tighten, or add new ACs; they do not replace prior wording.

### 3.1 Non-advice disclaimer — footer

- **G-1..G-4 (inherited).** Footer strip renders on every new Slice-3 page (dashboard with paper-gains tile, Profile with M720 inputs panel + threshold banner, grant-detail with "Precios de vesting" section, bulk-fill modal, rule-set explainer stub page per AC-7.1.3).
- **G-5 (extended, net-new in Slice 3).** The footer **gains a rule-set chip** on pages that render an FX-dependent number (see §7). Chip copy, locale handling, and click behaviour are specified in §7. On pages with no FX-dependent number (e.g., Sessions UI, residency-setup, Slice-1 empty dashboard before any current-price is entered), the chip does **not** render and the footer remains Slice-1 / Slice-2 copy-only.
- **G-6 (inherited).** Footer height with chip present is ≤ 48 px (chip on its own row at ≤ 640 px, inline on wider viewports); the chip must never obscure content at minimum supported height.
- **G-7 (inherited).** Footer reachable in tab-order after main content; chip is reachable in tab-order after the disclaimer copy and before any footer links (none in Slice 3).

### 3.2 Non-advice disclaimer — first-login modal

- **G-8..G-10 (inherited from Slice 1, not re-tested here).** Disclaimer gating is proven in Slice 1; Slice 3 does not add a re-acceptance trigger. Re-login during Slice 3 must not re-display the modal.

### 3.3 i18n

- **G-11 (inherited).** Every Slice-3 string ships in `es-ES` and `en`. CI lint rejects single-locale PRs.
- **G-12 (extended).** Spanish tax terms remain in Spanish even in EN locale: the Slice-1/-2 set (`IRPF`, `Modelo 720`, `Art. 7.p`, `Beckham`, `territorio común`, `foral`, `autonomía`) **plus the following Slice-3 tokens**: `ECB` (kept as acronym, already unchanged across locales), `FMV` (kept as acronym). Slice 3 introduces `Valor razonable` as the ES-native synonym for FMV — the acronym `FMV` is preferred in form labels and table headers; the long form `Valor razonable (FMV)` appears at least once per screen for discoverability. `rendimiento del trabajo`, `ahorro base`, and `Art. 37.1.d LIRPF` remain **Slice-4 concerns** and must not leak into any Slice-3 surface.
- **G-13 (inherited, extended).** Locale-aware number formatting. New FMV fields (up to 4 decimal places) render with the locale's decimal separator; the currency suffix is always explicit (`$42.8000 USD`, `€38,5000 EUR`). EUR amounts in the paper-gains tile render to 2 decimal places with a locale-appropriate thousands separator.
- **G-14 (inherited).** ISO 8601 in storage; user-locale long-form on display. `fx_rates.date`, `vest_date` edits, and `overridden_at` all render per G-14.
- **G-15 (inherited).** ES-first label testing. The "complete your data" banner copy, the bulk-fill confirmation-dialog copy, and the override-exists warning banner on grant-edit are ~40 % longer in ES than EN; test at 14″ desktop and at the 640 px mobile breakpoint for no truncation.

### 3.4 Accessibility (WCAG 2.1 AA / 2.2 AA)

- **G-16..G-20 (inherited).** Every Slice-3 page passes `axe` smoke in CI and a manual keyboard walkthrough.
- **G-21 (extended).** CI `axe` smoke runs on: dashboard with paper-gains tile (full data), dashboard with partial-data banner, Profile with M720 threshold alert active, grant-detail with "Precios de vesting" section (past-row edit open, future-row edit open, override marker visible), bulk-fill "Aplicar FMV a todos" modal, grant-edit form with override-exists warning, rule-set chip explainer stub page.
- **G-22 (inherited).** 100 %–200 % zoom without horizontal scroll on new surfaces.
- **G-23 (extended, net-new).** `is_user_override = true` vesting-event rows surface the override state via a **label + icon** ("Ajustado manualmente" / "Manually adjusted") — color alone never communicates override status. FX-staleness state on the rule-set chip likewise carries text ("ECB · 17 abr 2026 · stale 2 días") and an icon, never color alone.
- **G-24 (inherited).** `prefers-reduced-motion`: paper-gains tile band rendering and the vesting-events table edit-state transitions are instant when the preference is set.
- **G-25 (inherited).** `prefers-color-scheme: dark` renders the full dark token set for all Slice-3 screens.
- **G-33 (net-new in Slice 3).** The editable vesting-events table on grant-detail is fully keyboard-navigable: Tab advances between cells; Shift-Tab reverses; Enter submits the active cell edit; Escape cancels the active cell edit and restores the prior value. Arrow keys within a cell behave per the field type (date picker, numeric input). Focus indicator per G-19 applies to every cell in edit mode.

### 3.5 GDPR / data minimization

- **G-26 (extended).** The Slice-1/-2 payload-schema lint extends to Slice-3 analytics events. Specifically: **FMV values, current-price inputs, paper-gains EUR amounts, M720 category totals, override flags, override counts, and ECB response bodies** must never appear in an analytics event payload. Event payload may contain only the action verb and a surface identifier (e.g., `{ surface: "paper_gains_tile", verb: "view" }` or `{ surface: "vesting_event_override", verb: "create", field: "fmv" }` — the `field` is one of the three allowlisted symbolic values and carries no numeric content) plus the user UUID.
- **G-27 (inherited).** Analytics opt-in default off.
- **G-28 (extended — first external egress).** Slice 3 adds **one and only one** external network call: `GET https://www.ecb.europa.eu/stats/eurofxref/eurofxref-daily.xml` (plus `eurofxref-hist-90d.xml` for first-worker-startup bootstrap per AC-4.3.1). Per ADR-007, no PII is sent with either request (no query string, no body, no custom headers beyond a vanilla User-Agent). The nftables allowlist remains Slice-9's concern (deploy gate); in Slice 3 the developer-machine egress is unrestricted but the expectation is documented so Slice-9's allowlist entry has a precise target.
- **G-29 (inherited, extended).** Logs redact emails and raw IPs. **New Slice-3 requirement**: the ECB-fetch worker must not log the HTTP response body. Only the parsed tags used by the ingestion pipeline (`<Cube time="…">`, `<Cube currency="USD" rate="…">` and any other currencies published) are scraped into structured log fields; raw XML never appears in a structured log line. Likewise, the `GET /fx/status` (or equivalent dev affordance ADR-017 names) response is not mirrored to a log line — only the parsed freshness metadata is logged.

### 3.6 Observability + audit log

- **G-30 (inherited).** Every request logs the Slice-1 baseline fields.
- **G-31 (inherited).** Auth events continue to write to `audit_log` unchanged.
- **G-32 (extended).** Grant create/edit/delete continues to write the Slice-1/-2 audit-log entries. **New Slice-3 audit-log actions required**, all conforming to the SEC-101-strict payload allowlist (no numeric inputs/outputs, no PII, no destination fields beyond the allowlist below):
  - `fx.fetch_success` — `target_kind = "fx_rates"`, `target_id = null`; `payload_summary = { publication_date: "YYYY-MM-DD", quote_currencies: ["USD"], rows_inserted: N }`. Written by the worker once per scheduled fetch and per fetch-on-demand. `rows_inserted` is 0 for a same-day idempotent no-op. No raw rate values; `quote_currencies` is a whitelist of the ISO-4217 codes Orbit is currently ingesting (v1: `["USD"]`).
  - `fx.fetch_failure` — `target_kind = "fx_rates"`, `target_id = null`; `payload_summary = { reason: "http" | "parse" | "timeout" | "dns", attempt_number: N, attempted_at_minute: "HH:MM" }`. `attempted_at_minute` is the Madrid-local HH:MM to 1-minute granularity — never a full timestamp, never a raw IP, never a URL. An escalating alert fires on 2 consecutive failures (ADR-007).
  - `fx.bootstrap_success` — `target_kind = "fx_rates"`, `target_id = null`; `payload_summary = { historical_file: "eurofxref-hist-90d", rows_inserted: N, span_days: N }`. Written once per worker startup when the 90-day bootstrap runs (ADR-007).
  - `vesting_event.override` — `target_kind = "vesting_event"`, `target_id = vesting_event.id`; `payload_summary = { grant_id: <uuid>, fields_changed: ["vest_date" | "shares" | "fmv"] }`. The `fields_changed` array contains 1–3 symbolic values; no new values, no old values, no FMV amounts. Written on every user override of a vesting-event row (whether the row was previously un-overridden or already overridden).
  - `vesting_event.clear_override` — `target_kind = "vesting_event"`, `target_id = vesting_event.id`; `payload_summary = { grant_id: <uuid>, cleared_fields: ["vest_date" | "shares"], preserved: ["fmv"] }`. Written on a `clearOverride: true` PUT that reverts date + shares while preserving FMV (see AC-8.7). `preserved` is `[]` if the row had no FMV override to preserve.
- **G-34 (net-new in Slice 3).** The ECB worker emits a structured metric (`fx_fetch_staleness_days`) on every request-path FX lookup. This metric is log-only in Slice 3 (no dashboards, no alert route — those are Slice 9 concerns); its presence is verified in CI via a log-shape assertion.

## 4. ECB FX ingestion pipeline (new in Slice 3)

Reference: ADR-007 (ECB pipeline — authoritative). This section pins the **requirements** the Slice-3 implementation must meet; the algorithmic details of walkback, idempotency keys, and worker-scheduler wiring belong to ADR-007 + ADR-017.

### 4.1 Scheduled fetch

- **AC-4.1.1 — cadence.** Given a developer laptop with the worker binary running, when the clock reaches **17:00 Europe/Madrid** on a TARGET business day, then the worker issues a single `GET` to `https://www.ecb.europa.eu/stats/eurofxref/eurofxref-daily.xml` within 60 seconds of that wall-clock moment. Tolerance is ±60 seconds to account for scheduler jitter. No fetch runs in parallel with the scheduled fetch.
- **AC-4.1.2 — publication-date honesty.** Given a successful fetch on a TARGET business day, when the response's `<Cube time="YYYY-MM-DD">` matches today's date (in Europe/Madrid), then a new `fx_rates` row is inserted with `date = today`, `source = "ecb_daily_reference"`, `ecb_publication_date = today`, `rate = <published decimal>`, `base_currency = EUR`, `quote_currency = USD`. Full decimal precision as published (4 dp minimum); no rounding at ingestion.
- **AC-4.1.3 — idempotency.** Given a second fetch on the same day, when the row for `(source = "ecb_daily_reference", date, base_currency = EUR, quote_currency = USD)` already exists, then the insert is a no-op (ADR-007 idempotency key). No error surfaced; `fx.fetch_success` audit row is still written with `rows_inserted: 0`.

### 4.2 Non-publication-day walkback

- **AC-4.2.1 — weekend / TARGET-holiday behaviour.** Given the scheduled fetch runs on a weekend or one of the six TARGET holidays (ADR-007), when ECB responds with yesterday's (or last-business-day's) XML, then the worker does **not** insert a new row for today (the unique key would conflict with the last-published-day row if present, and inserting would falsely imply a fresh publication). The `fx.fetch_success` audit row is still written with `rows_inserted: 0` and `publication_date = <last published date>`.
- **AC-4.2.2 — request-path walkback window.** Given a request-path FX lookup for date `D` where `fx_rates` has no row for `D`, when the application issues the walkback per ADR-007 `lookup_rate`, then it walks **up to 7 calendar days** backward and returns `FxLookupResult::Stale { rate, published_on, age_days }` on first hit. No silent substitution; the caller must render the staleness indicator.
- **AC-4.2.3 — walkback exhausted.** Given no `fx_rates` row exists within the 7-day walkback window, when the lookup completes, then it returns `FxLookupResult::Unavailable`. The caller renders the unavailable state (see AC-5.5 for the dashboard's behaviour).

### 4.3 Bootstrap on first worker startup

- **AC-4.3.1 — 90-day bootstrap.** Given a cold database where `fx_rates` has zero rows (or fewer than 30 rows within the last 90 days), when the worker starts, then it fetches `https://www.ecb.europa.eu/stats/eurofxref/eurofxref-hist-90d.xml` exactly once and inserts one `fx_rates` row per `<Cube time>` × `<Cube currency>` pair. The insert is idempotent (same unique key as AC-4.1.3).
- **AC-4.3.2 — bootstrap audit.** Given a successful bootstrap, when the worker logs its completion, then one `fx.bootstrap_success` audit-log row is written per G-32 with `rows_inserted: N` and `span_days` reflecting the actual range of publication dates returned by ECB (typically 60–65 for a 90-day file due to weekends + holidays).
- **AC-4.3.3 — warm restart is a no-op.** Given a warm database where `fx_rates` already has the last 30 days populated, when the worker restarts, then no bootstrap fetch runs and no `fx.bootstrap_success` row is written. (The threshold for "already populated" is ADR-017's to pin; the requirement here is that a warm restart does not cost the user an extra external call.)

### 4.4 Fetch-on-demand fallback

- **AC-4.4.1 — on-demand trigger.** Given a request-path FX lookup for today's date where no `fx_rates` row exists for today and current wall-clock is past 17:00 Europe/Madrid, when the lookup runs, then the application issues a synchronous fetch with a **hard 5-second timeout** (ADR-007). On success, a new `fx_rates` row is inserted and the fresh rate is returned. On failure (timeout, HTTP error, parse error), the caller falls through to walkback per AC-4.2.2 and returns `FxLookupResult::Stale`.
- **AC-4.4.2 — on-demand audit.** Given an on-demand fetch success, when it completes, then an `fx.fetch_success` audit row is written with `rows_inserted: 1` (or `0` if the scheduled fetch raced ahead and already inserted — idempotency per AC-4.1.3). An on-demand fetch failure writes an `fx.fetch_failure` row.
- **AC-4.4.3 — on-demand budget.** The same request path does **not** trigger more than one on-demand fetch per user-facing request. A page render that needs 5 FX lookups does not issue 5 ECB calls; it issues at most 1 (the first) and reuses that result for the rest of the render cycle.

### 4.5 Staleness indicator (UX surface)

- **AC-4.5.1 — fresh state.** Given the FX lookup returns `FxLookupResult::Fresh`, when any Slice-3 surface renders an FX-dependent number, then the rule-set chip displays `Reglas: ECB · {fx_date} · motor v{version}` (see §7). No staleness text, no warning icon.
- **AC-4.5.2 — stale ≤ 2 days.** Given the FX lookup returns `FxLookupResult::Stale { age_days }` where `age_days ≤ 2`, when any Slice-3 surface renders an FX-dependent number, then the rule-set chip appends `· stale {age_days} día(s)` (ES) / `· stale {age_days} day(s)` (EN) with a subtle icon. No blocking banner.
- **AC-4.5.3 — stale ≥ 3 days.** Given `age_days ≥ 3`, when the dashboard renders, then a prominent banner appears at the top of the dashboard: `Los tipos de cambio ECB no se han actualizado en {N} días; las cifras en EUR pueden estar materialmente desfasadas.` (ES) / `ECB FX rates have not refreshed in {N} days; EUR figures may be materially off.` (EN). The banner is dismissible for the session only.
- **AC-4.5.4 — unavailable.** Given `FxLookupResult::Unavailable` (walkback exhausted), when the dashboard renders, then the paper-gains tile renders in an unavailable state (see AC-5.5) and no rule-set chip is shown (the chip requires a date, which does not exist in this state).

### 4.6 User FX overrides in Slice 3

- **AC-4.6.1 — Slice-3 write paths.** The Slice-3 write paths that consume FX are the **per-ticker current-price input** and the **per-grant current-price override** that drive the paper-gains tile (§5). Full "user-overridable FX mid + spread per calculation" per ADR-007 is a **Slice-4 tax-engine concern** and is explicitly deferred — no Slice-3 surface exposes FX mid or spread as a user-editable number. The chip shows what was used; it is not click-to-edit.
- **AC-4.6.2 — spread bands in Slice 3.** The paper-gains tile renders three EUR amounts using the ECB mid rate at **0 %, 1.5 %, and 3 %** spreads (§5.2) — identical to ADR-007's sell-now pattern but applied at render time rather than compute time. No user-adjustable spread field in Slice 3.

## 5. Dashboard paper-gains tile (new in Slice 3)

Reference: `docs/design/screens/dashboard.html` (paper-gains tile target) + `multi-grant-dashboard.html` (Slice-2 stacked envelope host — **unchanged**). The paper-gains tile sits **above** the Slice-2 stacked envelope; its introduction must not modify the envelope's behaviour (Slice-2 AC-8.2.1..8 remain authoritative for the envelope).

### 5.1 Tile placement and empty state

- **AC-5.1.1 — placement.** Given the user lands on the dashboard with ≥1 grant, when the page loads, then a **"Paper gains (EUR)"** tile renders above the Slice-2 multi-grant tiles and above the stacked-employer envelope. Tile headline: `Ganancias latentes (EUR)` (ES) / `Paper gains (EUR)` (EN).
- **AC-5.1.2 — empty-state (no current price entered).** Given the user has never entered a current-price input for any ticker in their portfolio, when the tile renders, then it shows an empty state: `Introduce el precio actual de tus tickers para ver las ganancias latentes en EUR.` (ES) / `Enter current prices for your tickers to see paper gains in EUR.` (EN) with a primary CTA `Introducir precios` / `Enter prices`. No EUR number is rendered.
- **AC-5.1.3 — empty-state (no ticker).** Given the user's portfolio consists only of unlisted-company grants (no ticker on any `grants` row), when the tile renders, then the empty-state copy changes to: `Introduce el precio actual por grant (tu empleador aún no cotiza).` (ES) / `Enter current price per grant (your employer is not yet publicly traded).` (EN). The CTA routes to the first such grant's detail screen (AC-5.3.3).

### 5.2 Per-ticker current-price input (Q1 decision)

- **AC-5.2.1 — input surface.** Given the user clicks `Introducir precios`, when the dialog opens, then it renders one row per **distinct ticker** in the user's portfolio (distinct = `UPPER(TRIM(grants.ticker))` with non-null / non-empty). Each row exposes: ticker symbol (read-only), currency suffix (derived from the grant's native currency; USD for the current persona), a numeric input (up to 4 decimal places, > 0), and an optional `last_updated_at` display (updated on save).
- **AC-5.2.2 — persistence scope.** Given the user saves a non-blank price for ticker `T`, when the save completes, then the value is stored under the current user's scope (not shared across users). The store is keyed by `(user_id, ticker_normalized)`. Entering a price for `ACME` also drives any other grant whose ticker is `ACME` (case-insensitive, trimmed).
- **AC-5.2.3 — bands.** Given ticker `T` has a current price `P_T` and the user owns grant `G` on ticker `T` with `N_G` vested shares and basis `B_G` (see AC-5.4 for basis resolution), when the tile renders, then the grant's per-grant paper gain in EUR at spread `s ∈ {0, 1.5 %, 3 %}` is computed as:
  ```
  gain_native = (P_T − B_G) × N_G
  gain_eur_s  = gain_native × ecb_mid × (1 − s)
  ```
  where `ecb_mid` is the `FxLookupResult` rate on today's date per §4. The tile renders the sum across all grants with resolved basis at each of the three spreads, rendered as a range (low / central / high) per UX Pattern C (§7.4 of the spec).
- **AC-5.2.4 — validation.** Given the user enters `P_T ≤ 0`, when they submit, then inline rejection: `Introduce un precio positivo.` / `Enter a positive price.`
- **AC-5.2.5 — optional fields.** Any ticker row may be saved blank (cleared); the tile then treats all grants on that ticker as "price unknown" and surfaces them via the partial-data banner (§5.5).
- **AC-5.2.6 — audit.** No `audit_log` row is written for current-price edits. Current prices are **user workspace data**, not regulated inputs; the rationale is parity with Slice-1/2 treatment of grant `notes` edits. (ADR-017 may decide to log this if the security-engineer review raises a flag; the requirement here is that no payload-schema violation occurs if it is logged.)

### 5.3 Per-grant override and unlisted fallback

- **AC-5.3.1 — per-grant override on listed grants.** Given the user is on grant-detail for a listed grant (ticker non-null), when they view the grant's summary region, then a new field `Precio actual (override)` is visible. The field pre-fills blank. If populated, the override takes precedence over the per-ticker price for that specific grant in all paper-gains calculations.
- **AC-5.3.2 — override visual state.** Given a per-grant current-price override is set, when the dashboard tile renders, then the tile's tooltip on hover over that grant's contribution notes "Este grant usa un precio específico introducido por ti." (ES) / "This grant uses a user-entered price override." (EN). No separate row on the tile — the override is absorbed into the same aggregate.
- **AC-5.3.3 — per-grant fallback on unlisted grants.** Given a grant has no ticker (`grants.ticker IS NULL` or empty, typically unlisted-company ESPP/NSO), when the user opens grant-detail, then the `Precio actual` field is **required** to include that grant in the dashboard paper-gains tile. The field's label is `Precio actual por acción (tu empleador aún no cotiza)` (ES) / `Current price per share (your employer is not yet publicly traded)` (EN).
- **AC-5.3.4 — validation.** Same numeric rule as AC-5.2.4. Saving a blank value clears the override.
- **AC-5.3.5 — persistence.** Per-grant current-price lives on the `grants` row (ADR-017 authoritative on column naming). Clearing the grant does not require clearing the override; editing the ticker does. (Changing the ticker on a grant that had a per-grant override: the override persists; the override remains attached to the grant, not to the old ticker.)

### 5.4 Basis resolution for paper-gains

- **AC-5.4.1 — RSU basis.** Given a vested RSU share (from a grant whose `instrument = "rsu"` and — for double-trigger — whose `liquidity_event_date IS NOT NULL` or whose row is not dashed per Slice-1 AC-6.1.4), when the tile computes the grant's paper gain, then the basis per share is the **`vesting_events.fmv_at_vest`** of the vesting event that produced the share, in the event's `fmv_currency`. Vesting events with `fmv_at_vest IS NULL` contribute to the partial-data banner per §5.5 and **not** to the EUR gain sum.
- **AC-5.4.2 — ESPP basis.** Given shares from an ESPP grant with recorded `espp_purchases` (Slice 2), when the tile computes the grant's paper gain, then the basis per share is the `espp_purchases.fmv_at_purchase` of the purchase row, in the purchase's `currency`. ESPP shares with no recorded purchase (e.g., captured-but-not-yet-purchased) do **not** contribute to the tile and do **not** appear in the partial-data banner (they appear nowhere).
- **AC-5.4.3 — NSO basis deferral.** Given shares from an NSO/ISO grant (`iso_mapped_to_nso` or `nso`), when the tile computes, then **those grants are excluded from Slice-3's paper-gains sum**. An NSO's cost basis on sale depends on FMV-at-exercise (captured in `nso_exercises` — **Slice 5**). A tester-facing note in the tile's legend reads `Los NSO/ISO se incluirán al activar el flujo de ejercicio (próximamente)` (ES) / `NSOs/ISOs will be included when the exercise flow ships (coming soon)` (EN). Tester do-not-flag — this is the expected Slice-3 state.
- **AC-5.4.4 — double-trigger exclusion.** Given a double-trigger RSU with `liquidity_event_date IS NULL`, when the tile computes, then those shares are excluded from the paper-gains sum (zero taxable-realized shares per Slice-1 AC-4.3.4). They do not appear in the partial-data banner.

### 5.5 Partial-data UX (Q3 decision)

- **AC-5.5.1 — trigger.** Given the user has entered current prices for at least one ticker AND at least one eligible grant (RSU with vested shares, ESPP with recorded purchases) has at least one vesting-event / purchase with basis = `NULL`, when the tile renders, then the tile shows a **partial result** (the EUR gain sum across grants with complete basis) AND a banner reading: `Cálculo parcial. Faltan FMV de vesting en: {grant_name_list}.` (ES) / `Partial computation. Missing vest FMVs on: {grant_name_list}.` (EN). The grant-name list is a comma-separated series of at most 3 employer+instrument labels; if more than 3 grants are affected, the label appends `y otros {N}` / `and {N} others`.
- **AC-5.5.2 — grant-name click-through.** Given the banner is visible and the user clicks a grant name in the list, when the click resolves, then the browser navigates to the "Precios de vesting" anchor on that grant's detail screen (`/app/grants/:id#precios-de-vesting`).
- **AC-5.5.3 — no basis-proxy fallback.** The tile does **not** substitute the grant's strike price (NSO) or grant-date price (RSU) as a basis proxy when `fmv_at_vest IS NULL`. The affected grants are simply excluded from the sum and surfaced via the banner. (Rationale: a basis proxy would silently under- or over-state the gain; the user must complete the data or explicitly accept exclusion.)
- **AC-5.5.4 — FX-unavailable state.** Given `FxLookupResult::Unavailable` per AC-4.2.3, when the tile renders, then it shows: `No se pudieron obtener tipos de cambio recientes del BCE. Las ganancias latentes no se muestran hasta que se restaure la fuente.` (ES) / `ECB FX rates are unavailable; paper gains are not shown until the source recovers.` (EN). No number is shown; no rule-set chip is shown.
- **AC-5.5.5 — all-grants-excluded state.** Given every eligible grant has at least one missing basis OR no grant is eligible at all, when the tile renders, then the tile shows the banner as the primary surface with no EUR number. This is distinct from the empty state (AC-5.1.2) — the empty state appears before the user has entered any current price; the all-excluded state appears after the user has entered prices but no grant is computable.

### 5.6 Performance (§7.8 extension)

- **AC-5.6.1 — P95 latency.** The dashboard paper-gains tile's server-rendered fragment (or equivalent client-rendered hydrate) completes within **500 ms P95** measured on a laptop-scale Postgres warmed from the Slice-2 dashboard demo state. The branch that renders the partial-data banner must not exceed this budget.

## 6. Modelo 720 threshold alert (new in Slice 3)

Reference: US-007 spec ACs 1, 2, 4. Profile panel host: the Slice-2 Modelo 720 inputs panel (AC-6.1.1 Slice 2). Dashboard host: a banner slot at the top of the dashboard when triggered.

**Scope reminder.** Slice 3 delivers US-007 ACs **1, 2, 4** — the passive threshold banner. **AC 3 (worksheet PDF export) is Slice 6.** The alert reads the currently-open `modelo_720_user_inputs` rows (bank accounts, real estate) captured in Slice 2 + derives the securities category from `grants` × `vesting_events.fmv_at_vest` × `fx_rates` at today's mid.

### 6.1 Derivation

- **AC-6.1.1 — securities derivation.** Given the user has ≥1 grant with at least one vested share carrying `fmv_at_vest IS NOT NULL`, when the M720 securities total is computed, then it equals the sum across all grants of `(sum of vested shares × fmv_at_vest)` converted to EUR via `ecb_mid` on today's date (per §4). ESPP purchased shares use `espp_purchases.fmv_at_purchase × shares_purchased` converted identically. Shares with `fmv_at_vest IS NULL` (pre-IPO gaps) are excluded from the securities total; a footnote in the M720 panel notes `Algunos grants no aportan al cálculo por FMV sin registrar (N grants).` (ES) / `N grants are excluded from this total due to unrecorded FMV.` (EN).
- **AC-6.1.2 — category totals.** The three category totals are: `bank_accounts_total_eur` (Slice-2 `modelo_720_user_inputs` currently-open row for `bank_accounts`), `real_estate_total_eur` (Slice-2 currently-open row for `real_estate`), `securities_total_eur` (derived per AC-6.1.1). If no Slice-2 row exists for a category (user never saved), that category's total is `NULL` and the alert treats it as `0` for threshold comparison only (not for display).
- **AC-6.1.3 — aggregate total.** The aggregate = sum of the three category totals (treating `NULL` as `0` for the sum).

### 6.2 Threshold and banner

- **AC-6.2.1 — per-category threshold (US-007 AC 2).** Given any single category total ≥ **€50 000**, when the Profile or Dashboard loads, then the threshold banner renders. Banner copy (ES): `Modelo 720 — una categoría supera €50.000: {category_label} (aprox. €{amount}). Es posible que tengas obligación de presentar Modelo 720.` Banner copy (EN): `Modelo 720 — one category exceeds €50,000: {category_label} (approx. €{amount}). You may have a Modelo 720 filing obligation.` `category_label` is one of `Cuentas bancarias extranjeras` / `Bienes inmuebles extranjeros` / `Valores extranjeros`.
- **AC-6.2.2 — aggregate threshold (defence-in-depth).** Given no single category exceeds €50 000 but the aggregate exceeds €50 000, when the page loads, then a softer variant of the banner renders: `Modelo 720 — el total de activos declarados se aproxima al umbral (€{aggregate}). Revisa categorías con tu gestor.` (ES) / `Modelo 720 — your declared foreign assets approach the threshold (€{aggregate}). Review categories with your gestor.` (EN). No filing-obligation claim is made in this variant (the per-category threshold is the regulatory trigger; the aggregate is informational).
- **AC-6.2.3 — sub-threshold state (US-007 AC 1).** Given every category total is below €50 000 AND the aggregate is below €50 000, when the page loads, then no banner renders. The Profile M720 panel shows each category's value without adornment.
- **AC-6.2.4 — FX sensitivity footnote (US-007 AC 4).** Given a category's total is within **±5 %** of €50 000 (i.e., in the range €47 500..€52 500), when the banner (or sub-threshold panel) renders, then a footnote appears: `A tipo ECB actual: €{central}; con ±5 % de variación FX: €{low}–€{high}.` (ES) / `At current ECB rate: €{central}; with ±5 % FX variation: €{low}–€{high}.` (EN). The low/high are computed by applying the 0 % / 3 % spread bands from §5.2 to the USD-denominated portion of the category (typically all of the securities category; none of the user-entered EUR categories).
- **AC-6.2.5 — host surfaces.** The banner renders on: (a) the **top of the dashboard**, above the paper-gains tile; (b) the **top of the Profile page**, above the Slice-2 M720 inputs panel. It does **not** render on grant-detail, on the bulk-fill modal, or on the signup wizard.
- **AC-6.2.6 — banner dismissibility.** The banner is dismissible **for the session only**. On the next session (or a hard reload), it re-renders while the threshold condition holds. Dismissal does not write to `audit_log` or alter any database state (aligns with Slice-2's banner-free posture — this is the first banner to ship).

### 6.3 Worksheet-export non-goal

- **AC-6.3.1 — no PDF export button on the banner.** US-007 AC 3 (worksheet PDF with category breakdown + per-asset detail) is Slice 6. The Slice-3 banner does **not** expose a "Descargar worksheet" CTA. The banner may link to a read-only explainer on why the threshold matters (a short markdown page; optional — not required for Slice 3 acceptance).
- **AC-6.3.2 — no e-filing.** Orbit never files Modelo 720. The banner copy explicitly says "Es posible que tengas obligación de presentar" / "You may have a filing obligation" — never "Presenta" / "File".

## 7. Rule-set chip in footer (new in Slice 3)

Reference: UX §8 layer 2 (chip + "Ver trazabilidad") — Slice 3 ships the chip; full tax-rule-set stamping starts in Slice 4.

### 7.1 Rendering

- **AC-7.1.1 — when rendered.** The rule-set chip renders in the footer on every page that displays at least one **FX-dependent number**. In Slice 3 those pages are: dashboard (when paper-gains tile shows an EUR number OR when the M720 banner shows a derived-securities EUR number), Profile (when the M720 securities row or the threshold banner shows an EUR number), grant-detail (when at least one vesting-event row carries `fmv_at_vest` AND the grant's employer has a ticker with a current price entered — i.e., when the "Precios de vesting" section contributes to the dashboard tile).
- **AC-7.1.2 — when NOT rendered.** The chip does **not** render on: signup / residency / first-grant wizard, Art. 7.p trips screen, Sessions UI, grant-detail for grants with no FMV data yet, or any page where no EUR number is derived from FX. On those pages, the Slice-1/-2 copy-only footer applies.
- **AC-7.1.3 — copy.** Chip text: `Reglas: ECB · {fx_date} · motor v{version}`. `{fx_date}` is the `ecb_publication_date` of the `fx_rates` row used for the surface's computation in G-14 format (short form: `17 abr 2026` / `Apr 17, 2026`). `{version}` is the Orbit engine semver (from the binary's compile-time version — ADR-017 pins the exact source; this doc requires that the value is stable across a render cycle on one page). Example: `Reglas: ECB · 17 abr 2026 · motor v0.3.0`.
- **AC-7.1.4 — staleness overlay.** When the FX lookup returns `FxLookupResult::Stale`, the chip extends per AC-4.5.2/4.5.3 with `· stale {N} día(s)` (ES) / `· stale {N} day(s)` (EN). The base chip format is preserved.
- **AC-7.1.5 — click behaviour (stub).** Clicking the chip navigates to `/app/help/chip-explainer` — a **stub explainer page** that reads: `Este chip muestra qué tipo de cambio ECB y qué versión del motor de Orbit han producido las cifras de esta página. Cuando llegue el motor fiscal completo (próximamente), el chip también mostrará la versión del conjunto de reglas fiscales aplicado.` (ES) / `This chip shows which ECB FX rate and which Orbit engine version produced the figures on this page. When the full tax engine arrives (coming soon), the chip will also show the applied tax rule-set version.` (EN). Full content (with tax-rule-set + AEAT guidance date) ships in Slice 4.
- **AC-7.1.6 — no tax rule-set yet.** The chip in Slice 3 never displays a tax rule-set version (e.g., `es-2026.1.0`). Attempting to introduce one is a defect — tax rule-set stamping lands in Slice 4 when the calculator itself lands.

## 8. Per-vest FMV capture + editable past vesting events (new in Slice 3 — v1.4 core)

Reference: `grant-detail.html` (new "Precios de vesting" section host). ADR-017 authors the DDL (`vesting_events.fmv_at_vest`, `fmv_currency`, `is_user_override`, `overridden_at`) and the override-preservation rules on `orbit_core::vesting::derive_vesting_events`. This section pins the **requirements**.

### 8.1 Section entry point + layout

- **AC-8.1.1 — section presence.** Given the user opens grant-detail for any grant with `instrument ∈ {rsu, nso, iso_mapped_to_nso, espp}`, when the page loads, then a new section `Precios de vesting` (ES) / `Vesting prices` (EN) renders **between** the Summary region and the Vesting-timeline region (Slice-1 AC-6.1.1). The section anchor is `#precios-de-vesting` (AC-5.5.2).
- **AC-8.1.2 — section empty state.** Given the grant has no `vesting_events` rows (edge case — typically only if the grant was just created and the derivation has not yet run), when the page loads, then the section renders `No hay vestings calculados todavía.` (ES) / `No vesting events computed yet.` (EN) and no table.
- **AC-8.1.3 — table rows.** Given `vesting_events.grant_id = {id}` returns ≥1 row, when the page loads, then the section renders a table with one row per event, sorted ascending by `vest_date`. Columns: `Fecha de vesting` (vest_date; G-14 format), `Acciones` (shares_vested_this_event; integer with locale thousands-separator), `FMV por acción` (fmv_at_vest + fmv_currency suffix; blank if `NULL`), `Estado` (derived: `Vestedo` / `Vested` for `vest_date ≤ today`; `Futuro` / `Future` for `vest_date > today`; and a chip `Ajustado manualmente` / `Manually adjusted` if `is_user_override = true`). A fifth column holds per-row action buttons (edit / save / cancel during edit).
- **AC-8.1.4 — bulk-fill CTA.** Given the grant's `instrument` is `rsu` or `espp` (ESPP has its own basis model via `espp_purchases`, but bulk-fill is still offered for pre-IPO FMV on future windows — see AC-8.6) AND at least one row has `fmv_at_vest IS NULL`, when the section renders, then a secondary CTA `Aplicar FMV a todos` (ES) / `Apply FMV to all` (EN) is visible at the top of the section. CTA opens the bulk-fill modal per §8.6.

### 8.2 Past-row inline edit (vest_date, shares_vested_this_event, fmv_at_vest)

- **AC-8.2.1 — edit affordance.** Given a row whose `vest_date ≤ today` ("past row"), when the user clicks the row's edit button, then the three editable cells (`vest_date`, `shares_vested_this_event`, `fmv_at_vest`) become focusable inputs. The `fmv_currency` is selectable only when `fmv_at_vest` is non-blank; it defaults to the grant's `strike_currency` if set, else `USD` (locked default).
- **AC-8.2.2 — vest_date validation.** Given the user changes `vest_date`, when they submit the cell edit, then the new value must satisfy: (a) valid date, (b) `vest_date ≤ today + 1 day` (future-date hint warning per Slice-1 AC-4.2.9 pattern for `today + 1..today + 365`; hard reject for `> today + 365 days`), (c) inside the grant's vesting-window bounds — defined as `grant_date ≤ vest_date ≤ grant_date + vesting_total_months`. Out-of-bounds rejects with inline copy: `La fecha de vesting debe estar dentro del período de vesting del grant.` / `The vesting date must fall within the grant's vesting window.`
- **AC-8.2.3 — shares validation.** Given the user changes `shares_vested_this_event`, when they submit, then the value must be a positive integer and must be ≤ the grant's `share_count`. Over-cap rejects with: `La cantidad de acciones no puede superar el total del grant.` / `Share count cannot exceed the grant total.` A row may carry 0 only via delete (implicit — a row with 0 shares is not valid; the user must delete the row, which is an AC-8.4.3 concern).
- **AC-8.2.4 — FMV validation.** Given the user enters `fmv_at_vest`, when they submit, then the value must be `> 0` and ≤ 4 decimal places. Blank is allowed (clears the FMV). `fmv_currency` must be one of `{USD, EUR, GBP}` (same set as Slice-2 AC-4.2.6).
- **AC-8.2.5 — persist + override mark.** Given a valid edit, when the save completes, then the row's `is_user_override = true`, `overridden_at = now()`, and the three fields (or the subset changed) are persisted. A `vesting_event.override` audit row is written per G-32 with `fields_changed` enumerating which of `"vest_date" | "shares" | "fmv"` actually changed (i.e., if the user edited only FMV, only `["fmv"]` is listed).
- **AC-8.2.6 — keyboard navigation.** Per G-33, Tab advances cells; Enter submits the whole-row edit; Escape cancels and restores prior values.

### 8.3 Future-row inline edit (FMV only)

- **AC-8.3.1 — allowed fields.** Given a row whose `vest_date > today` ("future row"), when the user clicks edit, then **only** `fmv_at_vest` and `fmv_currency` are editable. `vest_date` and `shares_vested_this_event` remain read-only (greyed inputs with a tooltip: `Solo los vestings pasados pueden cambiar fecha o acciones.` / `Only past vests can edit date or shares.`).
- **AC-8.3.2 — persist + override mark.** Given a valid FMV edit on a future row, when saved, then the row's `is_user_override = true` and `overridden_at = now()`. The `fields_changed` payload contains only `["fmv"]`.
- **AC-8.3.3 — FMV validation.** Same as AC-8.2.4.
- **AC-8.3.4 — rationale (non-test).** Future-row FMV is the pre-IPO 409A use case: the user enters a forward-looking FMV estimate so that the dashboard paper-gains tile and Slice-4 scenarios have a value to compute against. Date + shares are algorithm outputs for future events — the user should not drift them ahead of the derivation.

### 8.4 `is_user_override` semantics + grant re-derivation preserves overrides

- **AC-8.4.1 — flag lifecycle.** Given a row's `is_user_override = false`, when any user action (per AC-8.2.5 or AC-8.3.2) modifies `vest_date`, `shares_vested_this_event`, or `fmv_at_vest`, then the flag flips to `true` and `overridden_at = now()`. The flag **never flips back to `false` automatically** — only AC-8.7 `clearOverride: true` reverts it.
- **AC-8.4.2 — grant re-derivation preserves overrides.** Given the user edits the **parent grant**'s vesting fields (e.g., changes `vesting_start`, `vesting_total_months`, `cliff_months`, `vesting_cadence`) via the grant-edit form, when the server re-derives the vesting events, then rows with `is_user_override = true` are **preserved in place** — their `vest_date`, `shares_vested_this_event`, `fmv_at_vest`, `fmv_currency` are left untouched. Only rows with `is_user_override = false` are regenerated from the derivation algorithm (Slice-1 AC-4.3.1..5).
- **AC-8.4.3 — row deletion semantics.** Given the user wants to remove an overridden row (e.g., they overrode a row in error), the **deletion affordance is `clearOverride`** (AC-8.7), not a hard delete of the row. Hard deletion of `vesting_events` rows is not exposed to the user in Slice 3; the derivation algorithm owns row existence.
- **AC-8.4.4 — re-derivation of non-overridden rows on grant-edit.** Given a grant with N `vesting_events`, M of them overridden (`is_user_override = true`), when the grant is edited, then the algorithm produces a new candidate event list; non-overridden events are discarded and replaced by the candidate output; overridden events are preserved. If the new candidate list has fewer events than previously non-overridden rows, the excess non-overridden rows are deleted. If more, the excess are inserted.

### 8.5 Cumulative invariant relaxation on overridden grants

- **AC-8.5.1 — baseline invariant (Slice 1).** In Slice 1 (no overrides), `SUM(vesting_events.shares_vested_this_event WHERE grant_id = G) = grants.share_count`. This invariant is a property-based test input in Slice-1 AC-4.3.5.
- **AC-8.5.2 — relaxation on overrides.** Given any `vesting_events` row for grant `G` has `is_user_override = true`, the strict equality is **relaxed**: `SUM(vesting_events.shares_vested_this_event WHERE grant_id = G)` MAY differ from `grants.share_count`. The user's edits win; the algorithm does not redistribute the delta back across non-overridden rows.
- **AC-8.5.3 — UI signal.** Given the invariant is relaxed (i.e., at least one row is overridden AND the sum `≠ share_count`), when the grant-detail page renders, then an inline note appears above the "Precios de vesting" table: `Esta curva incluye ajustes manuales; la suma de acciones por evento puede diferir del total del grant.` (ES) / `This curve includes manual adjustments; the sum of per-event shares may differ from the grant total.` (EN).
- **AC-8.5.4 — dashboard envelope unchanged.** The Slice-1 single-grant cumulative curve and the Slice-2 multi-grant stacked envelope (AC-8.2.8 Slice 2) continue to render from the `vesting_events` rows as-is — the envelope on an overridden grant reflects the overridden shares, not the algorithmic prediction. The envelope does **not** refuse to render; it renders whatever the user's data says. Tester do-not-flag: a small visual discontinuity on an overridden curve is expected.

### 8.6 Bulk-fill "Aplicar FMV a todos" (Q4 decision)

- **AC-8.6.1 — modal entry.** Given the bulk-fill CTA is clicked per AC-8.1.4, when the modal opens, then it renders: a numeric input `FMV por acción` (required, > 0, 4 dp), a currency select (`USD | EUR | GBP`, default per the locked default — grant's `strike_currency` else `USD`), and a confirmation copy block (AC-8.6.3). The modal does not show which specific rows will be filled until the user enters a numeric value.
- **AC-8.6.2 — target-set computation.** Given the user enters an FMV and currency, when the modal updates (client-side), then it computes two counts: `X` = rows where `fmv_at_vest IS NULL` (these will be filled) and `Y` = rows where `fmv_at_vest IS NOT NULL` (these will be **skipped** — even if they were filled by an earlier bulk-fill, even if `is_user_override = false`).
- **AC-8.6.3 — confirmation dialog copy.** Given `X > 0`, the confirmation copy reads: `Se rellenarán {X} vestings sin FMV; los {Y} que ya tienen un valor manual no se tocan.` (ES) / `{X} vest rows without FMV will be filled; the {Y} rows that already carry a manual value are skipped.` (EN). If `Y = 0`, the skip phrase is elided. If `X = 0`, the primary CTA is disabled and the copy reads: `No hay vestings sin FMV para rellenar.` (ES) / `No vest rows lack FMV.` (EN).
- **AC-8.6.4 — commit.** Given the user confirms, when the request completes, then for each of the `X` rows: `fmv_at_vest = <input>`, `fmv_currency = <select>`, `is_user_override = true`, `overridden_at = now()`. One `vesting_event.override` audit row is written per modified row with `fields_changed: ["fmv"]`. The `Y` rows are untouched (no audit rows for them).
- **AC-8.6.5 — no bulk clear.** There is no "bulk clear FMVs" action in Slice 3. Clearing FMV is a per-row operation (AC-8.2.4 / AC-8.3.3 blank save). Rationale: an accidental bulk clear is harder to recover from than an accidental bulk fill; the asymmetry is deliberate.
- **AC-8.6.6 — dashboard ripple.** After a successful bulk-fill, the dashboard paper-gains tile recomputes on next render: grants that were on the partial-data banner (§5.5) drop off the banner for any rows now filled.

### 8.7 `clearOverride: true` semantics

- **AC-8.7.1 — endpoint semantics (requirement).** Given the user clicks a "Revertir a cálculo automático" / "Revert to auto" action on an overridden row, when the request body carries `clearOverride: true` (ADR-017 authoritative on API shape), then the server: (a) reverts `vest_date` and `shares_vested_this_event` to the derivation algorithm's current output for that row; (b) **preserves** `fmv_at_vest` and `fmv_currency` exactly as the user had them; (c) sets `is_user_override = false` only if `fmv_at_vest IS NULL` after the revert (i.e., the user had not also entered an FMV); (d) if the user had entered an FMV, `is_user_override` remains `true` because the FMV itself is a manual edit.
- **AC-8.7.2 — audit.** A `vesting_event.clear_override` audit row is written per G-32 with `cleared_fields: ["vest_date", "shares"]` and `preserved: ["fmv"]` (or `preserved: []` if the row had no FMV).
- **AC-8.7.3 — UI confirmation.** Clicking "Revertir a cálculo automático" opens a confirmation dialog: `Se restaurarán la fecha y las acciones al cálculo automático. El FMV que hayas introducido se mantiene.` (ES) / `Date and shares will revert to the algorithm's output. Any FMV you entered is preserved.` (EN). The dialog is dismissible via Cancel; the revert is not idempotent — clicking it on a non-overridden row is a no-op and writes no audit row.

### 8.8 Grant-edit form: override-exists warning banner

- **AC-8.8.1 — banner presence.** Given the user opens the grant-edit form for a grant with ≥1 `vesting_events` row where `is_user_override = true`, when the form renders, then a warning banner appears at the top of the form: `Este grant tiene {N} vesting(s) ajustado(s) manualmente. Los cambios aquí no alteran esos vestings; se conservan tal y como los editaste.` (ES) / `This grant has {N} manually adjusted vest(s). Edits here do not alter those vests; they are preserved as you edited them.` (EN). `{N}` is the exact count of `is_user_override = true` rows.
- **AC-8.8.2 — banner absence.** Given no overrides exist on the grant, when the form renders, then no banner appears.
- **AC-8.8.3 — no blocking.** The banner is informational only; submission proceeds per Slice-1 AC-6.2.1..3. The AC-8.4.2 preservation rule is what makes the banner's promise true.

### 8.9 `share_count` shrink-below-overrides validation (locked default)

- **AC-8.9.1 — 422 on shrink.** Given a grant with ≥1 overridden `vesting_events` row, when the user submits a grant-edit with a new `share_count` strictly less than `SUM(shares_vested_this_event WHERE is_user_override = true)`, then the server returns **422 Unprocessable Entity** with an error body that the form renders as: `No se puede reducir el total de acciones a {N}: tienes {M} acciones en vestings ajustados manualmente que superan ese total. Revisa los vestings manuales primero.` (ES) / `Share count cannot shrink to {N}: {M} shares on manually adjusted vests exceed that total. Revise manual vests first.` (EN).
- **AC-8.9.2 — no partial save.** The 422 response guarantees no update was applied. The form stays populated with the user's attempted values (AC-10.1 pattern).
- **AC-8.9.3 — alternate repair path.** The error copy directs the user to the "Precios de vesting" section; the user may either delete/revert overridden rows (AC-8.7) or raise the target `share_count`. No server-side auto-repair.

### 8.10 Audit-log payload allowlists (consolidated)

- **AC-8.10.1 — `vesting_event.override`.** Payload keys: `{ grant_id: <uuid>, fields_changed: [<"vest_date" | "shares" | "fmv">, ...] }`. No other keys permitted. `fields_changed` MUST contain at least one element (a no-op "override" with no field changed is not written).
- **AC-8.10.2 — `vesting_event.clear_override`.** Payload keys: `{ grant_id: <uuid>, cleared_fields: [<"vest_date" | "shares">, ...], preserved: [<"fmv">, ...] }`. `cleared_fields` is always `["vest_date", "shares"]` in practice (per AC-8.7.1 both revert together) but the array shape is preserved for forward-compatibility if ADR-017 introduces partial clears in a later slice. `preserved` is either `["fmv"]` or `[]`.
- **AC-8.10.3 — never present in payloads.** The following MUST NOT appear in any `vesting_event.*` audit-log payload: FMV values (any currency), share counts (new or old), vest dates (new or old), the grant's employer name, the grant's ticker, any locale-string copy, any derived EUR gain value. CI lint on the audit-writer enforces the allowlist.

## 9. "Tengo varios grants" link copy

The Slice-2 AC-9.1..5 copy update is **carried forward unchanged in Slice 3**. The link still targets Slice 8 (bulk import). Slice 3 introduces no new copy change and no new destination change.

- **AC-9.1 — no-op in Slice 3.** The first-grant form's "Tengo varios grants" link renders the Slice-2 copy (`... habrá una importación masiva (Carta / Shareworks / ETrade) más adelante ...`). Click still dismisses the form to the dashboard (populated or empty depending on state). Slice 8 flips the destination to the import landing page.
- **AC-9.2 — tester do-not-flag.** The link does not reference FX, FMV, or the paper-gains tile; it remains strictly about bulk import.

## 10. Error and edge states

- **AC-10.1 — network error during submit preserves form state.** On every new form surface (per-ticker current-price dialog, per-grant current-price override, "Precios de vesting" inline edits, bulk-fill modal, grant-edit with overrides present), a server error during submit renders inline banner: `No se pudo guardar. Inténtalo de nuevo.` / `Could not save. Try again.` Form state is preserved client-side; no partial-save occurs.
- **AC-10.2 — session expiry redirect preserving path.** A session that expires mid-edit on any Slice-3 surface triggers a redirect to the login screen with flash `Tu sesión ha caducado.` / `Your session expired.` On re-login the user lands back on the originating path (extends Slice-1 AC-7.2 / Slice-2 AC-10.2). For the "Precios de vesting" inline editor, unsaved cell-level inputs are **not** preserved across the re-login round-trip (acceptable v1 limitation, parity with Slice-2 AC-10.2).
- **AC-10.3 — cross-tenant 404 not 403.** For every new surface a request for an id outside the current user's RLS scope returns 404, not 403. Explicitly covered:
  - A `PUT` on `vesting_events/{id}` where `vesting_events.grant_id → grants.user_id ≠ current_user` returns 404.
  - A `GET` of a per-grant current-price override on someone else's grant returns 404.
  - A bulk-fill `POST` targeting someone else's grant returns 404.
- **AC-10.4 — ECB unavailable during dashboard render.** Given the ECB pipeline has returned `FxLookupResult::Unavailable` (walkback exhausted) and the user loads the dashboard, when the render completes, then the paper-gains tile shows the AC-5.5.4 unavailable state and the M720 threshold banner (if any category previously triggered) continues to render with the **last-known-good** FX date note in the chip-like footnote: `Cálculo con ECB {last_date} (sin actualización reciente).` The dashboard does not 500; partial degradation is the required behaviour.
- **AC-10.5 — concurrent vesting-event edits (optimistic concurrency).** Given the user opens the "Precios de vesting" table in two tabs and edits the same row in both, when the second `PUT` arrives, then the server performs an optimistic-concurrency check via `updated_at` (ADR-017 authoritative on the exact column and header name). The **first save wins**; the second returns 409 with copy: `Este vesting se editó en otra pestaña. Refresca para ver los valores actuales.` (ES) / `This vest was edited in another tab. Refresh to see the current values.` (EN). Rationale for choosing optimistic over last-write-wins: FMV edits are meaningful user authorship; silently overwriting a concurrent edit risks losing a correction the user just made. A 409 keeps the user in control.
- **AC-10.6 — validator + CHECK-constraint shared error envelope.** Validation errors from (a) client Zod/Yup, (b) server validator, (c) Postgres CHECK all surface via the same envelope shape (Slice-2 AC-10.4). New Slice-3 validators (FMV > 0, vest_date-in-window, share_count-shrink-below-overrides) conform.
- **AC-10.7 — bulk-fill partial failure.** Given the bulk-fill commit fails mid-transaction (e.g., a DB disconnection after row N of X), when the request completes, then the transaction rolls back entirely — no partial writes. The UI renders AC-10.1's generic error; the `Y`-count skip-preservation is unaffected because nothing was written.
- **AC-10.8 — stale override marker after clearOverride.** Given a user clears an override via AC-8.7 in Tab A while Tab B still renders the "Ajustado manualmente" chip on that row, when Tab B issues an edit on the same row, then AC-10.5's 409 engages. The user is steered to refresh.

## 11. Mobile / responsive

All Slice-3 surfaces meet the Slice-1 + Slice-2 mobile baseline. Additional Slice-3-specific assertions:

- **AC-11.1 — editable vesting table horizontal scroll.** On ≤640 px the "Precios de vesting" table scrolls horizontally with the **first column (`Fecha de vesting`) sticky**. Edit mode expands the row in-place; the sticky column continues to render the vest date even when the row is under edit.
- **AC-11.2 — bulk-fill modal centered.** On ≤640 px the bulk-fill modal renders as a centered dialog with ≥16 px margin on each side. The confirmation copy wraps without truncation in ES.
- **AC-11.3 — paper-gains tile stacking.** On ≤640 px the paper-gains tile stacks: headline → single EUR range (central + low/high on two lines) → partial-data banner (if triggered) → "Introducir precios" CTA. No horizontal scroll.
- **AC-11.4 — M720 threshold banner.** Banner wraps cleanly on ≤640 px; the `€{amount}` value does not truncate; the dismiss control is ≥44 px tall.
- **AC-11.5 — per-ticker current-price dialog.** Stacks one ticker row per viewport row; the numeric input is full-width; currency suffix remains inline.
- **AC-11.6 — rule-set chip wrapping.** The chip wraps to its own footer row below the disclaimer copy at ≤640 px (G-6 extended). No truncation.
- **AC-11.7 — touch targets ≥44×44.** Inherited from Slice 1/2; every new control (edit button per row, save/cancel, bulk-fill CTA, per-ticker-row input, banner dismiss) complies.

## 12. NFRs that do NOT apply (and why, explicitly)

This section is load-bearing. A tester validating Slice 3 must not mark these as defects.

- **§7.1 Tax-rule versioning — partial activation.** The **footer chip** (§7) appears for the first time in Slice 3, but the **tax rule-set itself** (e.g., `es-2026.1.0`, AEAT guidance date) does not exist yet. The chip displays ECB FX date + engine version only. Full §7.1 activates in Slice 4.
- **§7.4 Ranges-and-sensitivity — partial activation.** The **FX spread bands** (0 % / 1.5 % / 3 %) on the paper-gains tile **are** this slice's ranges. No tax-calc ranges yet — those activate in Slice 4 on IRPF projections. The M720 threshold banner's FX sensitivity footnote (AC-6.2.4) is a partial §7.4 preview.
- **§7.5 Autonomía rate tables.** Still not ingested; foral selection from Slice 1 produces no tax-calc block.
- **§7.6 Market-data vendor.** Still off. Current price is user-entered in Slice 3. Finnhub integration ships in Slice 5. Tester do-not-flag.
- **§7.7 FX source — NOW ON.** ECB pipeline is **live** in Slice 3. The 0 % / 1.5 % / 3 % spread bands render on the paper-gains tile (§5.2). User-overridable FX mid + spread per calculation (ADR-007) remains Slice 4 where tax-calc surfaces consume it.
- **§7.8 Performance targets extended.** Paper-gains tile ≤ 500 ms P95 (AC-5.6.1). Other Slice-3 surfaces (grant-detail with the "Precios de vesting" table, bulk-fill modal open) inherit the Slice-2 baseline.
- **§7.9 Security — pen-test.** Still Slice 9. Slice 3 adds one external egress (ECB); the egress allowlist landing is a Slice-9 concern — the Slice-3 local-dev posture is "egress is unrestricted but documented for Slice 9".

## 13. Demo-acceptance script

The Slice-2 19-step flow is assumed complete; Slice 3's demo picks up from a persisted user who holds a realistic portfolio (≥3 grants, ≥1 recorded ESPP purchase, ≥1 M720 category row) from the Slice-2 demo outputs. The ECB worker is running locally.

1. Open `http://localhost:<port>` and sign in as `test+slice3@<domain>`. The Slice-2 dashboard renders with the stacked-ACME envelope + the ESPP grant tile. **No rule-set chip in the footer yet** — the paper-gains tile has no current prices so no FX surface is live.
2. Open the ECB worker log (developer affordance). Observe the **bootstrap** log line on first startup: `fx.bootstrap_success` with `rows_inserted ≈ 65` and `span_days ≈ 90` (AC-4.3.1). Inspect `audit_log` for the corresponding row (G-32).
3. Wait for 17:00 Europe/Madrid (or fast-forward the worker clock). Observe the scheduled fetch log line + `fx.fetch_success` audit row with `rows_inserted: 1` (AC-4.1.1 + AC-4.1.2).
4. Navigate to the dashboard. The paper-gains tile renders the empty state `Introduce el precio actual de tus tickers para ver las ganancias latentes en EUR.` (AC-5.1.2). Click `Introducir precios`.
5. The per-ticker dialog lists two distinct tickers (`ACME` and the ESPP employer's ticker). Enter `$45.00` for ACME, leave the ESPP ticker blank. Save.
6. Dashboard re-renders. The paper-gains tile now shows a **partial result**: ACME's RSU gains computed in EUR at 0 % / 1.5 % / 3 % spread bands, AND a partial-data banner: `Cálculo parcial. Faltan FMV de vesting en: ACME · RSU.` (AC-5.5.1). The ESPP grant is excluded from the banner because it has no current price; it simply does not appear (AC-5.4.2 + AC-5.1.3 logic branch). **Rule-set chip now visible** in the footer: `Reglas: ECB · {today} · motor v{version}` (AC-7.1.1, AC-7.1.3).
7. Click the `ACME · RSU` link in the partial-data banner. Browser navigates to the grant's `#precios-de-vesting` anchor (AC-5.5.2). The "Precios de vesting" section lists all vest events (past + future) with `FMV por acción` blank for every row.
8. Click edit on the 3rd-oldest past row. Enter `$42.8000 USD` as FMV. Tab through and hit Enter. Row persists with the `Ajustado manualmente` chip (AC-8.1.3). Inspect `audit_log` for a `vesting_event.override` row with `fields_changed: ["fmv"]` and payload containing `{ grant_id, fields_changed }` only (AC-8.10.1).
9. Repeat step 8 for the next two past rows (`$44.1000 USD` and `$45.5000 USD`). Three rows now override-marked. Return to the dashboard — the paper-gains tile's EUR range shifts because basis resolution now covers 3 vested events (AC-5.4.1).
10. Return to grant-detail. Click `Aplicar FMV a todos`. Enter `40.0000` and currency `USD`. The confirmation dialog reports: `Se rellenarán 9 vestings sin FMV; los 3 que ya tienen un valor manual no se tocan.` (AC-8.6.3, with X=9 and Y=3 given a 4-year / monthly schedule minus 3 already-filled). Confirm. 9 rows fill; the 3 earlier overrides stay at their manual values (AC-8.6.4).
11. Return to dashboard. The partial-data banner for ACME is now **clean** — the grant no longer appears (all past rows now carry FMV) (AC-8.6.6). The paper-gains tile now renders the full RSU gains.
12. Return to grant-detail. Edit the 5th past row: change `vest_date` by +2 weeks and `shares_vested_this_event` by −10. Save. Row shows `Ajustado manualmente`; the section renders the **cumulative-invariant-relaxed** note: `Esta curva incluye ajustes manuales; la suma de acciones por evento puede diferir del total del grant.` (AC-8.5.3).
13. Open the grant-edit form (top of grant-detail). The **override-exists warning banner** renders: `Este grant tiene 13 vesting(s) ajustado(s) manualmente. ...` (AC-8.8.1 — N = 12 bulk-filled + 3 early overrides + 1 date-shares override, though the exact count depends on the specific flow; the ACs assert only that the displayed count equals the actual `is_user_override = true` row count).
14. Attempt to shrink `share_count` on the grant-edit form from 30 000 to 100. Submit. Server responds 422 with copy per AC-8.9.1; the form shows the error and stays populated. Cancel out.
15. Navigate to Profile → `Modelo 720 — valores declarados`. The Slice-2 panel renders with the prior-saved bank-account (€25 000) + real-estate (€0) rows. The **securities row now shows a number** (derived per AC-6.1.1): `€{derived_amount}` with the footnote `A tipo ECB actual: ...` (AC-6.2.4). The derivation uses today's ECB FX × the now-filled FMVs × vested shares.
16. Edit the bank-accounts value to `€45 000`. Save (Slice-2 close-and-create semantics unchanged). The aggregate total now exceeds €50 000 — the **M720 threshold banner** renders at the top of the Profile **and** at the top of the dashboard (AC-6.2.2 aggregate variant). Banner copy: `Modelo 720 — el total de activos declarados se aproxima al umbral (€{aggregate}). Revisa categorías con tu gestor.` (AC-6.2.2).
17. Inspect the footer on the Profile page: rule-set chip is present (the M720 securities row produces an FX-dependent number) with today's ECB date. Inspect the footer on the dashboard: same chip. Inspect the footer on the Art. 7.p trips screen: **no chip** (no FX-dependent surface — AC-7.1.2). Inspect the footer on the residency-setup edit screen: **no chip**.
18. Sign out. Sign back in. All Slice-3 state is preserved: per-ticker current prices, per-grant FMV overrides, bulk-fill fills, M720 category values, rule-set chip re-renders with the same date.
19. Run `axe` CI job on the PR's preview URL: zero violations across all new Slice-3 surfaces (G-21 extended).
20. Run the keyboard-only walkthrough of steps 5–14 (current-price dialog, vesting-events inline edit, bulk-fill modal, grant-edit 422 path): every interaction is reachable via Tab / Shift-Tab / Enter / Space / Escape; focus ring visible at every step (G-19 + G-33).
21. Inspect `audit_log` rows for this session:
    - `fx.bootstrap_success` × 1 (step 2).
    - `fx.fetch_success` × N (one per scheduled + any on-demand fetch during the session).
    - `vesting_event.override` × (3 + 9 + 1) = 13 (steps 8, 10, 12); each payload carries only `{ grant_id, fields_changed }`.
    - `modelo_720_inputs.upsert` × 1 (step 16; Slice-2 shape unchanged).
    - No FMV values, no share counts, no EUR amounts, no ticker symbols, no employer names, no raw XML bodies anywhere in any payload (AC-8.10.3 + G-29).
22. Check product-analytics event payloads (if opted in): G-26 extended lint holds — no FMV values, no current prices, no EUR amounts, no M720 totals, no override counts, no ECB response bodies in any payload.

If all 22 steps pass, Slice 3 is accepted.

## 14. Out-of-scope reminders (tester do-not-flag list)

The following are **correct** behaviours in Slice 3 and must not be written up as defects. Each item is anchored to the slice where it actually ships.

- **No IRPF projection** on any Slice-3 surface. No "you will owe X" number. Tax math ships in **Slice 4**.
- **No Art. 7.p eligibility evaluation.** The Slice-2 trip list + checklist still show only capture; no €60 100 cap math, no pro-rata, no overlap rejection. Calculation ships in **Slice 4**.
- **No Modelo 720 worksheet PDF export.** The threshold banner (US-007 ACs 1/2/4) lands in Slice 3; the worksheet PDF (US-007 AC 3) ships in **Slice 6**.
- **No scenario-modeler CTA beyond the "próximamente" sidebar stub.** Scenario modeler ships in **Slice 4**.
- **No sell-now calculator.** Paper-gains tile is a **display-only** EUR aggregator; it is **not** the sell-now calculator. Sell-now (US-013), Finnhub integration, net-EUR-landing bands, passive Modelo 720/721 banner on a sell screen — all ship in **Slice 5**.
- **No NSO exercise-FMV capture.** `nso_exercises` table, NSO bargain-element math, and NSO paper-gains contribution ship in **Slice 5**. Slice-3 paper-gains tile excludes NSO/ISO grants (AC-5.4.3); this is not a defect.
- **No market-data vendor.** Current prices are user-entered in Slice 3. Finnhub / Twelve Data wire up in **Slice 5** (free/dev tier) and **Slice 9** (commercial).
- **No user-overridable FX mid or spread** as a field on the dashboard. The 0 % / 1.5 % / 3 % spread bands render at 1.5 % (central) by default and at 0 % / 3 % as the low/high range (§5.2). Full user override ships in **Slice 4** on tax-calc surfaces and **Slice 5** on sell-now.
- **No `sessions.country_iso2` GeoIP population** (Q2 2026-04-21 decision). The Slice-2 column remains `NULL` through Slice 8; the Sessions UI's `ubicación desconocida` / `unknown location` branch is the default state through Slice 8. ADR-016 §9.2's "Slice 3 populates" note is superseded. Tester do-not-flag.
- **No override-badge on the cumulative vesting timeline** (Slice-1 AC-6.1.2 Gantt + cumulative-curve toggle). Overridden rows surface in the "Precios de vesting" table (AC-8.1.3) but do **not** yet carry a visual badge on the timeline curve or Gantt view. Adding an override-badge chip to the timeline view is a **Slice-4 polish concern**. Tester do-not-flag.
- **No bulk clear of FMVs.** AC-8.6.5 is deliberate; per-row clear is the only path.
- **No tax rule-set version in the footer chip.** The chip shows ECB date + engine version only in Slice 3 (AC-7.1.6). Tax rule-set stamping ships in **Slice 4**.
- **No "recompute under current rules" action anywhere.** Ships dormant in **Slice 4**.
- **No PDF / CSV export of anything.** Includes Modelo 720 worksheet, paper-gains tile, vesting-events table. All exports ship in **Slice 6**.
- **No bulk import (CSV / PDF).** "Tengo varios grants" link still dismisses to the dashboard per AC-9.1. Bulk import ships in **Slice 8**.
- **2FA still not offered.** Optional TOTP ships in **Slice 7**.
- **"Export my data" / "Delete my account"** still route to the "próximamente" page. Full DSR self-service ships in **Slice 7**.
- **No legal surface** (privacy policy, sub-processor list, DPA) published. Ships in **Slice 9**.
- **No pen-test gate.** Ships in **Slice 9**.
- **No paid tier anywhere.** Every account gets every Slice-3 surface. No `[paid]` badges, no blurred previews. (v1.2 PoC posture — permanent.)
