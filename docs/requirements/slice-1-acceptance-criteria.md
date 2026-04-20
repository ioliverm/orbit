# Orbit v1 — Slice 1 acceptance criteria

| Field       | Value                                                      |
|-------------|------------------------------------------------------------|
| Version     | 1.1                                                        |
| Date        | 2026-04-19                                                 |
| Owner       | requirements-analyst (Ivan Oliver)                         |
| Slice       | Slice 1 — "First portfolio" (see `v1-slice-plan.md` v1.2)  |
| Boundary    | Sign up → residency step → first grant → land on dashboard with one grant tile → view vesting timeline. **No tax math. No FX. No CSV import. No exports. No scenarios. No sell-now.** |
| Related     | Spec US-001, US-003, US-006 (partial), US-011 (data-minimization part only); UX screens `dashboard.html` (empty + single-grant tile), `grant-detail.html` (vesting section). |
| v1.1 update | Resync with `v1-slice-plan.md` v1.2 (2026-04-19): demo URL is localhost (deploy deferred to Slice 8); sidebar nav entries no longer show `[paid]` badges and no blurred-preview state exists (v1.2 PoC has no paid tier); Slice 1 does not ship any TOTP UI (optional TOTP lands in Slice 7). No AC body changes. |

This document is implementation-ready. Every AC below is testable as-written. Where a tester needs a specific screen state, the UX reference HTML is cited by filename.

## 1. In-scope stories

| Story | In Slice 1? | Notes |
|-------|-------------|-------|
| **US-001 — Create and manage grants manually** | Yes, in full | All three ACs apply. |
| **US-003 — Visualise vesting incl. double-trigger** | ACs 1, 2, 4 apply | AC 3 is scenario-mode and ships in Slice 4. |
| **US-006 — Autonomía selection** | ACs 1, 2, 4 apply | AC 3 (mid-year autonomía change) ships in Slice 4 when tax math arrives; data model stores the `residency_periods` row correctly in Slice 1. |
| **US-011 — GDPR DSR self-service** | Partial: data-minimization in analytics only | Full DSR self-service ships in Slice 7. |
| **US-002 — CSV import** | No | Slice 8 (deferred from Slice 2 per v1.3 plan decision 2026-04-20). |
| **US-004..US-009, US-012, US-013** | No | Later slices. |

## 2. Persona & demo context

- Primary tester persona: **María, pre-IPO**, see spec §2.1.
- Device: 14" laptop (1440×900), Chrome + Safari. Mobile must render but is not the primary acceptance surface for this slice.
- Locale acceptance: **ES primary**, EN fallback. Every user-visible string passes through the i18n layer.

## 3. Global ACs (apply to every screen this slice ships)

### 3.1 Non-advice disclaimer — footer

- **G-1.** The persistent footer strip renders on every authenticated page in Slice 1.
- **G-2.** Footer text (ES): `Esto no es asesoramiento fiscal ni financiero — Orbit calcula, no aconseja.`
- **G-3.** Footer text (EN): `This is not tax or financial advice — Orbit calculates, it doesn't recommend.`
- **G-4.** The footer is **not fixed to the viewport**; it sits at the bottom of the scrollable content area. (UX §8 rationale.)
- **G-5.** The footer in Slice 1 does **not** display a rule-set chip or a "Ver trazabilidad" link. No calculation outputs exist in Slice 1, so no rule-set surface is honest to show. (See C-3 in `open-questions-resolved.md`.)
- **G-6.** The footer height is 32 px and does not obscure content when the page is at minimum supported height.
- **G-7.** The footer is reachable in tab-order after the main content (not skipped, not first).

### 3.2 Non-advice disclaimer — first-login modal (already in Slice 0, re-verified here)

- **G-8.** A user who has not yet accepted the disclaimer cannot reach the dashboard, the grant form, or the residency step. Attempting to bypass via URL redirects them to the modal.
- **G-9.** Acceptance writes an `audit_log` entry with `action = "dsr.consent.disclaimer_accepted"`, `target_kind = "user"`, `target_id = user.id`, `occurred_at = now()`, `payload_summary = { version: "<modal_copy_version>" }`. No grant data, no PII in the payload summary.
- **G-10.** Re-login after acceptance does not re-display the modal (UX §4.1 rationale).

### 3.3 i18n

