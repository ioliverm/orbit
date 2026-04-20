# Orbit v1 — Slice 2 acceptance criteria

| Field       | Value                                                      |
|-------------|------------------------------------------------------------|
| Version     | 1.0                                                        |
| Date        | 2026-04-20                                                 |
| Owner       | requirements-analyst (Ivan Oliver)                         |
| Slice       | Slice 2 — "Portfolio completeness (hand-entry)" (see `v1-slice-plan.md` v1.3) |
| Boundary    | Multi-grant dashboard + stacked refresh view · ESPP purchase capture · Art. 7.p trip entry (with inline eligibility checklist) · Modelo 720 category inputs on Profile (time-series) · Session / device list UI in Account. **No CSV import. No ETrade PDF import. No tax numbers. No FX / EUR conversion. No Modelo 720 threshold alert. No scenarios. No sell-now. No billing, ever.** |
| Related     | Spec US-003 AC #4 (stacked refresh), US-005 (Art. 7.p eligibility + checklist — form side only), US-007 (Modelo 720 inputs — threshold alert is Slice 3), US-008 (ESPP basis), US-011 (data-minimization); UX screens `dashboard.html` (stacked-grant multi-tile target), `dashboard-slice-1.html` (starting baseline), `session-management.html`, `grant-detail.html` (record-ESPP-purchase CTA + per-grant purchases list), `residency-setup.html` (Profile shell for M720 inputs). ADR-005 entity outline; ADR-016 (Slice-2 technical design / DDL + state machines — authored in parallel by solution-architect). |
| v1.3 notes  | Product-owner decisions 2026-04-20: (Q1) same depth as Slice 1 — full AC granularity on every surface. (Q2) ESPP purchase capture has six mandatory fields + two optional (`fmv_at_offering`, `employer_discount_percent`) and lives under a dedicated `/app/grants/:id/espp-purchases/new` route with a "Record purchase" CTA on grant detail. (Q3) Art. 7.p trip form carries the full eligibility checklist (five yes/no criteria from US-005) captured alongside trip facts; no calculation in Slice 2. (Q4) Modelo 720 category inputs are time-series with close-and-create semantics (same pattern as `residency_periods` in Slice 1). CSV/PDF bulk import deferred to Slice 8 (v1.3). |

This document is implementation-ready. Every AC below is testable as-written. Where a tester needs a specific screen state, the UX reference HTML is cited by filename. ADR-016 authors the DDL, API shapes, and state-machine details that this document intentionally does not duplicate.

## 1. In-scope stories

