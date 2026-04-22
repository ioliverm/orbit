# Orbit v1 — Slice 3b acceptance criteria

| Field       | Value                                                      |
|-------------|------------------------------------------------------------|
| Version     | 1.0                                                        |
| Date        | 2026-04-22                                                 |
| Owner       | requirements-analyst (Ivan Oliver)                         |
| Slice       | Slice 3b — "Sell-to-cover withholding + tax preferences" (see `v1-slice-plan.md` v1.5) |
| Boundary    | `user_tax_preferences` time-series sidecar · ALTER `vesting_events` with 5 sell-to-cover columns + CHECK coherence · `orbit_core::sell_to_cover::compute` pure function with TS parity mirror and shared fixture · Profile "Preferencias fiscales" section (country + `rendimiento_del_trabajo_percent` + `sell_to_cover_enabled`) · Vesting-events editor refactor (inline rows → per-row **dialog**) with derived values panel + editable fields · default-sourcing of `tax_withholding_percent` from the user's active tax preferences · extended `PUT /api/v1/grants/:gid/vesting-events/:eid` accepting the new fields + `clearSellToCoverOverride` · three new audit actions (SEC-101-strict). **No tax math. No worker changes. No NSO sell-to-cover. No dual-residency concurrent rows. No per-grant tax-percentage default. No GeoIP country auto-detect. No automatic FMV ↔ sell-price reconciliation banner. No retroactive back-fill on pre-Slice-3b vests.** |
| Related     | Spec US-001 (grant CRUD — ESPP + RSU; unchanged here), US-003 (vesting schedules; unchanged rendering), US-004 (scenario modeler — Slice 4 consumer of sell-to-cover data); spec §L319/L334 (RSU cap-gains basis — amended in ADR-018: `basis = fmv_at_vest × net_shares_delivered` when sell-to-cover applied); Slice-2 AC-4.2.6 (currency whitelist); Slice-3 AC-8 (editable past vesting events — **replaced by the dialog in Slice 3b**), Slice-3 AC-5.4.1 (RSU basis for paper-gains tile — unchanged in Slice 3b; Slice 4 updates the basis to consume net-delivered); Slice-3 AC-10.5 (optimistic-concurrency pattern on vesting-events edits); UX screens `grant-detail-vesting-editor.html` (baseline inline editor that Slice 3b refactors into a dialog), profile page host for "Preferencias fiscales". ADR-017 (Slice-3 technical design — override-preservation template that Slice 3b mirrors for the sell-to-cover track), ADR-018 (Slice-3b technical design / DDL + API shapes + default-sourcing precedence — authored in parallel by solution-architect). |
| v1.5 notes  | Product-owner decisions 2026-04-22, pre-locked for this slice: **(Q-A)** Slice 3b is a net-new slice between Slice 3 and Slice 4, not a Slice-4 fold-in. **(Q-B)** `share_sell_price` is a net-new field on `vesting_events`, distinct from `fmv_at_vest` (the broker's per-share sell price may differ from the FMV used for income recognition). **(Q-C)** Capital-gains basis semantics shift: `basis = fmv_at_vest × net_shares_delivered` when sell-to-cover applied (consumed by Slice 4, not by Slice 3b). **(Q-D)** Tax-withholding-percent default lives **on the user** (`user_tax_preferences.rendimiento_del_trabajo_percent`), not on the grant. Per-grant defaults are a post-v1 concern. **(Q-E)** The dialog edits **all** of: `fmv_at_vest`, `share_sell_price`, `tax_withholding_percent`, `shares_vested_this_event` (per-row), and `vest_date` (past-only). The Slice-3 inline row-editor is removed in favor of the dialog. Additional locked defaults (confirmed via this AC doc, not re-opened): one open `user_tax_preferences` row per user at a time (partial UNIQUE; dual-residency out of scope); `clearOverride: true` reverts both the Slice-3 FMV track AND the sell-to-cover track; `clearSellToCoverOverride: true` reverts only the sell-to-cover track; default-sourcing seeds `tax_withholding_percent` on first sell-to-cover override when `sell_to_cover_enabled = true` and body omits the field (explicit body value wins; null stays null); audit-log action shapes per §3.6; override-preservation mirrors ADR-017's discipline; `share_sell_currency` nullable and defaults to `fmv_currency`. **Sidecar shape** (explicit from Ivan): `user_tax_preferences(user_id, from_date, to_date, country_iso2, rendimiento_del_trabajo_percent NULLABLE, sell_to_cover_enabled BOOLEAN)`. T-shirt **M**. |

This document is implementation-ready. Every AC below is testable as-written. Where a tester needs a specific screen state, the UX reference HTML is cited by filename. ADR-018 authors the DDL, API shapes, and state-machine details that this document intentionally does not duplicate.

## 1. In-scope stories

| Story | In Slice 3b? | Notes |
|-------|--------------|-------|
| **US-001 — Create and manage grants manually** | Already in Slice 1 | Grant-CRUD logic unchanged. The Slice-3 override-exists warning (AC-8.8.1) extends to cover `is_sell_to_cover_override = true` rows — see §5.5. |
| **US-003 — Visualise vesting incl. double-trigger** | Already in Slices 1 + 2 | Cumulative envelope and stacked multi-grant envelope unchanged. Overridden sell-to-cover rows do **not** surface a visual badge on the timeline itself — badge affordance is a Slice-4 polish concern. Tester do-not-flag. |
| **US-004 — Scenario modeler** | **No** (Slice 4) | Referenced as framing only: Slice 3b captures the data that US-004's tax engine consumes. No scenario CTA activates in Slice 3b. |
| **Spec L319/L334 — RSU cap-gains basis amendment** | **Partially** | Slice 3b captures `tax_withholding_percent`, `share_sell_price`, and derives `net_shares_delivered` at read time. Slice 4 is where the basis semantics change actually produces a number. Tester do-not-flag: the paper-gains tile still computes on `fmv × shares_vested_this_event` in Slice 3b (see Slice-3 AC-5.4.1 — unchanged). |
| **US-007 — Modelo 720 threshold alert** | Already in Slice 3 | Banner still reads per Slice-3 AC-6. Securities derivation is unchanged in Slice 3b — it still uses `fmv_at_vest × shares_vested_this_event`, **not** `× net_shares_delivered`. Slice 4 is where M720 securities derivation updates alongside the cap-gains basis amendment. Tester do-not-flag. |
| **US-013 — Sell-now calculator** | **No** (Slice 5) | Referenced only to anchor the NSO sell-to-cover decision: NSO exercise mechanics (`nso_exercises`) live in Slice 5, and sell-to-cover on exercise is part of that surface — not back-ported here. |
| **US-002, US-005, US-006 AC #3, US-008, US-009–012** | No | Later slices. |

No net-new `US-###` stories are introduced in Slice 3b; the slice extends existing US-001/US-003 surfaces and pre-supplies the US-004 tax engine.

## 2. Persona & demo context

- Primary tester persona: **María**, operating post-IPO on the Persona-B spec §2.1. By Slice 3b she has completed the Slice-1/-2/-3 demos and holds a multi-grant portfolio with at least one RSU grant whose employer has IPO'd and whose broker applies sell-to-cover on each vesting event (the default Spanish post-IPO flow for IRPF withholding).
- Device: 14″ laptop (1440×900), Chrome + Safari. Mobile renders but is not the primary acceptance surface; mobile-specific assertions are in §8.
- Locale acceptance: **ES primary**, EN fallback. Every user-visible string passes through the i18n layer.
- Environment: local-only per ADR-015 §0a + v1.1 (`http://localhost:<port>`). No cloud URL is relevant until Slice 9. No new external egress in Slice 3b (the ECB worker from Slice 3 continues unchanged).

## 3. Global ACs (apply to every screen this slice ships)

Slice 3b inherits **all** Slice-1 / Slice-2 / Slice-3 global ACs (G-1 through G-34) without re-litigation. The deltas below extend, tighten, or add new ACs; they do not replace prior wording.

### 3.1 Non-advice disclaimer — footer + rule-set chip

- **G-1..G-7 (inherited).** Footer renders on every new Slice-3b surface: the Profile "Preferencias fiscales" section (existing Profile page — not a net-new page), the per-row vesting-events dialog (renders as a modal over grant-detail; grant-detail's footer remains visible underneath — see §7.1), and the prior-periods history table below the Preferencias fiscales form.
- **G-5-b (no change vs Slice 3).** The rule-set chip renders on pages with an FX-dependent number per Slice-3 AC-7.1.1. Slice 3b introduces **no new FX-dependent surface** — the dialog's derived values panel renders in the grant's native currency (e.g., USD), not in EUR, so the dialog itself carries no chip. The underlying grant-detail page still carries the chip per Slice-3 AC-7.1.1.
- **G-5-c (net-new in Slice 3b).** The Profile "Preferencias fiscales" section does not render a chip (no FX-dependent number is surfaced there — the stored values are a percent and a boolean). Tester do-not-flag if the chip is absent on that section.

### 3.2 Non-advice disclaimer — first-login modal

- **G-8..G-10 (inherited from Slice 1, not re-tested here).** Disclaimer gating is proven in Slice 1; Slice 3b adds no re-acceptance trigger.

### 3.3 i18n

- **G-11 (inherited).** Every Slice-3b string ships in `es-ES` and `en`. CI lint rejects single-locale PRs.
- **G-12 (extended).** Spanish tax terms remain in Spanish even in EN locale. Slice-3b **adds**: `rendimiento del trabajo` (kept as Spanish — this is the specific IRPF term for employment income), `Preferencias fiscales` (kept as Spanish in the Profile section heading — section identity is locale-stable). The industry term `sell-to-cover` is **kept in English** in both locales (no ES translation that is not awkward — the broker-English is what Persona-B users recognize). `rendimiento del trabajo` appears in EN labels with a parenthetical gloss on first render: `rendimiento del trabajo (Spanish withholding on employment income)`.
- **G-13 (extended).** Locale-aware number formatting. `tax_withholding_percent` renders as a percent with up to 4 decimal places (`45,0000 %` ES / `45.0000%` EN; stored as `NUMERIC(5,4)`). `share_sell_price` renders like `fmv_at_vest` with up to 4 decimal places and an explicit currency suffix.
- **G-14 (inherited).** ISO 8601 in storage; user-locale long-form on display. `user_tax_preferences.from_date` and `.to_date` render per G-14; the history table below the Preferencias fiscales form uses short form (`15 abr 2026` / `Apr 15, 2026`).
- **G-15 (inherited).** ES-first label testing. The dialog's derived-values panel labels (`Bruto` / `Gross`, `Acciones vendidas` / `Shares sold`, `Neto entregado` / `Net delivered`, `Retenido en efectivo` / `Cash withheld`) are ~25–30 % longer in ES than EN; test at 14″ desktop and at the 640 px mobile breakpoint for no truncation.

### 3.4 Accessibility (WCAG 2.1 AA / 2.2 AA)

- **G-16..G-22 (inherited).** Every Slice-3b page passes `axe` smoke in CI and a manual keyboard walkthrough.
- **G-23 (extended).** Override state (`is_sell_to_cover_override = true` OR `is_user_override = true`) surfaces via a **label + icon** — never color alone. The chip text reads `Ajustado manualmente` / `Manually adjusted` (same copy as Slice 3); the chip is present on any row carrying either override flag (Slice 3b does not distinguish the two override tracks visually at the row level — both are a single "manually adjusted" signal to the user; the distinction lives in the dialog's revert affordances per §7.5).
- **G-24 (inherited).** `prefers-reduced-motion`: dialog open/close transitions are instant when the preference is set; the derived-values panel reveal is instant.
- **G-25 (inherited).** `prefers-color-scheme: dark` renders the full dark token set for the Profile section and the dialog.
- **G-33 (inherited).** Keyboard navigation on the dialog per §7.1. The inline row editor that Slice 3 AC-33 referenced is **removed** in Slice 3b — Slice-3 AC-33's assertions migrate to the dialog as §7.1 below.
- **G-35 (net-new in Slice 3b — dialog a11y).** The dialog is a **modal** with focus-trap (Tab/Shift-Tab cycle within the dialog; no focusable element outside the dialog receives focus while it is open), dismissible via **Escape** and via an explicit close control, labelled by the dialog heading (`aria-labelledby` points at the heading node), and **returns focus to the triggering row's action button** after close. Screen-reader announcement on open reads the heading plus the vest date being edited (`Editar vesting del 15 de enero de 2026`). The derived-values panel is marked up as a definition list (`<dl>`) so assistive tech can pair labels and values.

### 3.5 GDPR / data-minimization

- **G-26 (extended).** The Slice-1/-2/-3 payload-schema lint extends to Slice-3b analytics events. Specifically: **`tax_withholding_percent` values, `share_sell_price` values, `share_sell_currency` strings, `rendimiento_del_trabajo_percent` values, `country_iso2` values, derived sell-to-cover amounts (gross, shares_sold, net_delivered, cash_withheld), override flags, and override counts** must never appear in an analytics event payload. Analytics payloads may only carry the action verb + surface identifier (e.g., `{ surface: "tax_preferences", verb: "save" }`, `{ surface: "vesting_event_dialog", verb: "open" }`) plus the user UUID.
- **G-27 (inherited).** Analytics opt-in default off.
- **G-28 (no change vs Slice 3).** Slice 3b adds **zero** new external network calls. The ECB worker from Slice 3 continues unchanged (it fetches on its existing schedule; it does not read or write `user_tax_preferences` or the sell-to-cover columns).
- **G-29 (extended — PII-adjacent treatment of tax values).** `tax_withholding_percent`, `rendimiento_del_trabajo_percent`, and `share_sell_price` are treated as **PII-adjacent** in server logs: they MUST be redacted from any structured log line that is written by a request handler. The only place these values may appear is in the database itself (`user_tax_preferences`, `vesting_events`) and in the dialog's HTTP response body consumed by the same authenticated user. A CI lint on the request-handler layer verifies that these three field names never appear inside an `orbit_log::event!` invocation outside an explicit redaction wrapper. The user's tax percentage is not disclosed in error messages surfaced to other users (cross-tenant 404 per §9.3).

### 3.6 Observability + audit log

- **G-30..G-31 (inherited).** Request logs and auth events unchanged.
- **G-32 (extended).** Slice-1/-2/-3 audit-log actions continue to be written. **New Slice-3b audit-log actions**, all conforming to the SEC-101-strict payload allowlist (no raw percentages, no prices, no amounts, no country codes beyond a two-letter enum, no PII):
  - `user_tax_preferences.upsert` — `target_kind = "user_tax_preferences"`, `target_id = <the row id of the newly-inserted (or same-day-updated) row>`; `payload_summary = { outcome: "inserted" | "closed_and_created" | "updated_same_day" }`. Written on every Profile save of the Preferencias fiscales form. **No country code, no percent, no enabled/disabled flag** is written — only the outcome string so an auditor can reconstruct the time-series cadence without reading values.
  - `vesting_event.sell_to_cover_override` — `target_kind = "vesting_event"`, `target_id = vesting_event.id`; `payload_summary = { grant_id: <uuid>, fields_changed: [<"tax_percent" | "sell_price" | "sell_currency" | "shares" | "fmv" | "vest_date">, ...] }`. The `fields_changed` array contains 1..6 symbolic values covering all fields the dialog can edit. Written on every successful dialog save that mutates at least one row field (a save with no actual changes writes no audit row).
  - `vesting_event.clear_sell_to_cover_override` — `target_kind = "vesting_event"`, `target_id = vesting_event.id`; `payload_summary = { grant_id: <uuid> }`. Written on a `clearSellToCoverOverride: true` PUT that reverts only the sell-to-cover track while preserving the FMV track. No `preserved` array is emitted — this action narrowly targets the sell-to-cover fields and the preservation contract is implicit in the action name.
  - Existing `vesting_event.override` and `vesting_event.clear_override` actions from Slice 3 continue to apply when the user edits FMV fields alone or calls `clearOverride: true` per §7.5.1.
- **G-34 (inherited).** ECB worker metric unchanged.

## 4. `user_tax_preferences` sidecar surface (new in Slice 3b)

Reference: ADR-018 authors the DDL (`user_tax_preferences` with the shape pinned in v1.5 notes) and the close-and-create transaction. This section pins the **requirements** the Profile surface must meet.

### 4.1 Profile section placement and identity

- **AC-4.1.1 — section presence.** Given the user opens the Profile page, when the page loads, then a new section `Preferencias fiscales` (ES) / `Tax preferences` (EN) renders **below** the Slice-2 Modelo 720 inputs panel and **above** the Slice-2 Session/device list UI. The section anchor is `#preferencias-fiscales`.
- **AC-4.1.2 — section structure.** The section is laid out as: (a) a short prose block explaining what the form controls ("Orbit usa estos valores para estimar el sell-to-cover de tus RSU y preparar el cálculo fiscal. No cambia tu declaración real." / "Orbit uses these values to estimate the sell-to-cover on your RSUs and prepare the tax computation. It does not change your actual filing."), (b) the editable form (AC-4.2 + AC-4.3), (c) a `Guardar` / `Save` primary CTA, (d) a **history table** of prior closed rows (AC-4.5).
- **AC-4.1.3 — first-render state.** Given the user has never saved a `user_tax_preferences` row, when the section renders, then the form shows: country picker empty (no default — GeoIP is explicitly out of scope), the percent field hidden (AC-4.2.2), and `sell_to_cover_enabled` defaulting to a neutral unchecked state. The history table below reads `Sin historial aún.` / `No history yet.`

### 4.2 Country picker + conditional rendering

- **AC-4.2.1 — country picker.** The country field is an ISO-3166 alpha-2 select. The list of options is bounded to a curated set for v1 (Spain + the set of other EU countries and the UK for paper-design parity — ADR-018 pins the exact list). The control is labeled `País de residencia fiscal` / `Country of tax residence`.
- **AC-4.2.2 — percent field visibility (Spain-like rendering).** Given the country picker resolves to `ES` (and any other country that Slice 3b's curated list flags as "Spain-like" for IRPF-style withholding — ADR-018 pins the exact flag; for v1 this is Spain only), when the country selection changes, then the `rendimiento_del_trabajo_percent` numeric input renders immediately (no submit required for the reveal). For every other country in the list, the field is **hidden** — not merely disabled — and on save the persisted `rendimiento_del_trabajo_percent` is `NULL`.
- **AC-4.2.3 — percent input validation.** Given the percent field is visible and the user enters a value, when they submit, then the value must be a number in `[0, 100]` with up to 4 decimal places. Out-of-range rejects inline: `Introduce un valor entre 0 y 100.` / `Enter a value between 0 and 100.` The value is stored as a fraction (`NUMERIC(5,4)`; e.g. the input `45` persists as `0.4500`); ADR-018 pins the exact conversion direction. Blank is allowed — stored as `NULL`.
- **AC-4.2.4 — percent field label copy.** The percent input carries the label `Rendimiento del trabajo (%)` in both locales (Spanish-only phrasing per G-12). Helper copy below reads: `Tu retención aproximada de IRPF sobre ingresos del trabajo. Orbit la aplica por defecto al sell-to-cover de nuevos vestings.` (ES) / `Your approximate Spanish IRPF withholding rate on employment income. Orbit applies it by default to the sell-to-cover on new vests.` (EN).

### 4.3 `sell_to_cover_enabled` toggle

- **AC-4.3.1 — toggle presence.** The `sell_to_cover_enabled` control renders as a boolean toggle (checkbox or switch — UX-designer owns the visual treatment). Label: `Aplicar sell-to-cover por defecto` / `Apply sell-to-cover by default`. Helper copy: `Si tu broker vende acciones para cubrir la retención en cada vesting, deja esto activado.` (ES) / `If your broker sells shares to cover withholding on each vest, leave this on.` (EN).
- **AC-4.3.2 — default on country switch to Spain.** Given the user changes the country picker to `ES` **from a prior non-Spain country (or from blank on a first save)**, when the change propagates client-side, then the toggle pre-checks to `true`. The user may un-check before saving; the server respects whatever the client submits.
- **AC-4.3.3 — default on country switch to non-Spain.** Given the user changes the country picker to any non-Spain value, when the change propagates client-side, then the toggle pre-uncheks to `false`. The user may re-check before saving; the server respects whatever the client submits.
- **AC-4.3.4 — persist independently of percent.** The toggle persists regardless of whether `rendimiento_del_trabajo_percent` is visible or NULL. A user on country `PT` with toggle checked saves `sell_to_cover_enabled = true` and `rendimiento_del_trabajo_percent = NULL`; the dialog's default-sourcing (§7.6) then has no percent to seed and leaves `tax_withholding_percent` NULL unless the user enters one explicitly.

### 4.4 Save path: close-and-create + idempotent same-day save

- **AC-4.4.1 — close-and-create.** Given the user saves the Preferencias fiscales form and there is an existing open row (`to_date IS NULL`), when the server processes the save, then the prior row is closed (`to_date = <today, Europe/Madrid>`) and a new row is inserted with `from_date = <today>`, `to_date = NULL`, and the submitted values. The two writes occur in one transaction; a failure rolls both back. Audit writes one `user_tax_preferences.upsert` row with `outcome: "closed_and_created"`.
- **AC-4.4.2 — first-ever save.** Given no prior `user_tax_preferences` row exists for this user, when the user saves, then one row is inserted with `from_date = <today>`, `to_date = NULL`, and the submitted values. Audit writes `outcome: "inserted"`.
- **AC-4.4.3 — idempotent same-day save (M720 pattern).** Given the prior open row's `from_date` equals today and the user saves again on the same day, when the server processes the save, then it **updates the existing open row in place** (overwriting `country_iso2`, `rendimiento_del_trabajo_percent`, `sell_to_cover_enabled`) rather than closing-and-creating. No new row is inserted; no prior row is closed. Audit writes `outcome: "updated_same_day"`. Rationale: a user who mistypes the percent and re-saves five minutes later should not produce five closed zero-length rows in the history table.
- **AC-4.4.4 — one-open-row invariant.** The schema enforces at most one open row per user via a **partial UNIQUE index** on `(user_id) WHERE to_date IS NULL` (ADR-018 pins the DDL). A save that violates this invariant returns 500 with a generic "no se pudo guardar" surface (indicates a server bug; the client path cannot produce this).
- **AC-4.4.5 — cross-tenant 404.** A `PUT` or `POST` on `/api/v1/user-tax-preferences/*` from a user whose RLS scope does not match the target row returns 404, not 403 (parity with Slice-1/-2/-3 cross-tenant semantics).

### 4.5 History table (prior periods)

- **AC-4.5.1 — table presence.** Given the user has at least one closed row in `user_tax_preferences` (`to_date IS NOT NULL`), when the Profile renders, then a **history table** below the editable form lists all closed rows sorted descending by `from_date`. Columns: `Desde` (from_date), `Hasta` (to_date), `País` (country_iso2), `Rendimiento del trabajo` (percent — blank for non-Spain-like), `Sell-to-cover` (checkmark / dash). The current open row is **not** listed in the history table — it lives in the form above.
- **AC-4.5.2 — locale formatting.** Dates render per G-14 short form. Percent renders per G-13. The country code renders as an ISO-3166 alpha-2 with a tooltip carrying the full country name in the current locale.
- **AC-4.5.3 — read-only.** Rows in the history table are **not editable** in Slice 3b. Retroactive correction of a prior period's values is out of scope. If the user needs to correct a historical period, the only path is via admin intervention (not exposed in Slice 3b). Tester do-not-flag — this is the deliberate v1 scope.
- **AC-4.5.4 — empty state.** Given the user has saved the form at least once but has never changed country or percent (so no row is closed), when the Profile renders, then the history table renders its empty-state copy: `Sin historial anterior.` / `No prior history.`

### 4.6 Audit-log payload allowlist

- **AC-4.6.1 — `user_tax_preferences.upsert` payload.** Payload keys: `{ outcome: "inserted" | "closed_and_created" | "updated_same_day" }`. No other keys permitted. **No country code, no percent, no enabled flag** is written — only the outcome. CI lint on the audit-writer enforces the allowlist.
- **AC-4.6.2 — `target_id` carries the newly-written row.** `target_id` is the `user_tax_preferences.id` of the row that ended the transaction as the open row (i.e., the newly-inserted row for `inserted` and `closed_and_created`; the updated row for `updated_same_day`).

## 5. ALTER `vesting_events` — schema additions and invariants

Reference: ADR-018 authors the DDL. This section pins the **requirements** the schema must enforce.

### 5.1 Column additions

- **AC-5.1.1 — net-new columns.** The `vesting_events` table gains the following columns, all nullable, all defaulting to `NULL` on existing rows (no data migration back-fills): `tax_withholding_percent NUMERIC(5,4)`, `share_sell_price NUMERIC(20,6)`, `share_sell_currency TEXT`, `is_sell_to_cover_override BOOLEAN NOT NULL DEFAULT false`, `sell_to_cover_overridden_at TIMESTAMPTZ`.
- **AC-5.1.2 — no retroactive back-fill.** Existing Slice-1/-2/-3 `vesting_events` rows carry `tax_withholding_percent IS NULL` (meaning "no sell-to-cover has been recorded for this vest") until the user explicitly edits them via the dialog. The migration does **not** seed percent values from `user_tax_preferences` retroactively. Tester do-not-flag — this is the deliberate v1 scope.

### 5.2 CHECK coherence

- **AC-5.2.1 — all-or-none on the sell-to-cover triplet.** A CHECK constraint enforces that `(tax_withholding_percent, share_sell_price, share_sell_currency)` are either **all NULL** or **all NON-NULL** simultaneously. A row with a percent but no sell price (or vice-versa) is rejected at the database level with a 422 surfaced on the API. The error envelope conforms to Slice-2 AC-10.4.
- **AC-5.2.2 — override-flag coherence.** A CHECK constraint enforces `is_sell_to_cover_override = (sell_to_cover_overridden_at IS NOT NULL)` (mirrors Slice-3 ADR-017's `override_flag_coherent` pattern).
- **AC-5.2.3 — tax percent bounds.** A CHECK constraint enforces `tax_withholding_percent IS NULL OR (tax_withholding_percent >= 0 AND tax_withholding_percent <= 1)` (since the column stores a fraction per AC-4.2.3).
- **AC-5.2.4 — share sell price bounds.** A CHECK constraint enforces `share_sell_price IS NULL OR share_sell_price > 0`.
- **AC-5.2.5 — sell currency whitelist.** A CHECK constraint enforces `share_sell_currency IS NULL OR share_sell_currency IN ('USD', 'EUR', 'GBP')` (same set as Slice-2 AC-4.2.6 and Slice-3 AC-8.2.4).

### 5.3 Override preservation on grant re-derivation

- **AC-5.3.1 — Slice-3 FMV-override preservation continues.** The Slice-3 rule (AC-8.4.2) that `is_user_override = true` rows survive grant-edit re-derivation is unchanged.
- **AC-5.3.2 — sell-to-cover-override preservation.** The same discipline extends: `is_sell_to_cover_override = true` rows are **preserved in place** during a grant re-derivation triggered by a grant-edit of `vesting_start`, `vesting_total_months`, `cliff_months`, or `vesting_cadence`. Their `tax_withholding_percent`, `share_sell_price`, and `share_sell_currency` are left untouched. A row may carry **either, both, or neither** override flag; preservation logic treats each flag independently (a row with `is_user_override = false` AND `is_sell_to_cover_override = true` preserves the sell-to-cover fields but lets the derivation regenerate `vest_date` / `shares_vested_this_event` / `fmv_at_vest`).
- **AC-5.3.3 — rebuild vs. preserve arbitration.** Given a grant with N events, M carrying `is_user_override = true` OR `is_sell_to_cover_override = true`, when the grant is edited, then the algorithm produces a new candidate event list; rows preserved by either override flag are retained; the non-preserved rows are replaced by the candidate output. If the candidate list has fewer events than non-preserved rows, excess non-preserved rows are deleted (parity with Slice-3 AC-8.4.4).

### 5.4 Cumulative invariant

- **AC-5.4.1 — invariant parity with Slice 3.** Slice-3 AC-8.5 (relaxation of the `SUM(shares_vested_this_event) = share_count` invariant on any override) continues to apply. Slice 3b does not tighten or relax beyond Slice 3.
- **AC-5.4.2 — UI signal unchanged.** The "Esta curva incluye ajustes manuales..." note (Slice-3 AC-8.5.3) continues to render under the same trigger conditions; its presence does not distinguish between FMV-track and sell-to-cover-track overrides (both contribute to the "at least one override exists" trigger).

### 5.5 Grant-edit override-exists warning banner

- **AC-5.5.1 — banner extends to sell-to-cover overrides.** Slice-3 AC-8.8.1's banner (`Este grant tiene {N} vesting(s) ajustado(s) manualmente...`) continues to render. In Slice 3b, `{N}` counts rows where **either** `is_user_override = true` **OR** `is_sell_to_cover_override = true` (a row with both flags counts once). Copy does **not** distinguish the two override tracks — a "manually adjusted" row is a "manually adjusted" row regardless of which track produced it.

## 6. `orbit_core::sell_to_cover::compute` pure function (new in Slice 3b)

Reference: ADR-018 authors the function signature and the TS parity mirror path. This section pins the **behavioral requirements** the implementation must meet.

### 6.1 Inputs and outputs

- **AC-6.1.1 — function signature.** `orbit_core::sell_to_cover::compute` accepts `(fmv_at_vest: Money, shares_vested: u32, tax_percent: Decimal, share_sell_price: Money)` and returns a `SellToCoverResult { gross_amount: Money, shares_sold_for_taxes: Decimal, net_shares_delivered: Decimal, cash_withheld: Money }`. ADR-018 pins the exact Rust type identifiers; the requirement here is that the function is **pure**: no I/O, no clock read, no database.
- **AC-6.1.2 — currency handling.** If `fmv_at_vest.currency` and `share_sell_price.currency` differ, the function returns a typed error (ADR-018 names the error variant). Slice 3b does **not** bake in FX conversion between FMV and sell price — both are expected to be in the same broker-native currency (typically USD). The dialog UI enforces this upstream (AC-7.3.4).
- **AC-6.1.3 — no value stored.** The function is computed **at read time** in the dialog and in any downstream consumer. `gross_amount`, `shares_sold_for_taxes`, `net_shares_delivered`, and `cash_withheld` are **not** persisted on `vesting_events`. If the user edits a field, the stored columns change; the derived fields recompute on next render.

### 6.2 Derived-value formulas

- **AC-6.2.1 — gross_amount.** `gross_amount = fmv_at_vest × shares_vested`, in `fmv_at_vest.currency`. This is the income-recognition basis (unchanged from Slice 3's RSU basis; Slice 3b does not alter it).
- **AC-6.2.2 — cash_withheld.** `cash_withheld = gross_amount × tax_percent`, in the same currency.
- **AC-6.2.3 — shares_sold_for_taxes.** `shares_sold_for_taxes = cash_withheld / share_sell_price`, rounded **up** to 4 decimal places (the broker sells at least enough shares to cover the withholding; fractional shares are modeled as a decimal in v1 rather than as an integer-round-up — ADR-018 pins the rounding direction and precision). The currency of the numerator and denominator match per AC-6.1.2; the quotient is a unit-less share count.
- **AC-6.2.4 — net_shares_delivered.** `net_shares_delivered = shares_vested − shares_sold_for_taxes`. This is the field that Slice 4's cap-gains basis will multiply by `fmv_at_vest` per the v1.5 Q-C amendment; Slice 3b captures it but does not consume it.

### 6.3 Rounding rules

- **AC-6.3.1 — scaled shares to 4 decimal places.** `shares_sold_for_taxes` and `net_shares_delivered` are each returned with up to 4 decimal places of precision. The rounding direction for `shares_sold_for_taxes` is **up** (ceiling); the rounding for `net_shares_delivered` falls out as `shares_vested − <rounded-up shares_sold>`, which may produce up to 4 decimals of trailing non-zero digits. ADR-018 pins the exact `Decimal` type used.
- **AC-6.3.2 — cash_withheld to 2 decimal places.** `cash_withheld` is returned rounded to **2 decimal places** (the broker's settlement currency granularity — cents / céntimos). Rounding mode is **banker's rounding** (half-to-even) to stay consistent with the `Money` type's canonical display rule — ADR-018 pins.
- **AC-6.3.3 — gross_amount to full precision.** `gross_amount` is returned at the full `NUMERIC(20,6)` precision of `fmv_at_vest`; no truncation. The dialog display rounds to 2 decimals for the UI, but the underlying value is 6 decimals.

### 6.4 Edge cases

- **AC-6.4.1 — `tax_percent = 0`.** When `tax_percent` is exactly `0`, the function returns `cash_withheld = 0`, `shares_sold_for_taxes = 0`, `net_shares_delivered = shares_vested`. No rounding surprises.
- **AC-6.4.2 — `tax_percent = 1`.** When `tax_percent` is exactly `1` (i.e., `1.0000`), the function returns `cash_withheld = gross_amount`, `shares_sold_for_taxes = gross_amount / share_sell_price` (rounded up to 4 dp), `net_shares_delivered = shares_vested − shares_sold_for_taxes`. If `share_sell_price = fmv_at_vest`, `shares_sold_for_taxes` equals `shares_vested` and `net_shares_delivered = 0`. If `share_sell_price > fmv_at_vest`, `shares_sold_for_taxes < shares_vested` and `net_shares_delivered > 0`. If `share_sell_price < fmv_at_vest`, `shares_sold_for_taxes > shares_vested` and `net_shares_delivered < 0` — ADR-018 pins whether this returns an error or a negative decimal. The **requirement** here is that the function does not silently clamp; the test must observe the documented behaviour.
- **AC-6.4.3 — fractional `share_sell_price`.** When `share_sell_price` is non-integer (e.g. `$42.2537`), the division in AC-6.2.3 proceeds to full `Decimal` precision and rounds up only at the 4-dp boundary. No early truncation.
- **AC-6.4.4 — `shares_vested = 0`.** Defensive: if `shares_vested = 0`, the function returns all-zero outputs without computing a division by zero (the `shares_sold_for_taxes` formula's numerator is `gross_amount = 0`, so the division is `0 / share_sell_price = 0`). No error.
- **AC-6.4.5 — `fmv_at_vest = 0`.** Defensive: if `fmv_at_vest = 0`, `gross_amount = 0`, `cash_withheld = 0`, `shares_sold_for_taxes = 0`, `net_shares_delivered = shares_vested`. No error.

### 6.5 TS parity mirror + shared fixture

- **AC-6.5.1 — TS mirror path.** A parallel TypeScript implementation lives at `frontend/src/lib/sellToCover.ts` (ADR-018 pins the exact path). Its signature mirrors the Rust version; its behavior matches across every fixture case per AC-6.5.3.
- **AC-6.5.2 — shared fixture.** A JSON fixture at `shared/fixtures/sell_to_cover_cases.json` (ADR-018 pins the exact path) lists ≥12 cases covering: (a) baseline `45% / $42 FMV / $42.25 sell price / 100 shares` — the demo-acceptance headline case; (b) `tax_percent = 0`; (c) `tax_percent = 1` with `sell_price = fmv`; (d) `tax_percent = 1` with `sell_price > fmv`; (e) fractional sell price (AC-6.4.3); (f) `shares_vested = 0`; (g) `fmv_at_vest = 0`; (h) very large `shares_vested` (e.g., 100,000) to exercise precision; (i) very small `tax_percent` (e.g., `0.0001`); (j) very small `share_sell_price` (e.g., `$0.0001`); (k) `shares_vested = 1` with `tax_percent = 0.5` (fractional share scenario); (l) a case where `shares_sold_for_taxes` would round up from `42.00001` to `42.0001`.
- **AC-6.5.3 — parity CI gate.** CI runs both the Rust test suite and the TS test suite against the shared fixture. A divergence on any case fails CI with a clear diff pointing at the specific numeric delta. Parity is exact at 4 dp on shares and 2 dp on cash_withheld; intermediate precision may differ between the two implementations but final outputs MUST match.

## 7. Vesting-events dialog surface (new in Slice 3b)

Reference: `docs/design/screens/grant-detail-vesting-editor.html` (Slice-3 baseline inline editor — **replaced by the dialog in Slice 3b**). ADR-018 authors the exact DOM structure and transition states. This section pins the **behavioral requirements**.

### 7.1 Dialog open / close / focus

- **AC-7.1.1 — open affordance.** Given the user is on grant-detail and the "Precios de vesting" section is visible, when the user clicks a row's edit action (keyboard-Enter on focused row, or click on a row-level edit button), then the per-row dialog opens as a modal over the grant-detail page. The triggering row's action button receives `aria-expanded=true` for the duration of the dialog session.
- **AC-7.1.2 — dialog heading.** The dialog's heading reads `Editar vesting del {vest_date}` (ES) / `Edit vesting on {vest_date}` (EN), with `{vest_date}` rendered per G-14 long form.
- **AC-7.1.3 — close affordances.** The dialog closes via: (a) Escape key, (b) explicit close control (×) in the dialog chrome, (c) Cancel button in the dialog footer, (d) Save button after a successful save. Clicking the backdrop **also** closes the dialog if and only if there are no unsaved changes; if unsaved changes exist, the backdrop click triggers the unsaved-changes confirmation per §9.5.
- **AC-7.1.4 — focus-trap + return-focus (G-35).** While the dialog is open, Tab/Shift-Tab cycles focus among the dialog's focusable elements only; no element in the page behind receives focus. On close, focus returns to the row's action button that triggered the dialog.
- **AC-7.1.5 — slice-3 inline editor removed.** The inline row-editor surface described in Slice-3 AC-8.2 / AC-8.3 / G-33 is **removed** in Slice 3b. The same editable fields are reached exclusively via the dialog. **This is intentional and NOT a Slice-3 AC-8 regression** — see §11 tester do-not-flag list.

### 7.2 Derived-values panel layout

- **AC-7.2.1 — panel presence.** Given the dialog is open, when it renders, then it shows a derived-values panel at the top of the dialog body with four rows: `Bruto` / `Gross`, `Acciones vendidas` / `Shares sold`, `Neto entregado` / `Net delivered`, `Retenido en efectivo` / `Cash withheld`. Each row shows the computed value per §6 applied to the row's current field values (pre-edit) + any uncommitted field edits (live recompute as the user types).
- **AC-7.2.2 — pre-data state.** Given the row has `tax_withholding_percent IS NULL` OR `share_sell_price IS NULL` OR `fmv_at_vest IS NULL`, when the panel renders, then each derived row shows a placeholder dash (`—`) and a muted helper line: `Completa FMV, precio de venta y % de retención para ver los valores derivados.` (ES) / `Fill in FMV, sell price, and withholding % to see derived values.` (EN). The panel never renders `0` to mean "unknown" — the dash is the unambiguous placeholder.
- **AC-7.2.3 — live recompute.** Given all three inputs are non-NULL (either already persisted or typed in the current edit session), when the user edits any of `fmv_at_vest`, `tax_withholding_percent`, `share_sell_price`, or `shares_vested_this_event`, then the derived-values panel recomputes on-change (debounced ≤ 200 ms). No submit required.
- **AC-7.2.4 — currency suffix on amounts.** `Bruto`, `Retenido en efectivo` render with the currency suffix matching `fmv_at_vest.currency` (e.g., `$4 200,00 USD`). `Acciones vendidas`, `Neto entregado` render as decimal counts without currency suffix (e.g., `0,9936` or `99,0064`).
- **AC-7.2.5 — no EUR conversion in the panel.** The panel renders in the grant's native currency only. Slice 3b does **not** add EUR conversion inside the dialog; the Slice-3 paper-gains tile on the dashboard remains the only EUR-facing surface.

### 7.3 Editable fields (Q-E scope)

- **AC-7.3.1 — editable field set.** The dialog exposes five editable fields, arranged below the derived-values panel: (1) `FMV por acción` with a currency picker (same set as Slice-2 AC-4.2.6), (2) `Precio de venta por acción` / `Share sell price` with a currency picker, (3) `% de retención` / `Withholding %`, (4) `Acciones en este vesting` / `Shares vested this event` (integer), (5) `Fecha de vesting` / `Vesting date` (past-only — future rows show this as a disabled read-only value, same logic as Slice-3 AC-8.3.1).
- **AC-7.3.2 — past-row full editability.** Given the row's `vest_date ≤ today` at dialog open, when the user edits, then all five fields are editable per their validation rules. Validation rules for FMV / vest_date / shares inherit from Slice-3 AC-8.2.2..4 verbatim. Validation for `tax_withholding_percent` per AC-4.2.3. Validation for `share_sell_price` per AC-5.2.4.
- **AC-7.3.3 — future-row restriction.** Given the row's `vest_date > today` at dialog open, when the dialog renders, then `vest_date` and `shares_vested_this_event` are **read-only** (parity with Slice-3 AC-8.3.1). The three remaining fields (FMV, sell price, tax %) are editable. Helper tooltip on the disabled fields: `Solo los vestings pasados pueden cambiar fecha o acciones.` (same copy as Slice-3 AC-8.3.1).
- **AC-7.3.4 — `share_sell_currency` defaulting.** Given the user edits `share_sell_price` without explicitly picking a currency, when the dialog save submits, then `share_sell_currency` defaults to the row's `fmv_currency` (ADR-018 pins the server-side default; the client sends `null` and the server writes `fmv_currency`). If `fmv_currency` is also NULL (no FMV entered yet), the server returns 422: `Introduce también el FMV y su moneda antes de guardar el precio de venta.` / `Enter the FMV and its currency before saving the sell price.`
- **AC-7.3.5 — tax-percent placeholder copy.** The `% de retención` input's placeholder reads the user's active `user_tax_preferences.rendimiento_del_trabajo_percent` when the user has `sell_to_cover_enabled = true` (e.g., `p.ej. 45,0000 %` / `e.g. 45.0000%`). When the user has `sell_to_cover_enabled = false` or no active tax preferences, the placeholder reads `—`. The placeholder is visual only — it does NOT pre-fill the value; the default-sourcing (§7.6) happens server-side on the first save, not client-side in the input.
- **AC-7.3.6 — field submission order.** Tab order: FMV → FMV currency → share sell price → sell currency → tax % → shares → vest_date → Cancel → Revert (narrow) → Revert (full) → Save. Enter submits the form from any field; Escape cancels.

### 7.4 OCC behaviour

- **AC-7.4.1 — expectedUpdatedAt on save.** Given the user opens the dialog, the client captures the row's `updated_at` at open-time (ADR-018 pins whether this is in the body, a header, or an ETag). On Save, the client sends it back; the server performs the optimistic-concurrency check per Slice-3 AC-10.5. Requirement: **first save wins**; the second returns 409.
- **AC-7.4.2 — 409 surfacing.** Given the server returns 409, when the dialog renders the error, then a banner appears inside the dialog body (above the derived-values panel): `Este vesting se editó en otra pestaña o dispositivo. Refresca para ver los valores actuales.` (ES) / `This vest was edited in another tab or device. Refresh to see the current values.` (EN) with a primary action `Recargar` / `Reload`. Clicking reload closes the dialog (preserving focus return per AC-7.1.4), re-renders the row from the server's current state, and leaves the user to re-open the dialog if they still want to edit.
- **AC-7.4.3 — no partial save on 409.** The 409 guarantees no field was written. No audit row is emitted.
- **AC-7.4.4 — concurrent-edit across tracks.** Given Tab A has the dialog open and Tab B has saved an FMV-only edit (Slice-3 track) via a separate Slice-3 surface (if still reachable — it isn't in Slice 3b, but a direct API call is conceivable), when Tab A saves a sell-to-cover edit, then the OCC check fires regardless of which track produced the concurrent edit. A single `updated_at` gates both tracks.

### 7.5 Revert affordances (two-button model)

- **AC-7.5.1 — `Revertir todos los ajustes` (full clear) button.** Given the row has **either** `is_user_override = true` **OR** `is_sell_to_cover_override = true`, when the dialog renders, then a `Revertir todos los ajustes` / `Revert all adjustments` secondary action is present in the dialog footer. Clicking it sends a PUT with `clearOverride: true` (Slice-3 semantics, extended). The server (a) reverts `vest_date` and `shares_vested_this_event` to the derivation algorithm's current output; (b) **clears** `fmv_at_vest`, `fmv_currency`, `tax_withholding_percent`, `share_sell_price`, `share_sell_currency`; (c) sets `is_user_override = false` and `is_sell_to_cover_override = false`; (d) sets `overridden_at = NULL` and `sell_to_cover_overridden_at = NULL`. Both `vesting_event.clear_override` (Slice 3) and `vesting_event.clear_sell_to_cover_override` (Slice 3b) audit rows are written — one for each track. Rationale: `clearOverride: true` is the "nuclear" revert and signals the user intends to abandon all their manual work on this row.
- **AC-7.5.2 — `Revertir solo sell-to-cover` (narrow clear) button.** Given the row has `is_sell_to_cover_override = true`, when the dialog renders, then a `Revertir solo sell-to-cover` / `Revert sell-to-cover only` tertiary action is present in the dialog footer. Clicking it sends a PUT with `clearSellToCoverOverride: true`. The server (a) **clears only** `tax_withholding_percent`, `share_sell_price`, `share_sell_currency`; (b) **preserves** `fmv_at_vest`, `fmv_currency`, `is_user_override`, `overridden_at`, `vest_date`, `shares_vested_this_event`; (c) sets `is_sell_to_cover_override = false` and `sell_to_cover_overridden_at = NULL`. One `vesting_event.clear_sell_to_cover_override` audit row is written (no `vesting_event.clear_override`).
- **AC-7.5.3 — button visibility matrix.** Given a row's override state, the buttons render per this matrix:
  - `is_user_override = false` AND `is_sell_to_cover_override = false` → neither button renders.
  - `is_user_override = true` AND `is_sell_to_cover_override = false` → only `Revertir todos los ajustes` renders (equivalent to Slice-3 AC-8.7).
  - `is_user_override = false` AND `is_sell_to_cover_override = true` → both buttons render; the narrow clear is functionally equivalent to the full clear in this case but is the safer default mentally.
  - `is_user_override = true` AND `is_sell_to_cover_override = true` → both buttons render.
- **AC-7.5.4 — confirmation copy (full clear).** Clicking `Revertir todos los ajustes` opens a confirmation: `Se revertirán la fecha, las acciones, el FMV, el precio de venta y el % de retención al cálculo automático. Esta acción no se puede deshacer.` (ES) / `Date, shares, FMV, sell price, and withholding % will revert to the automatic calculation. This cannot be undone.` (EN).
- **AC-7.5.5 — confirmation copy (narrow clear).** Clicking `Revertir solo sell-to-cover` opens a confirmation: `Se revertirán el precio de venta y el % de retención. El FMV y los otros valores se conservan.` (ES) / `Sell price and withholding % will revert. FMV and other values are preserved.` (EN).
- **AC-7.5.6 — no idempotent-click audit.** Given the row has no override to clear and the server receives a `clearOverride: true` or `clearSellToCoverOverride: true`, when the request completes, then no audit row is written. (The button should not be visible per AC-7.5.3, but a direct API caller must observe no-audit-on-no-op.)

### 7.6 Default-sourcing of `tax_withholding_percent`

- **AC-7.6.1 — trigger condition.** Given the user submits a dialog save where **all** of the following hold: (a) the row's current `is_sell_to_cover_override = false` (i.e., no prior sell-to-cover on this row), (b) the body contains `share_sell_price` (non-NULL — the user is creating a sell-to-cover override, not just editing FMV), (c) the body **omits** `tax_withholding_percent` (the body key is absent or explicitly `null`), (d) the user has an active `user_tax_preferences` row with `sell_to_cover_enabled = true` and `rendimiento_del_trabajo_percent IS NOT NULL`, then the server **seeds** `tax_withholding_percent` from `user_tax_preferences.rendimiento_del_trabajo_percent` and writes the row accordingly.
- **AC-7.6.2 — explicit body value wins.** Given the body carries `tax_withholding_percent` (non-null), when the save processes, then the body value is used verbatim. The user's tax preferences are not consulted.
- **AC-7.6.3 — explicit null stays null.** Given the body carries `tax_withholding_percent: null` explicitly (as opposed to omitting the key), ADR-018 pins whether this is treated the same as "omit" (trigger default-sourcing) or as "user-specified null" (no default). For this AC doc, the **requirement** is that the behaviour is consistent with the OpenAPI schema: if the schema treats `null` as distinct from "omit", then null stays null; otherwise null triggers default-sourcing. The dialog UI submits `null` only when the user explicitly clears the field; omission is produced only by a raw API caller that omits the key.
- **AC-7.6.4 — `sell_to_cover_enabled = false` suppresses default-sourcing.** Given condition (d) above fails because the user's tax prefs have `sell_to_cover_enabled = false`, when the save processes, then no default-sourcing occurs and `tax_withholding_percent` stays NULL (if the body also omits it). The user has explicitly said "don't apply sell-to-cover by default"; the server respects that even when the user is editing sell-to-cover fields in the dialog (they must type the percent explicitly in that case).
- **AC-7.6.5 — no-active-tax-preferences suppresses default-sourcing.** Given the user has zero rows in `user_tax_preferences` (never saved the Profile section), when the save processes with conditions (a)–(c), then no default-sourcing occurs and `tax_withholding_percent` stays NULL. The CHECK in AC-5.2.1 then fires because the sell-to-cover triplet is not all-none — the server returns 422 with copy: `Introduce un % de retención o configura tus Preferencias fiscales en tu perfil.` / `Enter a withholding % or configure your tax preferences in your profile.` The dialog renders the error per AC-9.1.
- **AC-7.6.6 — one-shot semantics.** Default-sourcing fires only **on the first override** (condition (a)). On subsequent edits of a row that already has `is_sell_to_cover_override = true`, the server does not re-seed `tax_withholding_percent` — the user's prior value wins. This prevents a Profile-side percent change from silently mutating historical vest rows.
- **AC-7.6.7 — no retroactive back-fill on Profile save.** Saving a new `user_tax_preferences` row does **not** walk existing `vesting_events` rows and seed their percents. Default-sourcing happens only at the per-row dialog save path. Tester do-not-flag.

### 7.7 Audit-log payload allowlist (consolidated for the dialog)

- **AC-7.7.1 — `vesting_event.sell_to_cover_override` payload.** Payload keys: `{ grant_id: <uuid>, fields_changed: [<"tax_percent" | "sell_price" | "sell_currency" | "shares" | "fmv" | "vest_date">, ...] }`. No other keys permitted. `fields_changed` MUST contain at least one element.
- **AC-7.7.2 — `vesting_event.clear_sell_to_cover_override` payload.** Payload keys: `{ grant_id: <uuid> }`. No other keys permitted.
- **AC-7.7.3 — dual-write on full clear.** Given a `clearOverride: true` path per AC-7.5.1 that reverts both tracks simultaneously, when the server writes audits, then it writes **both** `vesting_event.clear_override` (Slice 3 shape — `{ grant_id, cleared_fields, preserved }`) **and** `vesting_event.clear_sell_to_cover_override` (Slice 3b shape — `{ grant_id }`), in that order, in the same transaction as the row update.
- **AC-7.7.4 — never present in any payload.** `tax_withholding_percent` values (old or new), `share_sell_price` values, `share_sell_currency` strings, raw FMV values, old share counts, new share counts, old vest dates, new vest dates, derived gross / shares_sold / net_delivered / cash_withheld values, employer names, ticker symbols, any locale-string copy. CI lint enforces.

### 7.8 Interaction with the Slice-3 FMV-only inline editing flow

- **AC-7.8.1 — removal of the inline editor.** The Slice-3 inline row-editor (Slice-3 AC-8.2 / AC-8.3 / G-33) is **removed** in Slice 3b. The same editable fields plus the new Slice-3b fields live exclusively in the dialog.
- **AC-7.8.2 — tester do-not-flag: Slice-3 AC-8.2 regression.** A tester who compares the Slice-3 grant-detail UX to the Slice-3b grant-detail UX will observe the inline editor is gone. This is the deliberate Slice-3b scope; the editor moved into the dialog, it did not disappear. See §11.
- **AC-7.8.3 — Slice-3 endpoints preserved.** The underlying `PUT /api/v1/grants/:gid/vesting-events/:eid` endpoint continues to accept FMV-only edits (a body with only FMV fields). A client that never sets sell-to-cover fields remains a valid Slice-3 user; the server does not require sell-to-cover fields on every PUT.

## 8. Extended `PUT /api/v1/grants/:gid/vesting-events/:eid` handler behaviour

Reference: ADR-018 pins the exact request/response schema. This section pins the **behavioral requirements** and the set of fields accepted.

### 8.1 Accepted body fields (additive over Slice 3)

- **AC-8.1.1 — field set.** The handler accepts the Slice-3 body fields (`vest_date`, `shares_vested_this_event`, `fmv_at_vest`, `fmv_currency`, `clearOverride`) plus the Slice-3b additions: `tax_withholding_percent` (nullable, or omitted), `share_sell_price` (nullable, or omitted), `share_sell_currency` (nullable, or omitted), `clearSellToCoverOverride` (boolean, defaults false). A body mixing `clearOverride: true` with sell-to-cover fields is rejected 422: `No combines revertir todo con nuevos valores.` / `Do not combine full-revert with new values.` A body mixing `clearSellToCoverOverride: true` with `tax_withholding_percent` / `share_sell_price` / `share_sell_currency` is rejected 422 with the same copy pattern scoped to the narrow clear.

### 8.2 Override-flag transitions

- **AC-8.2.1 — sell-to-cover-override flag set on first sell-to-cover write.** Given the prior row has `is_sell_to_cover_override = false`, when the handler writes any of `tax_withholding_percent`, `share_sell_price`, `share_sell_currency` (either from the body or via default-sourcing), then `is_sell_to_cover_override = true` and `sell_to_cover_overridden_at = now()`.
- **AC-8.2.2 — flag stays true on subsequent edits.** Given the prior row has `is_sell_to_cover_override = true`, when the handler writes any sell-to-cover field, then `is_sell_to_cover_override` stays `true` and `sell_to_cover_overridden_at = now()` (refreshed).
- **AC-8.2.3 — flag cleared only via narrow-clear or full-clear.** `is_sell_to_cover_override` transitions from `true → false` only via `clearSellToCoverOverride: true` (§7.5.2) or `clearOverride: true` (§7.5.1). It never transitions automatically.

### 8.3 OCC + cross-tenant

- **AC-8.3.1 — OCC on every mutating path.** The OCC check per §7.4 applies to every mutating path: new sell-to-cover write, FMV-only edit (Slice 3 path), `clearOverride: true`, `clearSellToCoverOverride: true`.
- **AC-8.3.2 — cross-tenant 404.** A `PUT` from a user whose RLS scope does not match the target returns 404 (parity with Slice-3 AC-10.3).

### 8.4 Audit-row emission sequencing

- **AC-8.4.1 — one audit row per track per request.** On a single PUT that mutates both tracks (e.g., the user edits FMV and tax percent in one dialog save), the handler writes at most **one** `vesting_event.override` audit row (Slice 3; fires if any of `vest_date`, `shares`, `fmv` changed) AND at most **one** `vesting_event.sell_to_cover_override` audit row (Slice 3b; fires if any of `tax_percent`, `sell_price`, `sell_currency`, `shares`, `fmv`, `vest_date` changed via the sell-to-cover write path). `fields_changed` arrays do not double-list. A shared field like `shares` that belongs to both tracks appears in the `vesting_event.sell_to_cover_override` payload **only** when the save is establishing or mutating a sell-to-cover override; an FMV-only edit to `shares` appears in `vesting_event.override` only.

## 9. Error and edge states

- **AC-9.1 — network error during submit preserves form state.** On the dialog, the Preferencias fiscales form, and any other new Slice-3b surface, a server error during submit renders an inline banner: `No se pudo guardar. Inténtalo de nuevo.` / `Could not save. Try again.` Form state preserved client-side; no partial save occurs. (Parity with Slice-3 AC-10.1.)
- **AC-9.2 — session expiry redirect preserving path.** A session that expires mid-edit triggers login redirect with flash `Tu sesión ha caducado.` / `Your session expired.` On re-login the user lands back on the originating path. Unsaved dialog values are **not** preserved across the re-login round-trip. (Parity with Slice-3 AC-10.2.)
- **AC-9.3 — cross-tenant 404 not 403.** For every new surface an id outside the current user's RLS scope returns 404, not 403. Explicitly covered: a `PUT` on `vesting_events/{id}` where the parent grant belongs to another user returns 404; a save of Preferencias fiscales that targets another user's row returns 404.
- **AC-9.4 — profile save concurrent with other-device edit.** Given Device A has the Preferencias fiscales form dirty with unsaved values, and Device B saves a different set of values, when Device A then submits, then Device A's save **wins** (close-and-create closes Device B's row; Device A's row becomes the new open row). Slice 3b intentionally does **not** apply OCC to `user_tax_preferences` — the semantic is "last-write-wins" because the history table preserves both writes regardless. Tester do-not-flag: this is NOT a defect, this is a deliberate departure from the dialog's OCC model.
- **AC-9.5 — dialog close with unsaved changes prompt.** Given the dialog has unsaved changes (any field differs from its value at dialog-open time), when the user attempts to close via Escape, Cancel, backdrop click, or close (×), then a confirmation renders: `Tienes cambios sin guardar. ¿Quieres descartarlos?` (ES) / `You have unsaved changes. Discard them?` (EN) with `Descartar` (destructive) and `Seguir editando` (default) actions. The Save path does not trigger this prompt. The `Revertir todos los ajustes` and `Revertir solo sell-to-cover` actions bypass this prompt (they have their own confirmation per AC-7.5.4 / AC-7.5.5).
- **AC-9.6 — validator + CHECK-constraint shared error envelope.** Validation errors from (a) client Zod/Yup, (b) server validator, (c) Postgres CHECK constraints (AC-5.2.*) all surface via the same envelope shape (Slice-2 AC-10.4; Slice-3 AC-10.6). New Slice-3b validators (all-or-none sell-to-cover triplet, tax percent in [0,1], share sell price > 0, currency whitelist) conform.
- **AC-9.7 — concurrent dialog edits (OCC).** Inherited from §7.4. An attempt to save a stale dialog returns 409 with the §7.4.2 banner; no audit row is written.
- **AC-9.8 — default-sourcing race.** Given the user saves a first sell-to-cover override on a vest event, and between the client's dialog-open and the server's save the user saves a new `user_tax_preferences` row on another tab, then the default-sourcing fires against **the current active `user_tax_preferences` row at the moment of save** — not at the moment of dialog open. Tester do-not-flag: this is the deliberate semantic (the user's most recent intent wins). No warning banner fires.

## 10. Mobile / responsive

All Slice-3b surfaces meet the Slice-1 / Slice-2 / Slice-3 mobile baseline. Additional Slice-3b-specific assertions:

- **AC-10.1 — dialog as full-screen sheet on narrow viewports.** On viewports ≤ 640 px, the per-row dialog renders as a **full-screen sheet** (not a centered modal): it occupies 100 % width and 100 % of the visual viewport height. The close control lives top-right; the Save / Cancel / Revert actions live in a sticky footer bar. The derived-values panel and the editable fields both scroll vertically within the sheet; the footer bar never scrolls out of view.
- **AC-10.2 — Preferencias fiscales form stacking.** On ≤ 640 px the Preferencias fiscales form stacks as: section heading → prose block → country picker (full width) → percent field (full width; shown/hidden per AC-4.2.2) → sell-to-cover toggle (full width) → Save CTA (full width). The history table scrolls horizontally with the first column (`Desde`) sticky; the percent and country columns collapse to abbreviations with a tooltip carrying the full label.
- **AC-10.3 — dialog derived-values panel stacking.** On ≤ 640 px the four derived-value rows stack vertically (`<dt>` above `<dd>` on each row).
- **AC-10.4 — touch targets ≥ 44×44.** Every new control (dialog buttons, history-table rows if made clickable in a later slice, sell-to-cover toggle) complies.
- **AC-10.5 — focus management on narrow viewport.** Focus-trap + return-focus (G-35) applies identically on narrow viewports. Safari-iOS "hide virtual keyboard" events do not close the dialog.

## 11. NFRs that do NOT apply (and why, explicitly)

This section is load-bearing. A tester validating Slice 3b must not mark these as defects.

- **§7.1 Tax-rule versioning — partial activation continues.** The footer chip continues to show ECB FX date + engine version (Slice 3 shape). The tax rule-set itself (`es-2026.1.0`) still does not exist in Slice 3b. Slice 4 is where §7.1 activates fully.
- **§7.4 Ranges-and-sensitivity — partial activation continues.** FX spread bands on the paper-gains tile continue to be the only "range" in the app. Slice 3b adds no ranges (the dialog's derived values render as point values, not ranges; Slice 4 is where IRPF projections produce ranges).
- **§7.5 Autonomía rate tables.** Still not ingested. No change in Slice 3b.
- **§7.6 Market-data vendor.** Still off. `share_sell_price` is user-entered in Slice 3b — there is no broker-quote lookup. Finnhub integration remains Slice 5.
- **§7.7 FX source — ON (Slice 3 unchanged).** ECB pipeline continues; Slice 3b adds no new FX usage. The dialog does not convert sell-to-cover amounts to EUR.
- **§7.8 Performance targets — extended.** The dialog's open-to-interactive latency is ≤ 200 ms P95 measured from the click on the row-level edit action to first paint of the dialog's derived-values panel (initial render with the row's current values). Save latency is ≤ 500 ms P95 (parity with Slice-3 AC-5.6.1). The Preferencias fiscales save is ≤ 500 ms P95.
- **§7.9 Security — pen-test.** Still Slice 9. Slice 3b adds no external egress; no new nftables entries.

## 12. Demo-acceptance script

The Slice-3 22-step flow is assumed complete; Slice 3b's demo picks up from a persisted user who has completed Slice 3 and holds a realistic portfolio including at least one RSU grant at **month-20 of a 48-month** vest (so there are ~19 past vests available as dialog-edit targets). The ECB worker continues running locally; no Slice-3b worker changes.

1. Open `http://localhost:<port>` and sign in as `test+slice3b@<domain>`. The Slice-3 dashboard renders: rule-set chip visible, paper-gains tile filled, ACME RSU grant visible.
2. Navigate to Profile. The **new "Preferencias fiscales" section** is visible below the Modelo 720 inputs panel and above the Sessions UI. The section shows an empty form (no prior rows): country picker empty, percent field hidden, toggle neutral (AC-4.1.3). History table reads `Sin historial aún.` (AC-4.1.3).
3. In the country picker, select `España`. The `Rendimiento del trabajo (%)` input appears immediately (AC-4.2.2). The `sell_to_cover_enabled` toggle pre-checks to `true` (AC-4.3.2).
4. Enter `45` in the percent field. Click `Guardar`. Profile re-renders; the form's open-row state now reflects `ES / 45,0000 % / on`. The history table remains empty (no prior closed row — this was the first save). Inspect `audit_log` for a `user_tax_preferences.upsert` row with `payload_summary = { outcome: "inserted" }` (AC-4.6.1).
5. Navigate to the ACME RSU grant's detail page. The "Precios de vesting" section renders with all past/future vest rows as in Slice 3. **No inline editor is available** (AC-7.8.1). Each row exposes a single edit action that opens the dialog.
6. Click the edit action on the **10th past vest** (month-10 of 48). The **dialog opens as a modal** over the grant-detail page, heading: `Editar vesting del {vest_date}`. The derived-values panel shows four dashes (`—`) because the row has no FMV, no sell price, no tax percent (AC-7.2.2). The tax-percent input's placeholder reads `p.ej. 45,0000 %` (AC-7.3.5). Screen reader announces the heading.
7. In the dialog, enter `$42.0000 USD` as FMV. The derived-values panel still shows dashes (only one of three required inputs is present; AC-7.2.2).
8. Enter `$42.2500` as share sell price (currency defaults to USD via AC-7.3.4). The derived-values panel still shows dashes (tax percent still NULL).
9. Click Save without entering a tax percent. The server path triggers default-sourcing per AC-7.6.1: the user's active `user_tax_preferences` row has `sell_to_cover_enabled = true` and `rendimiento_del_trabajo_percent = 0.4500`, so `tax_withholding_percent` is seeded as `0.4500`. Row persists; `is_sell_to_cover_override = true`, `sell_to_cover_overridden_at = now()`; `is_user_override = true` because FMV was also edited. Audit writes **both** `vesting_event.override` (`fields_changed: ["fmv"]`) AND `vesting_event.sell_to_cover_override` (`fields_changed: ["tax_percent", "sell_price", "sell_currency"]`) (AC-7.7.1, AC-8.4.1). The dialog closes; focus returns to the triggering row's edit button (AC-7.1.4).
10. Re-open the same row's dialog. The derived-values panel is now **populated**: `Bruto $4 200,00 USD` (assuming the vest event carries 100 shares), `Acciones vendidas 44,7337` (up-rounded at 4 dp per AC-6.3.1), `Neto entregado 55,2663`, `Retenido en efectivo $1 890,00 USD` (AC-6.2.*). The tax-percent input shows `45,0000 %` (seeded value).
11. Edit the tax percent to `47`. Derived values recompute live in the panel (AC-7.2.3): `Bruto` unchanged, `Acciones vendidas` rises, `Neto entregado` falls, `Retenido en efectivo` rises. Click Save. Row updates; audit writes a new `vesting_event.sell_to_cover_override` with `fields_changed: ["tax_percent"]` (AC-7.7.1).
12. Re-open the dialog. The new derived values reflect 47 %. Click `Revertir solo sell-to-cover` in the dialog footer (AC-7.5.2). Confirmation renders per AC-7.5.5; confirm. The server clears `tax_withholding_percent`, `share_sell_price`, `share_sell_currency`; **preserves** `fmv_at_vest = $42.0000 USD`, `fmv_currency = USD`, `is_user_override = true`, `overridden_at` (unchanged), `vest_date`, `shares_vested_this_event`. `is_sell_to_cover_override = false`. Audit writes `vesting_event.clear_sell_to_cover_override` with `payload_summary = { grant_id }` (AC-7.7.2). No `vesting_event.clear_override` is written.
13. Re-open the dialog. Derived values revert to dashes (no sell price + no tax percent → AC-7.2.2). FMV field still shows `$42.0000 USD` — preserved. The `Revertir todos los ajustes` button remains visible (the row still has `is_user_override = true` from the FMV edit); the `Revertir solo sell-to-cover` button is now hidden (AC-7.5.3 matrix row `false/false` or `true/false`).
14. Click `Revertir todos los ajustes`. Confirmation renders per AC-7.5.4; confirm. The server clears **everything**: FMV, sell price, tax percent, both override flags, both `*_at` timestamps; reverts vest_date and shares to the derivation algorithm's output. Audit writes **both** `vesting_event.clear_override` (Slice-3 shape; `cleared_fields: ["vest_date", "shares"]`, `preserved: []`) AND `vesting_event.clear_sell_to_cover_override` (AC-7.5.1, AC-7.7.3).
15. Return to Profile. In the Preferencias fiscales section, change the country to `Portugal`. The percent field **disappears** (AC-4.2.2); the `sell_to_cover_enabled` toggle pre-unchecks to `false` (AC-4.3.3). Click `Guardar`. Profile re-renders; the form's open-row state now reflects `PT / — / off`. The history table now shows **one prior closed row**: `Desde` = original save date, `Hasta` = today, `País` = `ES`, `Rendimiento del trabajo` = `45,0000 %`, `Sell-to-cover` = checkmark (AC-4.5.1). Inspect `audit_log` for a `user_tax_preferences.upsert` row with `payload_summary = { outcome: "closed_and_created" }` (AC-4.6.1).
16. Switch the country back to `España`. The percent field appears again, **blank** (it does not auto-repopulate from the prior history row — the user must re-enter or accept NULL). The toggle pre-checks to `true`. Enter `46` as the percent; click Guardar. Profile re-renders; the history table now has **two** closed rows (the `ES 45 %` row from step 4–15 and the `PT — off` row from step 15). The open row is now `ES 46 %`. Audit writes another `user_tax_preferences.upsert` with `outcome: "closed_and_created"`.
17. Save the Preferencias fiscales form **a second time within the same day** without changing any field. Audit writes `outcome: "updated_same_day"` (AC-4.4.3). The history table is unchanged — no new closed row is produced.
18. Return to grant-detail. The ACME RSU grant's "Precios de vesting" section still shows the test row from steps 12–14 back in its clean algorithmic state. Open the dialog; type FMV + sell price; leave tax percent blank; Save. Default-sourcing now sources **46 %** (the current open row, not the historical 45 %; AC-7.6.6 only suppresses re-sourcing on a row that *already* has a sell-to-cover override — this row's override was fully cleared in step 14).
19. Open a **future-dated** vest row's dialog (month-30 of 48, i.e., future). The vest_date and shares inputs render as **disabled / read-only** with the tooltip `Solo los vestings pasados pueden cambiar fecha o acciones.` (AC-7.3.3). FMV, sell price, tax percent remain editable.
20. Close the dialog via Escape. Focus returns to the row's edit button (AC-7.1.4).
21. Sign out. Sign back in. All Slice-3b state is preserved: Preferencias fiscales (`ES 46 % on`, 2 closed history rows), the dialog-edited row's sell-to-cover override values, the cleared-then-re-saved test row's fresh sell-to-cover override with percent `0.4600`.
22. Inspect `audit_log` rows for this session:
    - `user_tax_preferences.upsert` × 4 (step 4 `inserted`, step 15 `closed_and_created`, step 16 `closed_and_created`, step 17 `updated_same_day`).
    - `vesting_event.override` × 2 (step 9 FMV edit, step 14 full-revert triggers `vesting_event.clear_override` NOT `vesting_event.override` — so actually × 1 at step 9 only; count adjusted).
    - `vesting_event.sell_to_cover_override` × 3 (step 9 initial, step 11 tax-percent change, step 18 re-save).
    - `vesting_event.clear_sell_to_cover_override` × 2 (step 12 narrow, step 14 full).
    - `vesting_event.clear_override` × 1 (step 14 full).
    - Every payload conforms to §3.6 and §4.6 allowlists: no percents, no prices, no amounts, no country codes, no employer names, no ticker symbols.
23. Check product-analytics event payloads (if opted in): G-26 extended lint holds — no tax-percent values, no sell-price values, no country codes, no derived amounts in any payload.
24. Run `axe` CI job on the PR's preview URL: zero violations on the Preferencias fiscales section, on the dialog (open state with populated values, open state with empty values, open state with 409 banner, open state with unsaved-changes confirm), and on the history table.
25. Run the keyboard-only walkthrough of steps 6–20: every dialog interaction is reachable via Tab / Shift-Tab / Enter / Escape; focus ring visible; focus-trap holds while the dialog is open; focus returns correctly on close (G-35).

If all 25 steps pass, Slice 3b is accepted.

## 13. Out-of-scope reminders (tester do-not-flag list)

The following are **correct** behaviours in Slice 3b and must not be written up as defects. Each item is anchored to the slice where it actually ships (or to its status as a deliberate scope boundary).

- **No tax math.** Slice 3b captures the sell-to-cover data; the dialog's derived values are computed on-the-fly but are not consumed by any tax calculation. IRPF projection, Art. 7.p math, RSU cap-gains computation on the `fmv × net_shares_delivered` basis all ship in **Slice 4**. The Slice-3 paper-gains tile continues to compute on `fmv × shares_vested_this_event` (not `× net_shares_delivered`) — that tile's basis semantics update in **Slice 4**. Tester do-not-flag.
- **No Modelo 720 recalculation from sell-to-cover.** The M720 securities total in Slice 3b continues to use `fmv × shares_vested_this_event` per Slice-3 AC-6.1.1. The sell-to-cover-adjusted variant ships alongside the Slice-4 basis amendment.
- **No paper-gains tile impact.** The Slice-3 paper-gains tile's computation is unchanged in Slice 3b. Tester do-not-flag if Slice-3 AC-5.4.1 still reflects gross vested shares rather than net delivered.
- **No NSO sell-to-cover.** NSO exercise mechanics (`nso_exercises` table) and sell-to-cover on exercise both ship in **Slice 5**. The Slice-3b dialog is gated to `grants.instrument ∈ {rsu, espp}` for sell-to-cover editability (ADR-018 pins the exact gate — FMV-only editing on `nso` / `iso_mapped_to_nso` rows remains available per Slice 3's inline editor replacement, i.e., the dialog is also used for them but without the sell-to-cover fields; ADR-018 arbitrates the exact dialog field visibility on NSO rows).
- **No GeoIP auto country detection.** The Preferencias fiscales country picker starts empty on first render. `sessions.country_iso2` GeoIP population continues to be **Slice 9**. Tester do-not-flag.
- **No dual-residency concurrent tax-preferences rows.** One open row per user at a time; overlapping periods are not modelable in Slice 3b. Users with genuine dual residency are a post-v1 problem.
- **No per-grant tax-percentage default.** The percent lives on the user, not on the grant (Q-D). A per-grant default is a post-v1 feature.
- **No automatic FMV ↔ sell-price reconciliation warning.** If the user enters `fmv_at_vest = $42` and `share_sell_price = $42 000`, no banner warns about the suspect ratio. The two fields are independent user inputs in Slice 3b; Slice 4+ may add sanity heuristics, but that is out of this slice's scope.
- **No retroactive back-fill of sell-to-cover on pre-Slice-3b vest rows.** Existing vest rows carry `tax_withholding_percent IS NULL` until the user explicitly edits them via the dialog (AC-5.1.2). Saving the Preferencias fiscales form does NOT walk existing rows and seed their percents (AC-7.6.7).
- **No retroactive back-fill of percent on Profile save.** Changing `rendimiento_del_trabajo_percent` from 45 to 46 does not mutate any existing `vesting_events` row. Historical vests retain whatever percent they had (or NULL). Only the next default-sourcing event (a new override on a fresh row) sees the new percent.
- **No history-table editability.** Prior closed `user_tax_preferences` rows are read-only (AC-4.5.3). Retroactive correction of a prior period is not a Slice-3b surface.
- **No override-badge on the cumulative vesting timeline.** Continues to be deferred (Slice-3 do-not-flag). Slice-4 polish concern.
- **No FX conversion inside the dialog.** Derived values render in the grant's native currency; EUR conversion lives on the Slice-3 paper-gains tile on the dashboard and nowhere else.
- **No Slice-3 inline editor.** The Slice-3 inline editor is replaced by the dialog. Slice-3 AC-8.2 / AC-8.3 / G-33 assertions migrate to §7; any tester familiar with Slice 3 who expects the inline editor to be present is wrong — the editor moved into the dialog.
- **No bulk-edit of sell-to-cover fields.** The Slice-3 bulk-fill modal ("Aplicar FMV a todos", AC-8.6) continues to bulk-fill **only** FMV. There is no "apply sell-to-cover to all" analogue in Slice 3b. Users must open the dialog per row.
- **No recompute under current rules.** Continues to be dormant (Slice-3 do-not-flag). Wakes in Slice 6.
- **No PDF / CSV export of anything.** Slice 6.
- **No bulk import.** Slice 8.
- **2FA still optional, still not shipping.** Slice 7.
- **No legal surface.** Slice 9.
- **No pen-test.** Slice 9.
- **No paid tier.** v1.2 PoC posture — permanent.