- **G-11.** Every string shipped in Slice 1 has an entry in both `es-ES` and `en` catalogs. CI lint fails a PR that introduces a string without both locales.
- **G-12.** Spanish tax terms remain in Spanish even when UI locale is EN: `IRPF`, `rendimiento del trabajo`, `ahorro base`, `Modelo 720`, `Art. 7.p`, `autonomía`, `territorio común`, `foral`, `Beckham`. (Per D-5.) **Note:** no tax math ships in Slice 1, so the only term that can appear is `autonomía` on the residency step. Validate the locale behaviour even though the surface is small.
- **G-13.** Number formatting follows locale (UX §5.7): ES uses `.` thousands / `,` decimal; EN uses `,` thousands / `.` decimal. Share counts are integers with thousands-separators. No currency conversion in Slice 1, so only USD is rendered — formatted per locale convention but currency suffix always explicit (`$8.00 USD`).
- **G-14.** Date formats: storage in ISO 8601; display in user-locale long form (ES: `15 sep 2024`; EN: `Sep 15, 2024`).
- **G-15.** Line-length for form labels tested in ES first (Spanish text is ~20% longer than EN); no label truncation at desktop breakpoints.

### 3.4 Accessibility (WCAG 2.1 AA / 2.2 AA per UX)

- **G-16.** Every page has exactly one `<h1>`; heading order strict, no skips.
- **G-17.** Landmarks present: `<header>`, `<nav>`, `<main>`, `<footer>`.
- **G-18.** Every form input has a visible `<label>` or `aria-labelledby`. Error states use `aria-describedby`, not color alone.
- **G-19.** `focus-visible` ring on every interactive element (2 px width, 2 px offset, accent color per UX §9). No `outline: none` anywhere without a visible replacement.
- **G-20.** Tab order matches visual reading order (left-to-right, top-to-bottom).
- **G-21.** `axe` smoke test in CI passes on: sign-up wizard, residency step, grant form, dashboard, grant-detail screen.
- **G-22.** Layout works from 100 % to 200 % zoom without horizontal scroll (per UX §9).
- **G-23.** Color is never the only signal: validation errors carry an icon + text; the "time-vested awaiting liquidity event" state on vesting timelines uses a distinct **pattern** (dashed fill per D-7) plus a label, not only a color.
- **G-24.** `prefers-reduced-motion`: no non-essential animation runs (no vesting-timeline reveal, no tile fade-in). The only motion retained is the disclosure widget on edit-grant (essential per UX §5.5).
- **G-25.** `prefers-color-scheme: dark` renders the full dark token set (UX §5.2) for all Slice 1 screens.

### 3.5 GDPR / data minimization