| Story | In Slice 2? | Notes |
|-------|-------------|-------|
| **US-001 — Create and manage grants manually** | Already in Slice 1 | Multi-grant dashboard rendering is a Slice-2 refresh; grant-CRUD logic is unchanged. |
| **US-003 — Visualise vesting incl. double-trigger** | AC #4 lands here | Stacked refresh-grant cumulative view per employer on the dashboard (per-employer merge, per-instrument drill-down). AC #1, #2 already in Slice 1; AC #3 is scenario-mode (Slice 4). |
| **US-005 — Art. 7.p partial-year exemption** | Form side + inline eligibility checklist only | ACs that describe **calculation** (pro-rata, €60,100 cap application, 0-day omission, cap exposure, overlap-or-domestic rejection) are captured as data but **not evaluated** in Slice 2. Calculation ships in Slice 4 with the tax engine. |
| **US-007 — Modelo 720 threshold alert** | Inputs only | Bank-account + real-estate category totals captured as a time-series on Profile. **Threshold alert (AC #1/#2/#4) ships in Slice 3** with the FX pipeline; worksheet PDF (AC #3) ships in Slice 6. |
| **US-008 — ESPP Spanish-tax treatment** | Purchase capture only | `espp_purchases` table backs the lookup US-008 describes; **no tax treatment math** (discount-as-rendimiento-del-trabajo, cap-gain routing at sale) in Slice 2 — that is Slice 4/5. Slice 2 only captures the inputs the later calculator will consume. |
| **US-011 — GDPR DSR self-service** | Partial (data-minimization + new audit-log entries) | Full DSR self-service ships in Slice 7. Slice 2 extends the data-minimization posture to the three new surfaces (ESPP, trips, M720) and to the sessions UI. |
| **US-002 — CSV import** | No | Slice 8 (deferred from Slice 2 per v1.3 plan decision 2026-04-20). |
| **US-004, US-006 AC #3, US-009, US-010, US-012, US-013** | No | Later slices. |

## 2. Persona & demo context

- Primary tester persona: **María, pre-IPO**, see spec §2.1. By Slice 2 María has completed the Slice-1 wizard once already; she signs in with existing grant data and records additions on top.
- Device: 14" laptop (1440×900), Chrome + Safari. Mobile must render but is not the primary acceptance surface; mobile-specific assertions are in §7.
- Locale acceptance: **ES primary**, EN fallback. Every user-visible string passes through the i18n layer.
- Environment: local-only per ADR-015 §0a + v1.1 (`http://localhost:<port>`). No cloud URL is relevant until Slice 9.

## 3. Global ACs (apply to every screen this slice ships)

Slice 2 inherits **all** Slice-1 global ACs (G-1 through G-32) without re-litigation. The deltas below extend, tighten, or add new ACs; they do not replace Slice-1 wording.

### 3.1 Non-advice disclaimer — footer

- **G-1..G-7 (inherited).** Footer strip renders on every new Slice-2 page (grant detail with ESPP block, ESPP purchase form, Art. 7.p trip list and form, Modelo 720 inputs panel, Sessions UI). Copy, height, tab-order behaviour unchanged from Slice 1.
- **G-5 (re-confirmed).** No rule-set chip in the footer in Slice 2. The chip ships in Slice 3 on the first FX-dependent surface (paper-gains EUR tile), not before. Slice 2 adds no calculation outputs, so the footer remains copy-only.

### 3.2 Non-advice disclaimer — first-login modal

- **G-8..G-10 (inherited from Slice 1, not re-tested here).** Disclaimer gating is proven in Slice 1; Slice 2 does not add a re-acceptance trigger. Re-login during Slice 2 must not re-display the modal.

### 3.3 i18n

- **G-11 (inherited).** Every Slice-2 string ships in `es-ES` and `en`. CI lint rejects single-locale PRs.
- **G-12 (extended).** Spanish tax terms remain in Spanish even in EN locale: the Slice-1 set (`IRPF`, `Modelo 720`, `Art. 7.p`, `Beckham`, `territorio común`, `foral`, `autonomía`) plus no new tokens in Slice 2. `rendimiento del trabajo` first appears in Slice 4 copy (tax engine); it must not leak into any Slice-2 surface.
- **G-13 (inherited).** Locale-aware number formatting. Share counts are integers with thousands-separators; FMV and purchase-price inputs on the ESPP form accept up to 4 decimal places and render with the locale's decimal separator on display. Native currency suffix always explicit (`$30.2400 USD`).
- **G-14 (inherited).** ISO 8601 in storage; user-locale long-form on display. Trip `from_date`/`to_date` and ESPP `offering_date`/`purchase_date` both render per G-14.
- **G-15 (inherited).** ES-first label testing. Art. 7.p eligibility checklist criteria copy is ~3× longer in ES than EN; test at 14″ desktop and at the 640 px mobile breakpoint for no truncation.

### 3.4 Accessibility (WCAG 2.1 AA / 2.2 AA)

- **G-16..G-25 (inherited).** Every Slice-2 page passes `axe` smoke in CI and a manual keyboard walkthrough.
- **G-21 (extended).** CI `axe` smoke runs on: ESPP purchase form, ESPP purchases list on grant detail, Art. 7.p trip list, Art. 7.p trip form (with inline eligibility checklist open), Modelo 720 inputs panel on Profile, Sessions UI, multi-grant dashboard (≥2 tiles + stacked view open).
- **G-23 (extended).** Art. 7.p eligibility checklist uses a **label + icon** for each criterion's pass/fail state; color alone never communicates a criterion's outcome. (Note: "outcome" here is capture-only; Slice 2 does not evaluate eligibility.)
- **G-24 (inherited).** `prefers-reduced-motion`: stacked vesting curve drawing is instant (no reveal animation) when the preference is set.

### 3.5 GDPR / data minimization

- **G-26 (extended).** The Slice-1 payload schema lint extends to all Slice-2 event payloads. Specifically, analytics events for ESPP purchase create/update/delete must **not** include: FMV values, purchase price, share counts, currency, discount %, lookback FMV, grant employer. Events for Art. 7.p trips must **not** include: destination country, `from_date`, `to_date`, employer-paid boolean, purpose text, eligibility criterion values. Events for Modelo 720 inputs must **not** include: bank-account total or real-estate total. Event payload may contain only the action verb and a surface identifier (e.g., `{ surface: "espp_purchase", verb: "create" }`) plus the user UUID.
- **G-27 (inherited).** Analytics opt-in default off; Slice-2 surfaces inherit the consent state set during Slice-0 cookie banner.
- **G-28 (inherited).** Slice 2 still runs entirely on a developer machine. No external network calls are added by this slice. (Finnhub and ECB both remain deferred to later slices.)
- **G-29 (inherited).** Logs redact emails and raw IPs. Specifically for the Sessions UI: server-side queries must return only the `ip_hash`-derived rendering to the UI; raw IP never leaves the database row and never appears in a structured log line outside Caddy's 7-day access log (SEC-054 boundary).

### 3.6 Observability + audit log

- **G-30 (inherited).** Every request logs the Slice-1 baseline fields.
- **G-31 (inherited).** Auth events continue to write to `audit_log` unchanged.
- **G-32 (extended).** Grant create/edit/delete continues to write the Slice-1 audit-log entries. **New Slice-2 audit-log entries required**:
  - `espp_purchase.create`, `espp_purchase.update`, `espp_purchase.delete` — `target_kind = "espp_purchase"`, `target_id = espp_purchase.id`; `payload_summary` contains only `{ grant_instrument: "espp" }`. **Never** includes FMV, purchase price, share count, currency, or discount %.
  - `art_7p_trip.create`, `art_7p_trip.update`, `art_7p_trip.delete` — `target_kind = "art_7p_trip"`, `target_id = art_7p_trip.id`; `payload_summary` contains only `{ checklist_completed: bool }`. **Never** includes destination country, dates, employer-paid flag, purpose text, or individual criterion values.
  - `modelo_720_inputs.upsert` — `target_kind = "modelo_720_input"`, `target_id = modelo_720_input.id` (the newly-inserted row); `payload_summary` contains only `{ categories_changed: ["bank_accounts" | "real_estate"] }`. **Never** includes the totals.
  - `session.revoke` — `target_kind = "session"`, `target_id = session.id`; `payload_summary` contains `{ revoke_kind: "single" | "all_others", initiator: "self" }`. Raw IP never present; `ip_hash` is an acceptable field on the row if ADR-016 specifies it (but the payload must carry nothing more granular than the `revoke_kind`).

## 4. "Record ESPP purchase" flow (new in Slice 2)

Reference screens: `grant-detail.html` (hosting the new "Record purchase" CTA + purchases list), and a new dedicated form route `/app/grants/:id/espp-purchases/new` (ADR-016 defines the route shape; this document pins the AC surface).

Sequence: **Grant-detail screen (existing ESPP grant) → "Record purchase" CTA → purchase form → save → back to grant detail with purchases list updated**.

### 4.1 Entry points and visibility

- **AC-4.1.1 — CTA visibility (ESPP only).** Given the user is on the grant-detail screen for a grant with `instrument = "espp"`, when the page loads, then a primary CTA `Registrar compra ESPP` (ES) / `Record ESPP purchase` (EN) is visible in the header of the grant-detail screen. The CTA is **not** visible for RSU, NSO, or `iso_mapped_to_nso` grants.
- **AC-4.1.2 — CTA routing.** Clicking the CTA navigates to `/app/grants/:id/espp-purchases/new` where `:id` is the parent grant's id.
- **AC-4.1.3 — Purchases list surface.** Given the user is on the grant-detail screen for any ESPP grant, when the page loads, then a "Compras registradas" (ES) / "Recorded purchases" (EN) section renders below the summary / above the vesting timeline. If the grant has zero purchases, the section renders with empty-state copy: `Aún no has registrado ninguna compra. Las compras ESPP deben registrarse con FMV y precio de cada ventana.` (ES) / `No purchases recorded yet. ESPP purchases must be logged with FMV and per-window price.` (EN).
- **AC-4.1.4 — Purchases list pagination.** If a grant has >10 purchases, the list renders the 10 most recent (by `purchase_date` DESC) with a "Ver todas" / "See all" affordance. Purchase ordering is stable across refreshes.
- **AC-4.1.5 — Link copy confinement.** The CTA and the list do not appear anywhere outside the ESPP-grant detail screen. In particular, they do not appear on RSU/NSO grant-detail screens, on the dashboard, or on the Slice-1 first-grant form.

### 4.2 Form fields, defaults, validation

- **AC-4.2.1 — mandatory fields.** The form exposes exactly these mandatory fields, in this order:
  1. **Fecha de oferta** (`offering_date`) — date picker.
  2. **Fecha de compra** (`purchase_date`) — date picker.
  3. **FMV en fecha de compra** (`fmv_at_purchase`) — decimal, up to 4 decimals, >0.
  4. **Precio pagado por acción** (`purchase_price_per_share`) — decimal, up to 4 decimals, >0.
  5. **Acciones compradas** (`shares_purchased`) — integer, ≥1.
  6. **Moneda** (`currency`) — select; allowed values `USD | EUR | GBP` (per Q2 lock). Default: the parent grant's currency if known, else USD.
- **AC-4.2.2 — optional fields.** The form exposes exactly these optional fields, collapsed under a disclosure labelled `Detalles del plan con lookback` (ES) / `Lookback plan details` (EN):
  - **FMV en fecha de oferta** (`fmv_at_offering`) — decimal, up to 4 decimals. Required only if the user intends to later model lookback — not enforced at save time.
  - **% de descuento del empleador** (`employer_discount_percent`) — decimal, 0–100. Default blank (not 0 and not 15). On creation of the first purchase for an ESPP grant whose Slice-1 `grants.notes` carries `estimated_discount_percent`, the field pre-fills with that value (see AC-4.5.1 migration).
- **AC-4.2.3 — purchase_date ≥ offering_date.** Given the user enters `purchase_date < offering_date`, when they submit, then inline rejection: `La fecha de compra no puede ser anterior a la fecha de oferta.` (ES) / `Purchase date cannot precede offering date.` (EN). No row is created.
- **AC-4.2.4 — positive share count.** Given `shares_purchased <= 0` or non-integer, when submit, then inline rejection: `Introduce un número entero de acciones mayor que 0.` (ES) / `Enter a whole share count greater than 0.` (EN).
- **AC-4.2.5 — positive FMV and purchase price.** Given `fmv_at_purchase <= 0` or `purchase_price_per_share <= 0`, when submit, then inline rejection: `Introduce un valor positivo.` (ES) / `Enter a positive value.` (EN) on the offending field.
- **AC-4.2.6 — currency constraint.** Given the user selects a `currency` value not in `{USD, EUR, GBP}` (e.g., via a crafted request bypassing the select), when submit, then the API rejects with a validator error `Moneda no admitida.` / `Currency not supported.` and returns 400. The select UI enforces the three-value set.
- **AC-4.2.7 — future purchase_date warning.** Given `purchase_date > today + 1 day`, when submit, then a non-blocking warning (same pattern as AC-4.2.9 in Slice 1): `La fecha es futura. ¿Estás seguro?` / `That date is in the future. Are you sure?` User may proceed.
- **AC-4.2.8 — duplicate-purchase detection policy.** Given the user attempts to save a purchase for the same parent grant with an identical `(offering_date, purchase_date, shares_purchased)` triple as an existing, non-deleted purchase, when submit, then a soft warning: `Ya has registrado una compra con esta ventana y número de acciones. ¿Es un duplicado?` (ES) / `You already recorded a purchase with this window and share count. Is this a duplicate?` (EN). Submission requires a second explicit confirm click to proceed. No hard block. (Rationale: ESPP plans can legitimately have multiple purchases on the same purchase_date for the same employee only in edge cases; we warn but do not block.)
- **AC-4.2.9 — numeric-field error copy parity.** For every numeric field, the ES and EN error messages are the same string resource set. CI lint rejects a PR introducing a numeric validator whose ES text lacks an EN peer.
- **AC-4.2.10 — optional `fmv_at_offering` ≥ 0 if present.** Given the user enters `fmv_at_offering` and leaves it non-blank, when submit, then the value must be `>0`. If blank, the row is saved with `fmv_at_offering = NULL`.
- **AC-4.2.11 — optional `employer_discount_percent` range if present.** Given the user enters a value, when submit, then the value must be in `[0, 100]`. If blank, the row is saved with `employer_discount_percent = NULL`.

### 4.3 Successful create, read, edit, delete

- **AC-4.3.1 — successful create.** Given a valid form, when submit, then an `espp_purchases` row is created under the current `user_id`'s RLS scope, linked to the parent `grant_id`. The user lands back on the grant-detail screen with the purchases list updated. A success flash renders: `Compra ESPP registrada.` / `ESPP purchase recorded.`
- **AC-4.3.2 — audit log on create.** `audit_log` row written per G-32.
- **AC-4.3.3 — read: purchases list shows per-row fields.** Each purchases-list row displays: `purchase_date` (G-14 format), `shares_purchased` (G-13 format), `purchase_price_per_share` + currency suffix, `fmv_at_purchase` + currency suffix, `offering_date` (G-14 format, muted), and the optional `employer_discount_percent` (if present, as `· 15 %`). No tax interpretation anywhere.
- **AC-4.3.4 — edit.** Clicking a purchase in the list opens the form pre-populated. Save triggers the same validation ACs as create (AC-4.2.*). On save, an `espp_purchase.update` audit row is written.
- **AC-4.3.5 — delete.** A two-step confirm is available per purchase row. On confirm, the row is hard-deleted (consistent with Slice-1 grant-delete behaviour, per AC-6.2.4). An `espp_purchase.delete` audit row is written.
- **AC-4.3.6 — RLS cross-tenant 404.** A request for an `espp_purchase_id` under a different user's scope returns a 404 rendering of the grant-detail screen (not 403); consistent with AC-7.3 in Slice 1.

### 4.4 Integration with grant detail

- **AC-4.4.1 — parent-grant presence.** An `espp_purchases` row always has a non-null `grant_id` whose parent `grants.instrument = "espp"`. If the parent grant is deleted (AC-6.2.4 from Slice 1), purchases cascade-delete per the ADR-005 FK topology.
- **AC-4.4.2 — purchases do not alter the vesting timeline.** ESPP purchases in Slice 2 do not appear on the grant-detail vesting timeline (which remains the Slice-1 cumulative-curve view). Purchases are a separate block below the summary and above the vesting timeline.
- **AC-4.4.3 — no purchases on Slice-1 dashboard tiles.** The dashboard tile for an ESPP grant continues to render from the Slice-1 fields (instrument, share_count, grant_date, vested-to-date, sparkline). Purchases are not summed into the tile in Slice 2.

### 4.5 Migration of the Slice-1 `grants.notes` ESPP discount compromise

Slice-1 AC-4.2.2 captured the ESPP `estimated_discount_percent` inside `grants.notes` (a JSON field) because the `espp_purchases` table did not yet exist. This compromise is retired in Slice 2 via a non-lossy migration.

- **AC-4.5.1 — migration on first purchase.** Given an ESPP grant created in Slice 1 with `grants.notes` carrying an `estimated_discount_percent` field, when the user opens the "Record ESPP purchase" form for the first time on that grant, then the form's `employer_discount_percent` field pre-fills with the Slice-1 value. Saving the purchase writes the discount into the new `espp_purchases.employer_discount_percent` column (or the parallel field ADR-016 specifies — this doc pins the requirement, not the column name). The `grants.notes.estimated_discount_percent` key is **not** removed from the JSON in the same transaction (read-only compatibility is preserved for any read path that still consults it).
- **AC-4.5.2 — no lossy transform.** If the Slice-1 value was stored as a numeric string (e.g., `"15.0"`), the migration preserves it as a numeric with no silent rounding or truncation. If the value was absent from `grants.notes`, the field pre-fills blank (not 0, not 15).
- **AC-4.5.3 — deferred cleanup.** Removal of the `estimated_discount_percent` key from existing `grants.notes` JSON payloads is deferred (not in this slice). A follow-up cleanup task may be filed; it is not a Slice-2 gate.

## 5. Art. 7.p trips flow (new in Slice 2)

Reference: sidebar entry "Viajes Art. 7.p" (from `session-management.html` and `dashboard.html` left-nav). In Slice 1 this link routed to "próximamente"; in Slice 2 it routes to a live list + form.

**Scope reminder (US-005).** Slice 2 captures trip facts and each trip's eligibility-checklist answers as data. **No calculation.** The €60,100 cap, the pro-rata by qualifying days, the 0-day omission, and the overlap/domestic rejection (all from US-005 ACs) are **not evaluated** in Slice 2; they will be evaluated in Slice 4 when the tax engine goes live. This slice ensures the data exists and is captured coherently.

### 5.1 Trip list screen

- **AC-5.1.1 — empty state.** Given a user with zero trips, when they load the Art. 7.p trips screen, then they see: a headline `Viajes Art. 7.p`, one-sentence copy: `Registra aquí los viajes profesionales al extranjero cuyo beneficio revirtió en una entidad no residente en España.` (ES) / `Log here your professional trips abroad whose benefit accrued to a non-Spain-resident entity.` (EN), and a primary CTA `Añadir viaje` / `Add trip`.
- **AC-5.1.2 — populated list.** Given ≥1 trip, when the list loads, then each trip row shows: destination country (ISO 3166-1 alpha-2 flag + Spanish country name), `from_date`–`to_date` (G-14 format), purpose (truncated to 60 chars with ellipsis), a **checklist summary chip** (`Apto (5/5)` / `Capturado — revisión pendiente (3/5)` / `No apto (marcado)`). The chip is a **data-capture summary only** and must not say "exento" or "no exento". Clicking a row opens the edit form.
- **AC-5.1.3 — annual cap tracker (read-only).** Below the trip list, a read-only panel displays: `Días declarados en 2026: N · Cap anual €60.100 (no aplicado en esta fase)` (ES) / `Days declared in 2026: N · Annual cap €60,100 (not applied in this phase)` (EN). The value `N` is a simple sum of trip days in the current tax year (inclusive of both endpoints). **No EUR amount is computed here.** The panel explicitly states that the cap is for reference and Slice 2 does not apply it.
- **AC-5.1.4 — year selector.** The panel exposes a year selector defaulting to the current tax year (calendar year in Spain); selecting a prior year recomputes the day count for trips whose range intersects that year.

### 5.2 Trip form — fields and inline eligibility checklist

- **AC-5.2.1 — mandatory fact fields.** The form exposes, in this order:
  1. **País de destino** (`destination_country`) — single-select, ISO 3166-1 alpha-2 codes, searchable by Spanish name.
  2. **Fecha de inicio** (`from_date`) — date picker.
  3. **Fecha de fin** (`to_date`) — date picker; must be ≥ `from_date`.
  4. **Motivo / descripción** (`purpose_text`) — free text, ≤500 chars, required (not blank).
  5. **Gastos pagados por el empleador** (`employer_paid`) — yes/no radio, default No.
- **AC-5.2.2 — inline eligibility checklist.** Below the fact fields, an inline panel `Lista de verificación Art. 7.p` presents five yes/no controls (one per US-005 eligibility criterion):
  1. `services_rendered_outside_spain` — **Los servicios se prestaron físicamente fuera de España.** (Yes / No; default blank — user must answer.)
  2. `employer_non_spanish_or_pe` — **El empleador es una entidad no residente en España o un establecimiento permanente que se beneficia del trabajo.** (Yes / No; default blank.)
  3. `country_not_tax_haven` — **El país de destino no figura en la lista española de paraísos fiscales.** (Yes / No; default blank. A contextual help link explains how this is determined but the tool does not auto-resolve; user answers.)
  4. `no_double_exemption_elsewhere` — **El trabajo no es objeto de una exención equivalente en otra jurisdicción.** (Yes / No; default blank.)
  5. `within_annual_cap` — **No excederé el tope anual de €60.100 considerando este y mis demás viajes.** (Yes / No; default blank. This is a **user self-assertion**, not a computed check.)
- **AC-5.2.3 — submission requires all checklist answers.** Given any of the five checklist items is left blank, when submit, then inline rejection: `Responde a los cinco criterios antes de guardar.` / `Answer all five criteria before saving.` The offending items highlight with the G-18 `aria-describedby` pattern.
- **AC-5.2.4 — validation: dates ordered.** Given `to_date < from_date`, when submit, then inline rejection on `to_date`.
- **AC-5.2.5 — validation: dates sane.** Given `to_date > today + 365 days`, when submit, then a non-blocking future-date warning (same pattern as Slice-1 AC-4.2.9 and this slice's AC-4.2.7).
- **AC-5.2.6 — validation: destination non-empty.** Given `destination_country` is blank, when submit, then inline rejection.
- **AC-5.2.7 — no validation against overlap or domestic-Spain.** Slice 2 **does not** reject overlapping trips (US-005 AC that rejects them is a Slice-4 calculation). Overlap is stored as-is; the Slice-4 tax engine handles dedupe. Slice 2 also does not reject a trip where `destination_country = "ES"`; the eligibility checklist already captures the "services rendered outside Spain" answer. A Slice-2 soft hint: if `destination_country = "ES"`, the form shows an advisory note `Un viaje dentro de España no suele cumplir el Art. 7.p; confirma las respuestas de la lista de verificación.` — non-blocking.

### 5.3 Successful create, read, edit, delete

- **AC-5.3.1 — successful create.** A valid form submission writes one row to `art_7p_trips` with the five fact fields + five checklist booleans. Audit-log row per G-32.
- **AC-5.3.2 — edit.** Clicking a trip opens the form pre-populated with the saved data (including the five checklist answers, not re-blanked). Save triggers the same ACs; re-submitting unchanged data is a no-op (no redundant audit-log row).
- **AC-5.3.3 — delete.** Two-step confirm; hard-delete; audit-log row per G-32.
- **AC-5.3.4 — RLS cross-tenant 404.** Consistent with AC-7.3 in Slice 1.
- **AC-5.3.5 — checklist booleans never mutated.** The five checklist booleans, once saved, are never mutated by any background job. They are user-authored data; only a user edit changes them.

## 6. Modelo 720 inputs on Profile (new in Slice 2)

Reference: Profile screen (the Slice-1 residency edit surface at Account → Perfil y residencia; `residency-setup.html` is the closest UX mock). A new panel **"Modelo 720 — valores declarados"** lands below the residency fields.

**Scope reminder (Q4 + US-007 + OQ-12).** Slice 2 captures **two of the three Modelo 720 categories** as user-entered totals: bank accounts and real estate. The **securities** category is derived from grants via FX (Slice 3); in Slice 2 the securities row is stubbed with `Calculation requires activar seguimiento fiscal` copy (see AC-6.1.5). The time-series shape matches `residency_periods`: each save **closes** the prior open row and **inserts** a new row.

### 6.1 Panel layout and display

- **AC-6.1.1 — panel presence.** Given the user lands on Profile, when the page loads, then the M720 inputs panel renders below the residency fields. Panel headline: `Modelo 720 — valores declarados` (ES) / `Modelo 720 — declared values` (EN).
- **AC-6.1.2 — three category rows.** The panel shows three rows, in this order:
  1. **Valores extranjeros** (`securities`) — stubbed (see AC-6.1.5).
  2. **Cuentas bancarias extranjeras** (`bank_accounts`) — editable.
  3. **Bienes inmuebles extranjeros** (`real_estate`) — editable.
- **AC-6.1.3 — display of last-reference-date.** Each editable row displays the `from_date` of the currently-open row as "Desde {G-14 date}". If no row exists, the row displays `Sin valor registrado`. The prior-closed row's `to_date` is not displayed in-panel; it is surfaced only in the audit log.
- **AC-6.1.4 — zero-value vs null distinction.** Given the user explicitly saves `0` for a category, when the row persists, then the panel renders `€0 desde {G-14 date}`. Given no row has ever been saved for a category, when the page loads, then the panel renders `Sin valor registrado`. The two states are visually distinguishable and must never collapse to the same render.
- **AC-6.1.5 — securities row stub.** The securities row renders read-only with copy: `Se calcula a partir de tus grants al activar seguimiento fiscal (próximamente).` (ES) / `Computed from your grants when fiscal tracking is enabled (coming soon).` (EN). No edit control. No number. This copy is a **deliberate stub** — do not flag as defect.

### 6.2 Edit flow and time-series semantics

- **AC-6.2.1 — inline edit.** Clicking a row's value opens an inline edit control: a numeric field (EUR, up to 2 decimals, ≥0) + Save / Cancel. The field pre-fills with the currently-open row's value (or blank if none).
- **AC-6.2.2 — close-and-create on save.** Given the user saves a new value for a category, when the save completes, then: (a) the currently-open row (`to_date IS NULL`) for that category is updated with `to_date = today`; (b) a new row is inserted with the new value, `from_date = today`, `to_date = NULL`; (c) atomically — if either write fails, neither applies.
- **AC-6.2.3 — same-day re-edit.** Given the user saves a value, then edits the same category again on the same calendar day (UTC+01/02 per server TZ), when the second save completes, then the most recent row (now just-closed) is the one immediately superseded. The net effect: the DB carries a 1-day open-then-closed row + the new open row. This is not an error; it is the natural consequence of close-and-create and is acceptable. (A DB-side squash is a follow-up optimization, not a Slice-2 gate.)
- **AC-6.2.4 — cancel.** Clicking Cancel discards the pending edit without writing any row.
- **AC-6.2.5 — audit log on upsert.** A `modelo_720_inputs.upsert` row per G-32 is written on each successful save. If the user saves the same value as the currently-open row (no-op), no audit row is written.
- **AC-6.2.6 — read back.** On page reload after a save, the panel renders the new value + its `from_date`. Year-on-year history is not surfaced in Slice 2 UI (it exists as time-series data in the DB, available to the Slice-3 threshold alert and a possible future history view).

### 6.3 Integration with downstream slices (Slice-2 scope boundary)

- **AC-6.3.1 — no threshold alert in Slice 2.** The panel carries no banner, no chip, no warning even when the declared value exceeds €50,000 per category. The Slice-3 threshold alert surface reads the currently-open row and fires there. Tester do-not-flag.
- **AC-6.3.2 — no FX in Slice 2.** All values are entered in EUR directly. There is no conversion from another currency, no ECB chip. Tester do-not-flag.
- **AC-6.3.3 — no worksheet export.** No PDF/CSV export of M720 inputs in Slice 2. Worksheet ships in Slice 6. Tester do-not-flag.

## 7. Session / device list UI in Account (new in Slice 2)

Reference screen: `docs/design/screens/session-management.html`. Backend endpoints (list + revoke) existed in Slice 1 but had no UI; Slice 2 closes the C-7 phase-gap by shipping the UI against the same endpoints.

### 7.1 Panel layout

- **AC-7.1.1 — entry path.** Given the user navigates to Account → Sesiones activas (per the sidebar in `session-management.html`), when the page loads, then the sessions panel renders. The link is visible regardless of MFA status (MFA is not a Slice-2 concern; optional TOTP ships in Slice 7).
- **AC-7.1.2 — sessions table.** Each active, non-revoked session renders as a row with: `user_agent` (parsed into a short summary such as `Firefox 128 · macOS`), a coarse **location hint** (see AC-7.1.3), `created_at` (G-14 + short time HH:MM CET), `last_used_at` (humanized — "hace 2 min" / "2 min ago", G-11 locale-aware).
- **AC-7.1.3 — IP rendering (pick one, documented).** The UI renders a **coarse geo hint derived server-side from the creation-time country lookup** (e.g., `Madrid, ES (aprox.)`). Raw IP is **never** displayed in the UI, never serialized in any response body, and never included in any server log outside Caddy's 7-day access log (SEC-054 boundary). (The alternative option from the brief — "redacted beyond /24 for v4 + /48 for v6" — is **not** selected; the country-lookup approach gives the user a more meaningful signal with lower re-identification risk.)
- **AC-7.1.4 — current-session highlight.** The session matching the request's own refresh-token family renders with a visual highlight and the label `sesión actual` / `current session`. Its revoke button is **disabled** with a tooltip `Cierra sesión desde el menú del usuario para terminar esta sesión.` / `Sign out from the user menu to end this session.` (See AC-7.2.3 — self-revoke is forbidden from this UI.)

### 7.2 Revoke actions

- **AC-7.2.1 — revoke a single other session.** Given the user clicks `Cerrar esta sesión` on a non-current row, when they confirm the modal, then the corresponding `sessions.revoked_at` is set to `now()`, the row is removed from the UI, and a `session.revoke` audit-log row per G-32 is written with `revoke_kind = "single"`.
- **AC-7.2.2 — revoke all other sessions.** A button `Cerrar todas las demás sesiones` / `Revoke all other sessions` (danger style) is visible below the table. On click + confirm, all non-current `sessions` rows for the user get `revoked_at = now()`; the UI re-renders to show only the current session; a single `session.revoke` audit-log row per G-32 is written with `revoke_kind = "all_others"`.
- **AC-7.2.3 — current session cannot self-revoke from this UI.** The current-session row has no enabled revoke button. Attempting to revoke the current session via a crafted request (if the backend still accepts it) is **not** a Slice-2 concern for the UI layer — UX prevents it, and any such policy is ADR-016 / ADR-011's lane.
- **AC-7.2.4 — revoked sessions filtered out.** The UI never renders sessions whose `revoked_at IS NOT NULL`. A session that expired naturally (past the refresh-token TTL) is likewise filtered.
- **AC-7.2.5 — empty state.** Given only the current session is active, when the page loads, then the table renders a single row (the current one) and the "Cerrar todas las demás" button is disabled with tooltip `No hay otras sesiones activas.` / `No other sessions are active.`

### 7.3 Security and data boundaries

- **AC-7.3.1 — no cross-tenant leakage.** The sessions list returns only rows where `user_id = current_user`. Consistent with RLS enforcement (ADR-005 §RLS).
- **AC-7.3.2 — `ip_hash` only in logs.** Any server-side log line emitted during list or revoke operations references the session by `ip_hash` (HMAC-SHA256 per SEC-054), never by raw IP. This is enforced via the Slice-1 G-29 log scrubber extended to cover the new request paths.
- **AC-7.3.3 — rate limit (already in place).** The list + revoke endpoints inherit the Slice-0 authenticated-endpoint rate limit. No new rate-limit ACs in Slice 2.

## 8. Multi-grant dashboard refresh (new in Slice 2)

Reference screens: `dashboard.html` (Slice-3+ target with FX/tax numbers — **do not** ship those numbers in Slice 2), `dashboard-slice-1.html` (Slice-1 baseline). Slice 2's dashboard extends the Slice-1 baseline with: (a) N-grant tile rendering, (b) per-employer stacking of refresh grants into a cumulative view, (c) per-instrument drill-down when a stacked employer has mixed instruments.

### 8.1 Tile list with N ≥ 0 grants

- **AC-8.1.1 — empty state unchanged.** Given 0 grants, the dashboard renders the Slice-1 empty state per AC-5.1.1 of Slice 1. No change in Slice 2.
- **AC-8.1.2 — N-grant rendering.** Given ≥1 grants, the dashboard renders one tile per grant. Tiles keep the Slice-1 fields (AC-5.2.1 Slice 1): employer, instrument + count label, share count, grant date, vested-to-date integer, small vesting sparkline.
- **AC-8.1.3 — native currency only.** Values on tiles remain in native currency with explicit suffix (e.g., `$8.00 USD`). **No EUR conversion** — tester do-not-flag (Slice 3 unblocks EUR).
- **AC-8.1.4 — ordering.** Tiles render by `grant_date` DESC by default; a sort toggle lets the user switch to employer-then-date. Ordering is stable across refreshes.
- **AC-8.1.5 — per-tile header still shows instrument + count.** Even for stacked-employer tiles (see §8.2), the per-tile header continues to read `{employer} · {instrument} · {N} acciones` / `{employer} · {instrument} · {N} shares`.
- **AC-8.1.6 — "Añadir grant" CTA.** The secondary CTA `Añadir grant` stays visible regardless of N.

### 8.2 Stacked refresh-grant cumulative view (US-003 AC #4)

The stacked view is the Slice-2 realization of US-003 AC #4 — "Given multiple stacked refresh grants, When viewed together, Then a combined cumulative-vesting chart is shown with per-grant drill-down."

- **AC-8.2.1 — stacking trigger: same employer.** Given ≥2 grants share the same `employer_name` string (case-insensitive match, trimmed of trailing whitespace), when the dashboard loads, then those grants merge into a single **"Stacked: {employer}"** cumulative chart. Grants with unique employers continue to render as individual tiles.
- **AC-8.2.2 — stacked chart contents.** The stacked chart renders one cumulative curve per grant (same algorithm as Slice-1 AC-4.3.1..5), overlaid on a shared date axis. The envelope (sum across grants at each date) is rendered as a thicker line; each grant's individual curve is rendered in a lighter weight with a legend.
- **AC-8.2.3 — per-grant drill-down.** Clicking a grant name in the legend (or its individual curve) navigates to that grant's detail screen. The stacked chart itself does not provide inline edit.
- **AC-8.2.4 — mixed-instrument stacking.** Given two or more stacked grants for the same employer have **different instruments** (e.g., one RSU + one NSO), when the stacked view renders, then the summed cumulative envelope is still displayed, and the drill-down list in the legend renders **per-instrument** — i.e., the legend groups entries by instrument with a sub-header per instrument (`RSU (1 grant)`, `NSO (1 grant)`). Clicking any legend entry navigates to that grant's detail.
- **AC-8.2.5 — double-trigger handling in the stack.** If any grant in the stack is a double-trigger RSU whose `liquidity_event_date IS NULL`, that grant's contribution to the stacked envelope uses the **dashed-fill** visual from Slice-1 AC-6.1.4. The envelope's summary line asserts `Ingresos imponibles hasta la fecha: 0 acciones` only if **all** contributing grants have zero taxable-to-date shares; otherwise it shows the sum of realized-taxable-vested shares.
- **AC-8.2.6 — native currency only (re-assert).** The stacked view shows share counts, not monetary values. No EUR conversion required; no paper-gains in the stacked view. Tester do-not-flag.
- **AC-8.2.7 — no stack of size 1.** If an employer has only one grant, no "Stacked:" tile is created — the grant renders as a normal single-tile.
- **AC-8.2.8 — deterministic envelope.** The envelope is the sum of the same per-grant vesting curves that Slice-1 AC-4.3.5 asserts are deterministic. CI includes a property-based test that a stacked employer's envelope value at date D equals the sum of its constituent grants' vested counts at date D.

## 9. "Tengo varios grants" link copy update

Slice-1 AC-4.2.11 shipped the link with copy referencing "después" (later) and the link dismissed to an empty dashboard. In Slice 2, the copy updates to reference Slice 8 (bulk import) as the concrete target; the link's **destination behaviour does not change in Slice 2** (still dismisses to an empty dashboard, because bulk-import tooling lands only in Slice 8).

- **AC-9.1 — updated ES copy.** The link on the first-grant form (Slice-1 AC-4.2.11) reads: `Tengo varios grants — habrá una importación masiva (Carta / Shareworks / ETrade) más adelante; por ahora, puedes añadirlos uno a uno desde el dashboard.`
- **AC-9.2 — updated EN copy.** `I have multiple grants — bulk import (Carta / Shareworks / ETrade) is coming later; for now, you can add them one by one from the dashboard.`
- **AC-9.3 — destination unchanged.** Click still dismisses the form and lands the user on the dashboard (empty or populated depending on prior state). No new route; no "próximamente" page beyond the existing sidebar stubs. In Slice 8 the destination flips to the import landing page.
- **AC-9.4 — link styling unchanged.** Link typography, position below the first-grant form, and `focus-visible` handling remain identical to Slice 1; only the string content changes.
- **AC-9.5 — CI i18n lint.** The new ES + EN strings pass the G-11 lint; the old strings are removed from both catalogs.

## 10. Error and edge states

- **AC-10.1 — network error during submit preserves form state.** On any of the four new form surfaces (ESPP purchase, Art. 7.p trip, M720 inputs inline-edit, session-revoke confirm), a server error during submit renders inline banner `No se pudo guardar. Inténtalo de nuevo.` / `Could not save. Try again.` The form state is preserved client-side; no partial-save occurs.
- **AC-10.2 — session expiry redirect preserving path.** A session that expires mid-edit on any Slice-2 surface triggers a redirect to the login screen with flash `Tu sesión ha caducado.` / `Your session expired.` On re-login the user lands back on the originating path (extends AC-7.2 Slice 1). For the ESPP purchase form specifically, unsaved inputs are **not** preserved across the re-login round-trip (a known v1 limitation — acceptable because the persistence surface is small).
- **AC-10.3 — cross-tenant 404 not 403.** For every new surface (ESPP purchase, trip, M720 input row, session row) a request for an id outside the current user's RLS scope returns 404, not 403. This is the consistent Slice-1 AC-7.3 posture.
- **AC-10.4 — validator + CHECK-constraint shared error envelope.** Validation errors arising from (a) client-side Zod/Yup validation, (b) server-side validator layer, (c) Postgres CHECK constraint violation all surface to the UI via the same error envelope shape — a top-level `errors` array with `{ field, code, message }` entries. No distinction in UX between "validator caught it" and "DB constraint caught it"; the message is locale-aware per G-11.
- **AC-10.5 — optimistic-concurrency on M720 upsert.** Given the user opens M720 inputs in two tabs and saves in both, when the second save arrives, then the second save succeeds (close-and-create is tolerant of sequential double-save); no 409 is surfaced to the user. The first save's row is the one closed; the second save's row is the new open row. Acceptable.
- **AC-10.6 — stale session list.** Given the user has the sessions list open in Tab A and revokes a session in Tab B, when the user acts on the stale session in Tab A (click revoke), then the backend returns 404 (row already revoked, filtered out of the scoped query); the UI re-fetches the list and renders the fresh state with an inline advisory `La lista se actualizó; la sesión ya no existía.`

## 11. Mobile / responsive

All Slice-2 surfaces meet the Slice-1 AC-8.1..5 mobile baseline. Additional Slice-2-specific assertions:

- **AC-11.1 — trip form + eligibility checklist stack vertically on ≤640 px.** The five fact fields + five checklist items each occupy full width; the checklist retains its radio-button pair per criterion; labels wrap without truncation in ES-first testing (G-15).
- **AC-11.2 — sessions table horizontal scroll on narrow viewports.** On ≤640 px, the sessions table may scroll horizontally with the **first column sticky** (the `user_agent` summary). The "Cerrar todas las demás sesiones" button wraps to its own line below the table.
- **AC-11.3 — ESPP purchases list on grant detail.** Purchases-list rows collapse to a two-line card layout on ≤640 px: top line `{purchase_date} · {shares_purchased}`, bottom line `{purchase_price}/sh · FMV {fmv_at_purchase}`.
- **AC-11.4 — M720 inputs panel.** Inline edit controls stack as full-width rows; the securities stub copy wraps gracefully.
- **AC-11.5 — stacked dashboard tile on mobile.** The stacked cumulative chart retains legibility on ≤640 px; if three or more grants stack, the chart adopts a compact legend (instrument + count, employer omitted since the tile header carries it).
- **AC-11.6 — touch targets ≥44×44 on `pointer: coarse`.** Inherited from Slice 1. All new controls in Slice 2 (CTAs, revoke buttons, yes/no radios, inline edit save/cancel) comply.

## 12. NFRs that do NOT apply (and why, explicitly)

This section is load-bearing. A tester validating Slice 2 must not mark these as defects.

- **§7.1 Tax-rule versioning.** No calculations exist in Slice 2. No rule-set chip is displayed anywhere. No `rule_sets` row is consulted. Full §7.1 activates in Slice 4. (Inherited from Slice 1.)
- **§7.4 Ranges-and-sensitivity.** No tax numbers in Slice 2 → no ranges. Share counts are integers; FMV and purchase-price are user-entered values, not estimates; the M720 panel echoes what the user typed. Full §7.4 activates in Slice 4.
- **§7.5 Autonomía rate tables.** Still not ingested in Slice 2; foral selection is stored from Slice 1 but produces no tax-calc block.
- **§7.6 Market-data vendor.** No quotes needed in Slice 2. Finnhub integration ships in Slice 5.
- **§7.7 FX source.** No EUR conversion in Slice 2. ECB pipeline ships in Slice 3. The M720 bank-account + real-estate values are user-entered in EUR directly; no conversion from another currency.
- **§7.8 Performance.** Slice 2 adds no compute-heavy path. The stacked-dashboard envelope is a sum of already-cached `vesting_events`. P95 targets in §7.8 are met trivially on a laptop-scale Postgres.
- **§7.9 Security — pen-test.** Still deferred to Slice 9 (was Slice 7 pre-v1.1; moved per v1.3). Slice 2 inherits the Slice-1 OWASP-baseline posture.

## 13. Demo-acceptance script

The 17-step Slice-1 flow is assumed complete; Slice 2's demo picks up from a persisted user who has 3 grants already (from an earlier session — Slice-1 demo outputs) and extends with the new surfaces.

1. Open `http://localhost:<port>` and sign in as `test+slice2@<domain>`. The Slice-1 dashboard renders with 3 existing grants (2 RSU at ACME, 1 NSO at ACME).
2. Observe the **stacked-employer view**: ACME's 2 RSU + 1 NSO grants merge into a single "Stacked: ACME" cumulative chart (AC-8.2.1, AC-8.2.4). Legend groups by instrument. The per-grant drill-down opens grant detail on click (AC-8.2.3).
3. Navigate to the ESPP grant (added in Slice 1 or added here via "Añadir grant" → ESPP, 100 shares, $30 offering, employer-discount 15%, notes carry the Slice-1 JSON value). On the grant-detail screen, the **"Registrar compra ESPP"** CTA is visible (AC-4.1.1).
4. Click "Registrar compra ESPP". The form renders (AC-4.1.2). Observe the `employer_discount_percent` field pre-filled with 15 from the Slice-1 `grants.notes` migration (AC-4.5.1).
5. Fill in: offering_date 2025-01-15, purchase_date 2025-06-30, fmv_at_purchase $45.00, purchase_price_per_share $38.25, shares_purchased 100, currency USD, leave `fmv_at_offering` blank. Submit. Purchase is saved; land back on grant detail with the purchases-list row rendered (AC-4.3.1, AC-4.3.3). Confirm `audit_log` carries `espp_purchase.create` with no FMV in the payload summary (AC-4.3.2 + G-32).
6. Navigate to sidebar "Viajes Art. 7.p". The empty state renders (AC-5.1.1). Click "Añadir viaje".
7. First trip — eligible capture: destination United States, from 2026-03-01, to 2026-04-15, purpose "Kickoff with NYC team, engineering reviews", employer_paid=Yes. Checklist: all five = Yes. Submit. Trip lands in the list with the chip `Apto (5/5)` (AC-5.1.2 — capture-only; no calculation claim).
8. Second trip — mixed/ineligible capture: destination "ES" (intra-Spain), from 2026-05-01, to 2026-05-03, purpose "Reunion regional en Barcelona", employer_paid=Yes. Advisory note renders (AC-5.2.7). Checklist: criterion 1 (services outside Spain) = No; others Yes or No as the tester prefers. Submit. Row renders with the chip summarizing "(X/5)" — again, capture-only.
9. Observe the annual-cap tracker panel below the list (AC-5.1.3): `Días declarados en 2026: ~48 · Cap anual €60.100 (no aplicado en esta fase)`. No calculation. Change year selector to 2025; panel shows `0 días declarados`. Revert.
10. Navigate to Account → Perfil y residencia. Scroll to the **Modelo 720** panel (AC-6.1.1). Inline-edit **Cuentas bancarias extranjeras**, save `25000.00`. Observe render `€25.000,00 desde {today}` (AC-6.1.4). Inline-edit **Bienes inmuebles extranjeros**, save `0`. Observe render `€0 desde {today}` (AC-6.1.4 zero-value explicit). Observe the **securities** row shows the stub copy (AC-6.1.5). Confirm `audit_log` carries two `modelo_720_inputs.upsert` rows (G-32).
11. Wait a simulated month (or adjust system clock). Edit **Cuentas bancarias extranjeras** again, save `40000.00`. Query the DB (or use a developer affordance): the prior `bank_accounts` row now has `to_date = today` and a new row has `from_date = today`, `to_date = NULL` (AC-6.2.2). The UI still shows only the currently-open row (AC-6.1.3).
12. Navigate to Account → Sesiones activas (AC-7.1.1). From a second device (or a second browser), sign in to the same account. Return to Sesiones activas in the first browser. Observe the new session row (AC-7.1.2) with the coarse location hint `Madrid, ES (aprox.)` (AC-7.1.3) and no raw IP anywhere.
13. Click "Cerrar esta sesión" on the second-device row. Confirm modal. The row disappears (AC-7.2.1). The audit_log carries one `session.revoke` row with `revoke_kind = "single"` and no raw IP (G-32 + AC-7.3.2).
14. Click "Cerrar todas las demás sesiones" — it is disabled (only the current session is active now, AC-7.2.5). Try to revoke the current session: the button is disabled with the documented tooltip (AC-7.1.4).
15. Return to the dashboard. Click **"Tengo varios grants"** on the first-grant form (reachable via "Añadir grant" → cancel to form's alternative-path link). The updated copy renders (AC-9.1). Click it; the form dismisses to the (now populated) dashboard (AC-9.3).
16. Run `axe` CI job on the PR's preview URL: zero violations across all new surfaces (G-21 extended).
17. Run the keyboard-only walkthrough of steps 3–14: every interaction is reachable via Tab / Shift-Tab / Enter / Space; focus ring visible at every step (G-19 inherited).
18. Inspect `audit_log` rows for this session:
    - `espp_purchase.create` × 1 (payload_summary carries only `{ grant_instrument: "espp" }`).
    - `art_7p_trip.create` × 2 (payload_summary carries only `{ checklist_completed: true|false }` per trip).
    - `modelo_720_inputs.upsert` × 3 (2 from step 10, 1 from step 11; payload_summary carries only `{ categories_changed: ["..."] }`).
    - `session.revoke` × 1 (step 13; payload_summary carries only `{ revoke_kind: "single", initiator: "self" }`).
    - None of these payloads contain monetary values, share counts, destination countries, dates, checklist-criterion values, or raw IPs.
19. Check product-analytics event payloads (if opted in): G-26 extended lint holds — no FMVs, no share counts, no totals, no destination countries, no checklist answers, no raw IPs in any payload.

If all 19 steps pass, Slice 2 is accepted.

## 14. Out-of-scope reminders (tester do-not-flag list)

The following are **correct** behaviours in Slice 2 and must not be written up as defects. Each item is anchored to the slice where it actually ships.

- **No EUR amount** appears on any Slice-2 surface (dashboard tiles, stacked view, M720 inputs other than the literally-EUR-entered values, ESPP purchases list). EUR conversion ships in **Slice 3** with the ECB pipeline.
- **No Modelo 720 threshold alert** renders, even when the user's declared bank-account + real-estate sum exceeds €50,000. The passive banner pattern ships in **Slice 3**.
- **No rule-set chip in the footer** on any page. The chip ships in **Slice 3** against the first FX-dependent surface; full tax-rule-set stamping in **Slice 4**.
- **No "you will owe X" number** on any Slice-2 surface.
- **No scenario-modeler CTA beyond the "próximamente" sidebar stub**. Scenario modeler ships in **Slice 4**.
- **No sell-now CTA beyond the "próximamente" sidebar stub**. Sell-now ships in **Slice 5**.
- **Art. 7.p eligibility is captured but not evaluated.** The trip list chip never says "exento" or "no exento"; no €60,100 cap math is applied. Calculation ships in **Slice 4**.
- **ESPP purchases have no tax-treatment rendering.** Rendimiento del trabajo at purchase, ahorro-base cap-gain at sale, lookback handling — all ship in **Slices 4 and 5**.
- **Securities row on M720 panel** remains stubbed with "activar seguimiento fiscal" copy. Securities calculation ships in **Slice 3** (when FX unblocks the grants-to-EUR conversion).
- **No CSV import** anywhere. "Tengo varios grants" link dismisses to the dashboard per AC-9.3. Bulk import (Carta + Shareworks CSV, ETrade PDF) ships in **Slice 8**.
- **No PDF / CSV export of anything** (including Modelo 720 worksheet, including ESPP purchases list, including Art. 7.p trip list). All exports ship in **Slice 6**.
- **No "recompute under current rules" action** anywhere. Ships dormant in **Slice 4**; activates the first time a second rule-set is published.
- **No sensitivity ranges** anywhere. Activates in **Slice 4**.
- **No IP address rendered in cleartext** anywhere in the sessions UI. This is the SEC-054 boundary and must remain so — inherited throughout v1.
- **No raw email in logs.** Log scrubber inherited from Slice 0a.
- **2FA is still not offered.** The Account screen has no TOTP setup UI in Slice 2; optional TOTP ships in **Slice 7**.
- **"Export my data" / "Delete my account"** still route to the "próximamente" page. Full DSR self-service ships in **Slice 7**.
- **No legal surface** (privacy policy, sub-processor list, DPA) is published. Ships in **Slice 9**.
- **No pen-test gate.** Ships in **Slice 9**.
- **No paid tier anywhere.** Every account gets every Slice-2 surface. No `[paid]` badges, no blurred previews, no feature matrix. (v1.2 PoC posture — permanent.)