- **G-26.** No grant values, no share counts, no strike prices, no autonomía selection, no Beckham flag appear in analytics event payloads. The per-event payload schema is reviewed and CI-lint-enforced (per §7.2).
- **G-27.** Analytics are disabled by default and enabled only after explicit cookie-banner opt-in (AEPD 2023).
- **G-28.** No PII (email, name) leaves EEA in any request path. Slice 1 runs entirely on a developer machine against local Docker Compose Postgres per ADR-015 §0a (v1.1: cloud deploy deferred to Slice 8); no external services are called from Slice 1 code paths.
- **G-29.** Logs redact email addresses (replaced by the user's UUID once known; pre-auth logs redact beyond the domain).

### 3.6 Observability

- **G-30.** Every request logs: `request_id`, `user_id` (null pre-auth), `route`, `method`, `status`, `latency_ms`, `db_tx_count`. No PII, no grant values.
- **G-31.** Auth events (`login.success`, `login.failure`, `signup.success`, `signup.failure`, `logout`) write to `audit_log`.
- **G-32.** Grant create / edit / delete writes to `audit_log` with `target_kind = "grant"`, `target_id = grant.id`. `payload_summary` contains only non-sensitive metadata: `instrument`, never share counts or values.

## 4. Sign-up wizard (new in Slice 1; extends Slice 0 sign-up)

Sequence: **disclaimer modal (Slice 0) → residency step → first grant form → dashboard**.

### 4.1 Residency step

User lands here immediately after disclaimer acceptance on first login; and from Account → Profile thereafter.

- **AC-4.1.1 — happy path.** Given a user who has just accepted the disclaimer, when they land on the residency step, then they see three fields: **Autonomía** (dropdown), **Régimen Beckham** (radio: Sí / No, default No), **Moneda principal** (dropdown: EUR / USD, default EUR).
- **AC-4.1.2 — autonomía list.** The Autonomía dropdown lists all territorio común autonomías plus Ceuta and Melilla in alphabetic ES order, **plus** País Vasco and Navarra with a visible suffix `(no soportado en v1)` (ES) / `(not supported in v1)` (EN). Selecting a foral autonomía is allowed; it does not block submission.
- **AC-4.1.3 — foral storage.** Given the user selects País Vasco or Navarra, when they submit, then a `residency_periods` row is created with `jurisdiction = 'ES'`, `sub_jurisdiction = 'ES-PV'` (or `ES-NA`), `regime_flags` contains `foral_pais_vasco` (or `foral_navarra`), `from_date = today`, `to_date = null`. The user is **not** shown a tax-calc block in Slice 1 (no tax calcs exist yet).
- **AC-4.1.4 — Beckham storage.** Given the user selects Beckham = Sí, when they submit, then the `residency_periods.regime_flags` contains `beckham_law`. No UI block is shown in Slice 1 (no tax calcs exist yet).
- **AC-4.1.5 — submit advances.** On submit with all three fields valid, the user is advanced to the first-grant form.
- **AC-4.1.6 — submit blocked.** Given any of the three fields is empty, when the user submits, then the form blocks with inline validation and no partial save occurs.
- **AC-4.1.7 — edit later.** Account → Profile exposes the same three fields for editing. Editing creates a **new** `residency_periods` row and closes the prior one (`to_date = today`). The prior row is not updated in place. (This enables C-13 future behaviour even though Slice 1 has no calculations to show diff.)
- **AC-4.1.8 — audit log.** Residency create/edit writes to `audit_log`; `payload_summary` contains `{ autonomia_changed: bool, beckham_changed: bool, currency_changed: bool }` — the booleans, not the values.

### 4.2 First-grant form

Reference screen: `grant-detail.html` edit-mode section; see also UX §4.1 step 3.

- **AC-4.2.1 — instrument picker.** Options: RSU, NSO, ESPP, ISO. Selecting ISO stores `instrument = 'iso_mapped_to_nso'` and displays an inline informational note: `Las ISO se tratan como NSO a efectos fiscales españoles en v1.` (ES) / `ISOs are treated as NSOs for Spanish tax purposes in v1.` (EN).
- **AC-4.2.2 — conditional fields.** RSU grant shows: grant date, share count, employer (free text), ticker (optional), vesting template, double-trigger toggle. NSO/ISO adds strike. ESPP shows: grant date (offering date), expected purchase date(s), employer, ticker (optional), estimated discount % (default 15). **ESPP purchase details (FMV-at-purchase, purchase price) are not captured on the first-grant form** — they are captured on a later "record ESPP purchase" action shipped in Slice 2. (Keeps Slice 1 single-grant-form.)
- **AC-4.2.3 — vesting templates.** Four presets: "4 años, cliff 1 año, mensual" (default); "4 años, cliff 1 año, trimestral"; "3 años, sin cliff, mensual"; "Personalizado" (exposes `vesting_total_months`, `cliff_months`, `vesting_cadence`).
- **AC-4.2.4 — double-trigger toggle.** Visible only when instrument = RSU. Default off. If on, a follow-up field `liquidity_event_date` (optional) appears; empty = "not yet occurred".
- **AC-4.2.5 — live vesting preview.** As the user types valid vesting fields, a vesting sparkline preview renders on the right (or below on narrow viewports) using the same algorithm the final timeline uses. (UX §4.1 step 3.) The preview uses the native grant currency; no EUR conversion.
- **AC-4.2.6 — cliff > vest validation (US-001 AC #3).** Given the user enters `cliff_months > vesting_total_months`, when they submit, then the form rejects with inline error `El cliff no puede superar el periodo total de vesting.` / `The cliff cannot exceed the total vesting period.` No row is created.
- **AC-4.2.7 — negative / zero share count.** Given `share_count <= 0`, when submit, then inline rejection `Introduce un número de acciones mayor que 0.` / `Enter a share count greater than 0.`
- **AC-4.2.8 — strike required conditional.** Given instrument in {NSO, ISO}, when the user submits without a strike, then inline rejection. RSU/ESPP do not require strike.
- **AC-4.2.9 — grant date future.** Given `grant_date > today + 1 day`, when submit, then a non-blocking warning: `La fecha es futura. ¿Estás seguro?` (ES) / `That date is in the future. Are you sure?` (EN). User may proceed.
- **AC-4.2.10 — successful create (US-001 AC #1).** Given a valid grant, when submit, then a `grants` row is created under the current user_id via `Tx::for_user`, and derived `vesting_events` are populated for rendering. Audit-log row written.
- **AC-4.2.11 — "I have many grants" link.** Below the first-grant form, a link reads `Tengo varios grants — importaré desde Carta o Shareworks después` (ES) / `I have multiple grants — I'll import from Carta or Shareworks later` (EN). In Slice 1 this link **dismisses the form and advances to an empty dashboard**; CSV import itself does not exist until Slice 8 (deferred from Slice 2 per v1.3). The dashboard then shows an empty-state tile directing the user to "Añadir grant". (This is a Slice 1 compromise: the link is necessary UX per §4.1 but the destination is Slice 8.)

### 4.3 Vesting-derivation algorithm (implementation constraint, not UI)

The vesting-derivation used by both the live preview and the persistent timeline must satisfy:

- **AC-4.3.1.** For `cliff_months = 0` and `vesting_cadence = 'monthly'`, shares vest evenly across `vesting_total_months`; each month's vested share count is `floor((i * total) / months)` where `i` is the month index; the final month receives any rounding remainder so that total vested at month `vesting_total_months` equals exactly `share_count`.
- **AC-4.3.2.** For `cliff_months > 0`, nothing vests before the cliff; at month `cliff_months`, `floor((cliff_months * total) / months)` shares vest; monthly thereafter per AC-4.3.1.
- **AC-4.3.3.** For `vesting_cadence = 'quarterly'`, vesting grains are 3 months; otherwise identical to monthly.
- **AC-4.3.4.** For double-trigger RSUs with `liquidity_event_date = null`, time-vested shares accumulate per the above, but are rendered with the `time_vested_awaiting_liquidity` visual state. No shares are ever reported as "fully vested" until the liquidity event is set.
- **AC-4.3.5.** Deterministic: the same grant always yields the same vesting event list. Covered by a property-based test in CI.

## 5. Dashboard (Slice 1 version)

Reference: `docs/design/screens/dashboard.html`, adapted for Slice 1 (no tax numbers, no EUR conversion, no Modelo 720 banner).

### 5.1 Empty state

- **AC-5.1.1.** Given a user with zero grants, when they load the dashboard, then they see: a headline `Tu cartera` (ES) / `Your portfolio` (EN), a prose empty-state explaining what a grant is in one sentence, and a primary CTA `Añadir grant` (ES) / `Add grant` (EN).
- **AC-5.1.2.** No Modelo 720 banner, no rule-set chip, no tax tiles, no scenario CTA, no sell-now CTA on the empty state.

### 5.2 Single-grant / multi-grant tile state

- **AC-5.2.1.** Given a user with ≥1 grant, when they load the dashboard, then each grant is rendered as a tile. Tiles contain: employer, instrument, share count, grant date, vested-to-date count (integer), a small vesting sparkline.
- **AC-5.2.2.** Grant values displayed in **native currency only** (C-4 decision). For US-company grants, this is USD. No EUR paper-gains. No FX conversion. Currency suffix always explicit on monetary values (`$8.00 USD`).
- **AC-5.2.3.** Clicking a tile navigates to `grant-detail.html` for that grant.
- **AC-5.2.4.** A secondary CTA `Añadir otro grant` is visible below the tiles.

### 5.3 What is NOT on the Slice 1 dashboard

The following UX `dashboard.html` elements are **deferred** in Slice 1 and should not render even as placeholders:

- Modelo 720 passive banner (Slice 3).
- Paper-gains EUR tile (Slice 3 once FX is live).
- Rule-set chip in footer (Slice 4 once calculations exist).
- Sell-now tile / Scenario tile (Slice 4/5; they appear in the sidebar as paid preview-only, but not on the dashboard itself).
- Art. 7.p reminder tile (Slice 2).

## 6. Grant detail + vesting timeline

Reference: `docs/design/screens/grant-detail.html`.

### 6.1 Read view

- **AC-6.1.1.** Given a user opens a grant, when the page loads, then it renders three regions: **Summary** (fields as entered), **Vesting timeline**, **Edit** button.
- **AC-6.1.2.** The vesting timeline renders the cumulative-curve view by default (D-7); a toggle switches to Gantt view.
- **AC-6.1.3.** Monthly cliff and post-cliff tranches are shown (US-003 AC #1).
- **AC-6.1.4.** For double-trigger RSUs whose `liquidity_event_date IS NULL`, time-vested tranches are rendered with dashed fill + label `Vesting por tiempo, pendiente de evento de liquidez` (ES) / `Time-vested, awaiting liquidity event` (EN) (US-003 AC #2). A summary line asserts `Ingresos imponibles hasta la fecha: 0 acciones` (ES) / `Taxable income to date: 0 shares` (EN).
- **AC-6.1.5.** For stacked grants visible on this screen (if any refresh grants exist from the same employer), a per-grant drill-down is available (US-003 AC #4). The combined cumulative view lives on the dashboard in Slice 1; the grant-detail screen shows one grant only.
- **AC-6.1.6.** No tax numbers anywhere on this screen. No EUR conversion.

### 6.2 Edit view (US-001 AC #2)

- **AC-6.2.1.** Clicking Edit surfaces the same form as §4.2 pre-populated.
- **AC-6.2.2.** On save with a changed `vesting_start`, all derived `vesting_events` are recomputed and the timeline updates on re-render. The edit is recorded in `audit_log`.
- **AC-6.2.3.** Editing cannot violate the cliff > vest rule (AC-4.2.6).
- **AC-6.2.4.** Deleting a grant is available on this screen with a two-step confirm. Delete writes to `audit_log` with `action = 'grant.delete'`; the grant row is hard-deleted (grants are user data, no retention obligation at grant-row granularity).

## 7. Error and edge states

- **AC-7.1.** Network error during submit: inline banner `No se pudo guardar. Inténtalo de nuevo.` / `Could not save. Try again.` Form state preserved client-side.
- **AC-7.2.** Session expired mid-edit: redirect to login with a flash message `Tu sesión ha caducado.` / `Your session expired.` On re-login the user returns to the grant being edited (redirect preserving path).
- **AC-7.3.** Grant not found / not owned: the `Tx::for_user`-scoped query returns zero rows; the UI renders a 404 state, not a 403 (RLS is fail-closed; we do not leak existence).
- **AC-7.4.** Keyboard-only user can complete the sign-up → residency → first grant → dashboard flow without a mouse (covered by the global A11y ACs plus manual keyboard walkthrough in acceptance).
- **AC-7.5.** Screen-reader walkthrough: VoiceOver on macOS + NVDA on Windows each complete the same flow. Live vesting preview announces via `aria-live="polite"` when the preview has updated (debounced to avoid spam).

## 8. Mobile / responsive

Per UX §10 and D-6.

- **AC-8.1.** Sign-up wizard stacks vertically; steps remain usable on ≤640 px widths.
- **AC-8.2.** Dashboard stacks tiles single-column; sidebar collapses to a hamburger.
- **AC-8.3.** Grant detail stacks panels; timeline horizontally scrollable on small viewports with the leftmost date column sticky.
- **AC-8.4.** Touch targets ≥44×44 on `pointer: coarse`.
- **AC-8.5.** No UI is blocked on mobile in Slice 1. (The "scenario modeler not recommended on mobile" banner from D-6 is a Slice 4 concern.)

## 9. NFRs that do NOT apply (and why, explicitly)

This section is load-bearing. A tester validating Slice 1 should not mark these as defects.

- **§7.1 Tax-rule versioning.** No calculations exist in Slice 1. No rule-set chip is displayed. No stamping occurs. Full §7.1 activates in Slice 4. (C-3, C-9.)
- **§7.4 Ranges-and-sensitivity.** No tax numbers in Slice 1 → no ranges. The `share_count` shown is an integer, not an estimate. Full §7.4 activates in Slice 4.
- **§7.5 Autonomía rate tables.** The autonomía *selector* is in Slice 1 (storage only). The *rate tables* are not ingested until Slice 4. A foral selection is stored but no tax-calc block is shown.
- **§7.6 Market-data vendor.** No quotes needed in Slice 1 (no paper-gains EUR, no sell-now). Finnhub integration ships in Slice 5.
- **§7.7 FX source.** No EUR conversion in Slice 1. ECB pipeline ships in Slice 3.
- **§7.8 Performance.** Slice 1 has no compute-heavy path. P95 targets in §7.8 are met trivially; acceptance is "page loads feel snappy" (≤2 s P75 dashboard per §7.8, validated on EU broadband from a cold CDN cache).
- **§7.9 Security — pen-test.** Deferred to Slice 7. Slice 1 is expected to be free of the OWASP Top 10 basics via framework defaults and code review; a penetration test is not a Slice 1 gate.
- **US-004..US-013.** Not shipped. Any UI gesture that would lead to those flows routes to a "próximamente" placeholder. (v1.2 note: no paid gating, so nav entries are plain placeholders — no blurred-preview state, no `[paid]` badges.)

## 10. Demo-acceptance script

A single PR is merged. The reviewer runs through:

1. Open `http://localhost:<port>` as a brand-new user (the local dev stack from ADR-015 §0a; v1.1 defers any public `app.orbit.<tld>` URL to Slice 8).
2. Sign up with `test+slice1@<domain>` (see Slice 0a setup). Verify email (retrieve the verification link from the local SMTP sink or structured-log output). Log in.
3. See disclaimer modal. Read ES copy. Accept.
4. Residency step: select Comunidad de Madrid. Beckham = No. Primary currency = EUR. Submit.
5. First-grant form: RSU, 30,000 shares, grant date 2024-09-15, employer "ACME Inc.", ticker blank, template "4 años / cliff 1 año / mensual", double-trigger = Sí, liquidity event date blank. Submit.
6. Land on dashboard. See one tile: ACME Inc. · RSU · 30,000 acciones · 2024-09-15 · vested-to-date `7,500 acciones` (if today is between `2025-09-15` and `2025-10-15`) · sparkline renders.
7. Click the tile. Grant detail loads. Timeline shows monthly tranches post-cliff; the entire vesting curve is dashed (awaiting liquidity event); summary asserts "Ingresos imponibles hasta la fecha: 0 acciones".
8. Click Edit. Change grant date to 2024-08-15. Save. Timeline updates; vested-to-date changes accordingly.
9. Return to dashboard. Click "Añadir otro grant". Create an NSO grant (10,000 shares, $8 strike, same dates, no cliff, no double-trigger).
10. Dashboard now shows two tiles. Sparklines differ.
11. Switch locale to EN via top-bar toggle. Verify strings; verify number format changes (`10,000` EN vs `10.000` ES).
12. Switch to País Vasco in Profile → residency. No block appears anywhere (nothing to block — no tax calcs). Revert to Madrid.
13. Log out; log back in. All state preserved. No disclaimer modal this time.
14. Run `axe` CI job on the PR's preview URL; zero violations.
15. Run keyboard-only walkthrough of steps 2–10; every interaction is reachable via Tab / Shift-Tab / Enter / Space; focus ring visible at every step.
16. Check `audit_log` table: rows for `signup`, `login`, `disclaimer.accept`, `residency.create`, `grant.create` × 2, `grant.update`, `login`, `logout`.
17. Check product-analytics event payloads (if opted in): no share counts, no strikes, no autonomía values, no Beckham flag.

If all 17 steps pass, Slice 1 is accepted.

## 11. Out-of-scope reminders (tester do-not-flag list)

The following are **correct** behaviours in Slice 1 and must not be written up as defects:

- No EUR amount appears anywhere.
- No "you will owe X" number appears anywhere.
- No rule-set version chip in the footer.
- The sidebar entries "Sell-now", "Escenarios", "Modelo 720", "Exports" render but route to a "próximamente" page. **No `[paid]` badges — v1.2 PoC has no paid tier.** No blurred-layout preview state at any slice (the UX D-9 pattern was scrapped in v1.2 along with the paid gate).
- CSV import is not offered (deferred to Slice 8 per v1.3). The "Tengo varios grants" link dismisses the form to an empty dashboard.
- País Vasco / Navarra selection produces no tax-calc block because there is no tax-calc surface to block.
- Beckham = Sí produces no tax-calc block for the same reason.
- Art. 7.p trip entry is not offered.
- No export of any kind.
- No "recompute under current rules" action.
- No sensitivity ranges anywhere.
- 2FA is **not** offered in Slice 1 at all. The Account screen has no TOTP setup UI; the ADR-011 TOTP scaffolding returns 501 per its Slice-1 shape. Optional TOTP ships in Slice 7 (v1.2 scope: optional for every user; OQ-01 mandatory-for-paid is moot since there is no paid tier).
- "Export my data" and "Delete my account" buttons exist in Account → Data & privacy but link to a "próximamente" page; full DSR self-service ships in Slice 7.
