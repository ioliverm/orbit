# Orbit v1 — Implementation slice plan

| Field       | Value                                                      |
|-------------|------------------------------------------------------------|
| Version     | 1.3                                                        |
| Date        | 2026-04-20                                                 |
| Owner       | requirements-analyst (Ivan Oliver)                         |
| Sources     | `docs/specs/orbit-v1-persona-b-spain.md`, ADR-001..ADR-008, ADR-015 (local-first split), `docs/design/orbit-v1-ui-proposal.md`, `docs/requirements/open-questions-resolved.md` |
| Purpose     | Break v1 into ordered, independently shippable vertical slices. Each ends in a demo-able state. No layers; every slice crosses frontend + backend + data + ops. |
| v1.1 change | Product owner decision 2026-04-19: defer cloud deployment to the end. Slices 1–7 build and demo against the local stack established in Slice 0a. Everything deferred from ADR-015 §0b, plus the pen-test and legal-surface publication previously in Slice 7, consolidate into a new **Slice 8 — Production deployment & launch gate**. No feature work changes. |
| v1.2 change | Product owner decision 2026-04-19 (same day, follow-up): **this is a PoC; no Stripe, no paid tier.** The free/paid distinction is removed everywhere. Every account is the same account; every feature is available to every user. Slice 3 collapses from "Paid shell + Modelo 720 passive UX" to just "FX pipeline + Modelo 720 passive UX + dashboard EUR conversion". Billing, subscription state, VAT handling, feature-matrix screen, preview-only blurred state, and `subscriptions` table all drop out. TOTP 2FA becomes optional for every user in Slice 7 (no "mandatory for paid"). Stripe live-mode cutover and professional indemnity insurance drop out of Slice 8. |
| v1.3 change | Product owner decision 2026-04-20: **defer bulk-import tooling (CSV Carta + Shareworks, ETrade PDF) to the end of v1**, immediately before cloud deployment. Slice 2 trims to the non-import surfaces (ESPP purchases, Art. 7.p trips, multi-grant dashboard, Modelo 720 category inputs, session-device UI). A new **Slice 8 — Portfolio bulk import** is inserted between the old Slice 7 and the old Slice 8; the deploy slice renumbers to **Slice 9 — Production deployment & launch gate**. Rationale: hand-entry through Slice 7 is the minimum viable path for Ivan's own dogfooding; import tooling has a long-tail QA story (column-mapping edge cases, vendor CSV drift) that is safer to build in one concentrated batch just before production users arrive. Neither the "Tengo varios grants" link (AC-4.2.11) nor the bulk-import affordance in Slice 2 is moved — both defer to Slice 8. Internal-readiness gate shifts: **end of Slice 8** is now the feature-complete mark (was end of Slice 7); end of Slice 7 is the "hand-entry-only MVP" mark. |

## Principles

1. **Vertical, not horizontal.** Every slice includes frontend, backend, data, and ops changes. No "build the whole backend, then the whole frontend" plan.
2. **Demo-able at the boundary.** A slice is done when a real Persona-B user could open the app and experience something they could not before.
3. **Non-goals are load-bearing.** Each slice explicitly lists what it does not ship; this is how scope creep is caught at the boundary.
4. **Respect the ADRs.** Tech stack, cloud, data-model outline, versioning scheme, FX source, vendor, and export traceability are all decided (ADR-001..ADR-008). Slices do not re-litigate these.
5. **No paid tier in the PoC (v1.2).** Every account gets every feature. No Stripe integration, no subscription state, no feature gating, no preview-only UX, no `subscriptions` table. If monetization is ever reopened, it is a post-v1 initiative with its own spec and its own slice.
6. **Local-first, deploy last (v1.1, broadened in v1.3).** Slices 0a–8 run against the local Docker Compose Postgres + Vite dev server stack from ADR-015 §0a. Cloud deployment is a single concentrated slice at the end, once the app is feature-complete and polished. No external user ever touches a Slice 0a–8 build; public launch happens only after Slice 9 closes.

## Summary table

| # | Slice | One-liner | T-shirt | Demo |
|---|-------|-----------|---------|------|
| 0a | Foundation shell (local) | Bootable local app, auth, empty dashboard, CI, observability skeleton, cookie banner. | M | Log in on `localhost`, land on empty dashboard, log out. |
| 1 | **First portfolio** | Sign up → residency → first grant → see vesting schedule. | L | Persona B enters one grant, sees vesting timeline. Nothing else. |
| 2 | Portfolio completeness (hand-entry) | Multiple grants, dashboard tiles, ESPP purchases, Art. 7.p trip entry, Modelo 720 category inputs, session-device UI. **No bulk import** (Slice 8). | M | Persona B adds several grants by hand, records an ESPP purchase, logs a trip. |
| 3 | FX + Modelo 720 passive UX | ECB FX pipeline, dashboard EUR conversion (paper-gains tile), Modelo 720 threshold alert, rule-set chip in footer. | M | Paper-gains tile shows EUR with bands; M720 threshold alert fires; footer chip shows ECB FX date + engine version. |
| 4 | Tax engine + autonomía + scenario modeler | First tax numbers. Rule-set versioning goes live. Ranges-and-sensitivity NFR activates. | XL | Persona B runs the IPO/lockup/hold scenario and sees net proceeds with sensitivity. |
| 5 | Sell-now calculator (post-IPO leg) | Finnhub (dev tier) + ECB pipeline + US-013. | L | Persona B opens sell-now, enters lots, sees net-EUR-landing range. |
| 6 | Exports + Modelo 720 worksheet + recompute | Gestor PDF, CSV, traceability IDs, recompute-under-current-rules. | L | Persona B exports a scenario PDF; tests recompute after a rule-set bump. |
| 7 | GDPR DSR self-service + optional 2FA | Data export, erasure, TOTP optional for all users. | M | User enables TOTP; exports their data archive; deletes account; 30-day grace works. |
| 8 | **Portfolio bulk import** | Carta + Shareworks CSV import, ETrade PDF import, column-mapping preview, row-level error report, rejected-rows CSV download. Internal-readiness gate. | L | Persona B imports 10 grants from Carta; rejects fix via a second pass; dashboard renders the imported portfolio. |
| 9 | **Production deployment & launch gate** | Hetzner stack, TLS/HSTS/nftables, offsite backups, MFA on live accounts, published legal surface, Finnhub commercial cutover, third-party pen-test. Everything deferred from ADR-015 §0b. | L | Deploy to `app.orbit.<tld>`; run the Slice-8 demo on production; uptime + alert route + pen-test report green. |

**Hand-entry-only MVP gate = end of Slice 7.** Every decision-support surface works end-to-end against hand-entered grants; no bulk import yet.

**Internal-readiness gate = end of Slice 8.** Orbit is feature-complete (including import), polished, and locally testable end-to-end; still no external user.

**Public launch gate = end of Slice 9.** Cloud deployment, legal surface, third-party pen-test, and commercial contracts (Finnhub commercial-tier) all green. First external user onboards after Slice 9 closes.

Slices 0a–5 are internal / closed-beta-acceptable on local only. Slice 6 is needed for the gestor promise. Slice 7 raises the GDPR/security bar. Slice 8 adds the import surface. Slice 9 is the production bar.

---

## Slice 0a — Foundation shell (local)

### Scope one-liner
The smallest bootable Orbit that a developer can sign up to and log in to **on localhost**. No product features, no cloud.

> **Relationship to ADR-015.** This slice is exactly ADR-015 §0a. ADR-015 §0b — the deploy-green checkpoint — used to be the second half of Slice 0; per the 2026-04-19 product-owner decision it has been **moved to Slice 9** and no longer gates Slice 1.

### Entry state
Empty git repo + the accepted ADRs.

### Exit state
- A locally bootable app: `docker compose up` brings up Postgres; `cargo run -p orbit -- api` starts the backend; `pnpm --filter frontend dev` starts the Vite SPA. The app is reachable at `http://localhost:<port>` only.
- User can sign up with email + password (argon2id), verify email (local SMTP sink or log-based token retrieval is acceptable in 0a), log in, log out.
- Landing page post-login: an empty dashboard shell with the sidebar from UX §3.1 (Portfolio / Decisions / Compliance / Account). Most links go to "próximamente" placeholders.
- First-login disclaimer modal displays (UX §8 layer 1); acceptance is recorded in `audit_log`.
- Persistent footer strip renders on every page. **Footer in Slice 0a shows only the "Esto no es asesoramiento fiscal ni financiero" copy** — no rule-set chip yet (see C-3 in open-questions-resolved).
- Cookie banner live (AEPD 2023, analytics opt-in default off).
- `/healthz` and `/readyz` endpoints respond.
- Baseline observability skeleton: structured logs via `orbit_log::event!` (JSON to stdout in 0a; off-VM shipping deferred to Slice 9). No uptime monitor yet.
- CI pipeline: `cargo test`, `cargo clippy -D warnings`, `cargo audit`, `cargo deny`, frontend `pnpm build` + unit tests + `axe` a11y smoke on the sign-in page. `deploy.yaml` is committed but disabled (`workflow_dispatch`-only) until Slice 9.
- Migrations framework in place (`orbit migrate`), `users`, `sessions`, `audit_log`, `dsr_requests` tables created per ADR-005. `orbit_app` + `orbit_support` Postgres roles provisioned in the init migration; `audit_log` is INSERT-only for `orbit_app` and SELECT-only for `orbit_support` (see ADR-015 §"Additional decisions").
- Postgres RLS scaffolded and enforced: the `Tx::for_user(user_id)` helper is the only connection-acquisition path; CI lint rejects direct `pool.acquire`.
- Response headers (CSP, X-CTO, X-Frame, Referrer, Permissions, COOP) verified against the local dev server via `curl -I`. HSTS is declared but only observable at Slice 9.
- Locale switcher (ES/EN) works at the page-chrome level (LinguiJS per ADR-009).

### Explicit non-goals
- **No cloud deploy.** No Hetzner VMs, no Caddy, no TLS/HSTS end-to-end, no nftables, no Postgres private-network binding, no offsite backups, no uptime monitor, no published legal surface. All of this moves to Slice 9.
- No grants, no vesting, no calculations, no scenarios, no sell-now. **No billing ever in v1** (v1.2 PoC scope).
- No TOTP (deferred to Slice 7 as an optional-for-all setting).
- No device/session management UI (backend exists, UI deferred to C-7 resolution).
- No market-data vendor integration, no ECB FX ingestion (ADR-006/007 pipelines not yet wired).
- No PDF/CSV export plumbing.
- No rule-set ingestion, no tax engine stub, no `rule_sets` table populated.
- No FX display on the dashboard (see C-4).

### T-shirt
**M.** Boring but critical; the shell is where architecture mistakes cost most to retrofit.

### Dependencies
- ADR-001, ADR-002, ADR-015 accepted.
- ADR-005 entity outline (for the auth tables). Full DDL is in ADR-014; Slice 0a ships `users`, `sessions`, `audit_log`, `dsr_requests` from that DDL.
- No cloud account procurement in this slice (pushed to Slice 9).

### Demo script
1. `docker compose up -d && cargo run -p orbit -- api &` and `pnpm --filter frontend dev` on a developer machine.
2. Open `http://localhost:<port>`.
3. Click "Regístrate", enter email + password.
4. Verify email (retrieve the verification link from the local SMTP sink or structured-log output).
5. Log in; see the disclaimer modal; accept.
6. Land on the empty dashboard; see the sidebar and footer.
7. Click the locale switcher; see UI swap ES↔EN.
8. Log out. Log back in. Disclaimer modal is not shown again (one-time acceptance recorded).

---

## Slice 1 — First portfolio (chosen as the first-value slice)

### Scope one-liner
A signed-in user can enter **one grant manually** and see its vesting timeline. Nothing else.

### Entry state
End of Slice 0.

### Exit state
- Sign-up wizard extends with two new required steps **after** the disclaimer modal and **before** the first grant form: (a) residency (autonomía selector + Beckham flag + primary currency), (b) "Tu primer grant" form.
- On completion the user lands on the dashboard, which now has exactly one grant tile showing: instrument, share count, vest start, a computed vested-to-date count, and a sparkline vesting curve.
- `Grants` sidebar link opens the grants list (one row, for now).
- Clicking the grant opens a grant-detail screen with the vesting timeline (cumulative curve, Gantt toggle per D-7).
- Double-trigger RSUs render in the distinct visual state ("time-vested, awaiting liquidity event") per US-003 AC #2.
- Edit-grant flow works (US-001 AC #2); validation rejects cliff > vest period (US-001 AC #3).
- País Vasco / Navarra: the residency step sets the foral-regime flag on the user's `residency_periods` row; the dashboard and grant-detail screens still work (free tier doesn't need tax math). **No tax-calc foral block is yet displayable because no tax calc exists yet** — this is important framing for the test plan.
- Beckham = Yes at this stage: stored on the residency row, not yet surfaced anywhere (no screens yet display tax math or the Beckham block).

### Explicit non-goals
- **No CSV import** (Slice 2).
- **No multiple grants UI polish** — the schema supports multiple, the list screen will render however many exist, but the first-grant wizard is single-grant only.
- **No tax numbers anywhere.** No IRPF, no cap gains, no paper gains in EUR. Grant values shown in native currency only (C-4 decision).
- **No rule-set chip in footer** (C-3).
- **No Modelo 720 banner** (no foreign-asset-value concept yet).
- **No Art. 7.p** (Slice 2).
- **No scenario modeler, no sell-now** (nav links exist but open to "próximamente" placeholders — no blurred preview state, since there's no paid gate to preview).
- **No export.**
- **No FX conversion** on the dashboard.

### T-shirt
**L.** Three net-new screens (residency step, grant form, grant detail), the vesting-derivation engine, and the double-trigger visual state are the bulk.

### Dependencies
- Slice 0 complete.
- ADR-005 `grants`, `vesting_events`, `residency_periods` tables. Full DDL not required, outline sufficient.
- Design tokens + primitives from `docs/design/style-guide.md` and `shared.css`.
- Reference screens: `grant-detail.html` (vesting timeline section), `dashboard.html` (empty-state and single-grant tile).

### Demo script
See `slice-1-acceptance-criteria.md` for the ceremony; at a high level:
1. Sign up; complete disclaimer modal.
2. Residency: select Comunidad de Madrid, Beckham = No, primary currency EUR.
3. First grant: RSU, 30,000 shares, 4-year/1-year-cliff/monthly, double-trigger = yes.
4. Land on dashboard with one grant tile and a vesting sparkline.
5. Click into grant detail; see full vesting timeline with the "awaiting liquidity event" state.
6. Edit the grant; change the vesting start date by one month; see the timeline update.
7. Add a second grant from the grants list (NSO, 10,000 shares, $8 strike). Dashboard updates.
8. Sign out. Sign back in. State is preserved.

---

## Slice 2 — Portfolio completeness (hand-entry)

### Scope one-liner
The user can hand-build a realistic portfolio — multiple grants, ESPP purchase records, Art. 7.p trips — without any tax calculation yet. **Bulk import (CSV / PDF) is deferred to Slice 9 per v1.3.**

### Entry state
End of Slice 1.

### Exit state
- **ESPP purchases** captured alongside their parent grant (backs US-008 and US-013 basis lookup). Lookback FMV input optional. Retires the Slice-1 compromise where `estimated_discount_percent` rode inside `grants.notes` JSON — a dedicated `espp_purchases` table lands instead (ADR-005).
- **Art. 7.p trip entry** (US-005 AC form side only — no calculation yet). Trips stored in `art_7p_trips`; inline checklist visible.
- **Dashboard** now renders multiple grant tiles, stacked refresh-grant cumulative view (US-003 AC #4).
- **Modelo 720 category inputs** on the profile (user-self-reports current bank-account total foreign value and real-estate total foreign value; securities are derived from grants once FX is live — which it is not yet, so the securities line still reads "calculation requires activar seguimiento fiscal"). See `modelo_720_check` calculation kind in ADR-005 — that is Slice 3's job.
- **Session/device list UI** in Account (closes C-7 backend/UI phase-gap).
- **"Tengo varios grants" link (AC-4.2.11) wording updated** to reflect Slice 8 as the target, instead of Slice 2. In Slice 2 the link still dismisses to an empty dashboard; copy explains bulk import ships late.

### Explicit non-goals
- **No CSV import. No ETrade PDF import.** Both deferred to Slice 9 per v1.3.
- Still **no tax numbers**.
- Still **no FX conversion**.
- Still **no Modelo 720 threshold-crossing alert** (the passive banner pattern ships in Slice 3).
- No billing ever in v1 (v1.2 PoC scope).
- No sell-now, no scenarios.

### T-shirt
**M.** Two new small entities (ESPP purchases, Art. 7.p trips), one list/tiles expansion, two account-side UIs. The big pre-v1.3 item (CSV import) moved to Slice 9.

### Dependencies
- Slice 1 complete.
- `espp_purchases`, `art_7p_trips` tables in Postgres (ADR-005).

---

## Slice 3 — FX pipeline + Modelo 720 passive UX

### Scope one-liner
The ECB FX ingestion pipeline stands up, EUR conversion lights up on the dashboard, and the Modelo 720 threshold UI (alert only — not the worksheet PDF yet) works. **No billing. No Stripe. No paid/free gating. Every feature is available to every account** (v1.2 PoC scope).

### Entry state
End of Slice 2.

### Exit state
- **ECB FX ingestion pipeline** per ADR-007: worker fetches the ECB eurofxref-daily.xml at ~17:00 Madrid; non-publication-day fallback walks back ≤7 days with staleness indicator; bootstrap-ingest 90-day historical on first worker startup; user overrides persisted per-calculation.
- **`fx_rates` table populated** per ADR-005.
- **Dashboard paper-gains tile** displays gains in EUR for the first time (uses ECB FX, bands at 0% / 1.5% / 3% per UX and ADR-007). Paper gains = (current price − grant price) × shares, converted to EUR. Current price is user-entered at this slice (Finnhub wires up in Slice 5 when it is decision-load-bearing).
- **Modelo 720 threshold alert** (US-007 ACs 1, 2, 4) against the user-entered totals + the grant-side securities number. The securities number requires FX — that's why M720 alert rides with the FX pipeline in the same slice.
- **Rule-set chip in footer** on pages that now carry an FX-dependent number. No tax rule-set yet — the chip surfaces ECB FX date + Orbit engine version in Slice 3. Full tax rule-set stamping starts in Slice 4.

### Explicit non-goals
- **No Stripe, no billing, no subscription state, no VAT handling, no feature matrix screen, no preview-only blurred state, no `subscriptions` table.** All of this is permanently out of v1 PoC scope (v1.2 decision).
- **No tax math on the dashboard still.** No IRPF projection.
- **No Modelo 720 worksheet PDF export** (Slice 6).
- **No scenarios, no sell-now compute** (Slices 4–5).
- **No market-data vendor** yet (current price is user-entered in this slice).

### T-shirt
**M.** The ECB pipeline is bounded (ADR-007 is explicit); M720 alert is form + threshold; paper-gains tile is a formula with band rendering.

### Dependencies
- Slice 2 complete.
- ADR-007 ECB pipeline.
- `fx_rates` table populated per ADR-005.

---

## Slice 4 — Tax engine goes live (scenario modeler first)

### Scope one-liner
The hybrid tax engine (ADR-003) ships its first calculator: the scenario modeler (US-004). Rule-set versioning (ADR-004) goes live. The ranges-and-sensitivity NFR (§7.4) activates on real tax numbers.

### Entry state
End of Slice 3.

### Exit state
- **`orbit-tax-core` primitives** (ADR-003) implemented: `Money`, `TaxResult`, `FormulaTrace`, `SensitivityBand`.
- **Spain `TaxCalculator` impl** (ADR-003): statewide + autonomía rate tables for all territorio común autonomías (§7.5). Ahorro-base tiers. Art. 7.p partial-year capped exemption (US-005). Autonomía rate selector (US-006).
- **País Vasco / Navarra block** (US-006 AC #2) displays on tax-calc screens; free-tier surfaces unchanged.
- **Beckham block** (US-004 AC #3) displays on tax-calc screens when flag = Yes.
- **Rule-set `es-2026.1.0`** authored as YAML, reviewed, published to `rule_sets` table. Content hash + AEAT guidance date stamped.
- **Scenario modeler** (US-004) fully functional. Sensitivity per US-010. Uncertainty patterns per UX §7 (C for headline, B for sub-totals, A for tabular).
- **Export traceability IDs** generated on each calculation; visible on the scenario result page. Export **artefact generation** ships in Slice 6.
- **"Recompute under current rule set"** affordance ships dormant (C-9).
- **Modelo 720 scenario-crossing alert** (US-004 AC #4 and US-007 AC #2) now fires during scenario compute.

### Explicit non-goals
- **No sell-now yet** (Slice 5).
- **No ESPP calculator wiring in scenarios** — scenarios are whole-portfolio projections; per-instrument ESPP tax treatment (US-008) rides in Slice 5 with the sell-now calculator where its basis math is load-bearing.
- **No PDF/CSV export** yet (Slice 6).
- **No UK paper-design coded artefacts** — UK remains pure paper-design per ADR-003 acceptance gate.

### T-shirt
**XL.** This is the heaviest slice. It includes rule-set authoring, the calculator core, the autonomía tables, Art. 7.p logic, scenario persistence, sensitivity rendering, and the full flag-state UX for foral/Beckham.

### Dependencies
- Slice 3 complete.
- ADR-003, ADR-004 fully implemented.
- Rule-set content authored by someone who understands 2026 AEAT guidance (owner: Ivan or contracted gestor).
- All D-* range-pattern decisions from the UX proposal locked.
- Security-engineer sign-off on audit-log retention posture (C-8).

---

## Slice 5 — Sell-now calculator

### Scope one-liner
US-013 fully delivered: post-IPO user enters lots, sees net-EUR-landing with bands.

### Entry state
End of Slice 4.

### Exit state
- **Finnhub** integration per ADR-006, **on the free/dev tier** for this slice. 15-min cache in `market_quotes_cache`. Staleness UX wired (ADR-006 + UX §4.2). Commercial-tier contract + ToS-confirmed-for-SaaS-redistribution is a Slice 9 launch-blocker (same for the Twelve Data standby contract).
- **Sell-now screen** (US-013). All ACs.
- **ESPP Spanish-tax calculator** (US-008), called by both scenario modeler retroactively (if needed) and sell-now at compute time.
- **NSO same-day exercise-and-sell** bargain-element computation (US-013 AC #4).
- **Live-update debounced compute** per UX D-4.
- **Price band + FX band rendering** per §7.4 + UX §7 Pattern C.
- **Passive Modelo 720/721 banner** on sell-now per US-013 AC #7 — static text only (D-11).
- **Rate limiting** on sell-now compute and market-data endpoints per §7.9.
- **Sell-now audit persistence**: `sell_now_calculations` rows written per session (ADR-005).

### Explicit non-goals
- **No realized-sale ledger** (explicit v1 out of scope).
- **No Modelo 100 worksheet** (explicit out of scope).
- **No streaming quotes** (explicit out of scope).
- **No vested-unexercised NSO "sell-later"** (explicit out of scope).
- **No US qualifying-disposition ESPP** (explicit out of scope).

### T-shirt
**L.** Smaller than Slice 4 because the tax-engine foundation is now in place; the slice is integration + a single new screen + one new calculator kind.

### Dependencies
- Slice 4 complete.
- Finnhub free/dev API key (sufficient for this slice).
- Finnhub commercial-tier contract signed and ToS-confirmed for SaaS redistribution (ADR-006 launch-blocker; OQ-13 escalation) — **moved to Slice 9**, since it gates real-user exposure, not local development.
- Twelve Data standby contract also in place (ADR-006 resilience) — **moved to Slice 9** for the same reason.

---

## Slice 6 — Exports, Modelo 720 worksheet, recompute-under-current

### Scope one-liner
The promise to the gestor. PDF + CSV exports, Modelo 720 worksheet, and the recompute flow that makes rule-set versioning visible to the user.

### Entry state
End of Slice 5.

### Exit state
- **PDF export** generator (US-009) per ADR-008. Per-page footer with traceability ID, rule-set version, AEAT guidance date, disclaimer. XMP metadata.
- **CSV export** with header comments carrying the same traceability (ADR-008).
- **Export dialog** (UX §4.4) with scope / format / language / disclaimer-confirm checkbox.
- **Exports list** screen with traceability IDs copyable (UX §4.4 step 4).
- **Modelo 720 worksheet export** (US-007 AC #3).
- **"Recompute under current rules"** flow wakes up when `es-2026.1.1` or later is published (C-9). Side-by-side diff per D-8.
- **Six-year export retention** policy enforced (ADR-008).

### Explicit non-goals
- No e-filing (explicit out of scope).
- No gestor-facing portal (explicit out of scope, C-15).

### T-shirt
**L.** PDF rendering (Typst or headless Chromium per ADR-001 follow-up), CSV formatting, and the recompute diff UI are each substantial.

### Dependencies
- Slice 5 complete.
- PDF library decided (ADR-001 follow-up).
- A second rule-set (`es-2026.1.1` or similar) authored so the recompute flow can be end-to-end tested — can be a synthetic version or a genuine AEAT update.

---

## Slice 7 — GDPR DSR self-service + 2FA mandatory

### Scope one-liner
The code-side compliance and security bar. **Hand-entry-only MVP gate** (internal-readiness gate shifts to end of Slice 8 after bulk import lands). Everything that requires a production environment or a published legal surface moves to Slice 9.

### Entry state
End of Slice 6.

### Exit state
- **US-011 ACs in full:** data export (access/portability) with 7-day self-service SLA / 30-day hard SLA. Two-step account deletion with 30-day grace. Rectification-request form. Restrict-processing action.
- **Account → Data & privacy screen** (UX §4.5) delivers all four DSR actions.
- **TOTP 2FA optional for every user** (v1.2 PoC scope; OQ-01 mandatory-for-paid resolution is moot now that there is no paid tier). Recovery-code flow implemented. Users can enable, disable, and reset TOTP from the Account screen.
- **Audit-log pseudonymization on erasure** verified (ADR-005 + security-engineer follow-up).
- **DPA draft** written and reviewed; **sub-processor register draft** written and reviewed. Publication is Slice 9 (once the public surface exists to publish on).
- **Breach-notification runbook draft** written; ES/EN templates drafted. Tabletop exercise is Slice 9.

### Explicit non-goals
- No additional jurisdictions.
- No additional personas.
- No additional instruments.
- **No third-party pen-test** — moved to Slice 9 because a pen-test requires the production stack (Hetzner + Caddy + nftables + real Postgres networking), not a developer laptop.
- **No DPA publication, no sub-processor list publication, no breach-notification tabletop** — all moved to Slice 9 for the same reason (public surface + operational stack).

### T-shirt
**M.** Mostly wiring known pieces together.

### Dependencies
- Slice 6 complete.

---

## Slice 8 — Portfolio bulk import

### Scope one-liner
Bulk-import tooling for brokers that Persona B actually uses: Carta + Shareworks CSV import, ETrade PDF import. Everything else deferred from Slice 2 on the import axis. No new calculation, no new jurisdiction. Internal-readiness gate.

### Entry state
End of Slice 7. Every feature surface except bulk import is live against hand-entered data.

### Exit state
- **CSV import** from Carta and Shareworks (US-002 Musts). Column-mapping preview, row-level error report, rejected-rows CSV download. 1,000-row / 5 MB cap (OQ-04). Both happy-path and malformed-row scenarios land in the Playwright E2E suite.
- **ETrade PDF import** (US-002 AC #5). Vendor-specific parser lives under `orbit-api/src/import/etrade/`; the parsing boundary is well-tested and isolated from the grants CRUD path.
- **"Tengo varios grants" link (AC-4.2.11 update)** routes to the import landing page; copy updated in ES/EN.
- **Post-import reconciliation UI**: after a successful import, Persona B lands on a review screen that shows the imported rows, lets them correct any field, and one-click commits — the same hand-entry form from Slice 1/2 is the edit fallback.
- **Audit log additions**: `grant.import.csv.*` and `grant.import.pdf.*` actions with `payload_summary` carrying only non-sensitive metadata (source, row count, rejected-row count) per SEC-101.
- **Integration tests** against real Carta + Shareworks + ETrade fixtures (owner: Ivan to supply).
- **Internal-readiness gate closes here.** Orbit is feature-complete; still no external user has touched the system.

### Explicit non-goals
- Still **no cloud deploy** (that is Slice 9).
- No new grant instruments, no new brokers beyond Carta / Shareworks / ETrade.
- No post-launch ops tooling.
- No new legal surface (that is Slice 9).

### T-shirt
**L.** CSV import was always the big one from the pre-v1.3 plan; it carries the same weight here (column mapping, validation, error reporting, reconciliation UI). ETrade PDF adds parsing complexity for a vendor-specific format.

### Dependencies
- Slice 7 complete.
- Real Carta, Shareworks, and ETrade exports as test fixtures (owner: Ivan to supply). Synthetic fixtures exercised first; real ones used for acceptance.

### Demo script
1. Start from an empty dashboard on `localhost`.
2. Upload a 30-row Carta CSV. Column-mapping preview renders; defaults accepted.
3. Confirm. Review screen shows 28 rows green, 2 rows red with per-row reasons.
4. Download the rejected-rows CSV, fix the two rows, re-upload. All 30 rows land.
5. Dashboard renders 30 grant tiles.
6. Upload an ETrade quarterly PDF as a second import; confirm the grants append (no duplicates heuristic documents the match key).
7. `audit_log` carries two `grant.import.csv.success` rows and one `grant.import.pdf.success` row; no share counts or employer names in any payload summary.

---

## Slice 9 — Production deployment & launch gate

### Scope one-liner
Stand up the Hetzner production stack, close every ADR-015 §0b item, complete the commercial/legal cutovers deferred from Slices 3/5/7, run the pen-test, and open to external users. No feature work.

### Entry state
End of Slice 8. Orbit runs end-to-end on a developer machine against Docker Compose Postgres + Vite dev server. Every feature — including bulk import — is built, tested, and polished locally. No external user has touched the system.

### Exit state — ADR-015 §0b closed
- **Hetzner Cloud Falkenstein** VMs provisioned per ADR-002 (CX22 API+worker+Caddy, CX32 self-managed Postgres). At-rest encryption enabled (S0-11).
- **Caddy TLS 1.3 + Let's Encrypt** on `app.orbit.<tld>`; HSTS observable end-to-end (S0-12, S0-13 final).
- **nftables deny-default outbound + allowlist** on the API host (S0-15). HIBP endpoint (SEC-149) and ECB endpoint explicit allowlist entries.
- **Postgres private-network binding** + `pg_hba.conf` TLS-required; `orbit_app` no BYPASSRLS, no superuser (S0-16 final shape). The `orbit_app` / `orbit_support` role split shipped in 0a carries through unchanged.
- **Secrets via systemd `LoadCredential=`** on the API host (S0-19 final shape). Local `.env` no longer in the execution path.
- **MFA enabled on every live operational account:** Hetzner, registrar, DNS / Bunny.net, email provider, Finnhub, Twelve Data (S0-20 live-account set).
- **`deploy.yaml` workflow enabled** with full-history gitleaks (S0-01 final), production GitHub Environment with manual-review reviewers (S0-06 final), SBOM uploaded to release store (S0-10 final). Atomic symlink swap per ADR-013.
- **Nightly `pg_basebackup` + WAL archive + `age`-encrypted to Hetzner Storage Box** (S0-26).
- **End-to-end restore drill** performed and dated in the runbook (S0-27).
- **Uptime monitor** (Better Stack or equivalent, EU-residency verified) pinging `/healthz`; alert route tested with a synthetic outage (S0-28).
- **Privacy policy + sub-processor register published** on the public site (S0-29, absorbing the Slice 7 deferral).
- **Incident-response runbook + AEPD 72-hour timer + ES/EN breach-notification templates** finalized (S0-30, absorbing the Slice 7 deferral).
- **Breach-notification tabletop exercise** run against the published runbook (moved from Slice 7).

### Exit state — commercial / legal cutovers
- **Finnhub commercial-tier contract signed** and ToS explicitly confirmed for SaaS redistribution of delayed quotes (ADR-006 launch-blocker; OQ-13). Twelve Data standby contract also in place (ADR-006 resilience). Both moved from Slice 5.
- **DPA published** for users (moved from Slice 7). No paid/free distinction applies since the PoC has no tier split (v1.2); whatever terms cover external usage apply to every account equally.
- **Sub-processor list published** (moved from Slice 7; joins the S0-29 privacy policy on the same page).
- **Third-party pen-test** completed against the production stack; findings resolved or risk-accepted with security-engineer sign-off (moved from Slice 7 because it requires real infra; §7.9).

### Explicit non-goals
- No new features. This slice is pure productionization + procurement + validation.
- No new personas, jurisdictions, or instruments.
- No post-launch ops tooling (auto-scaling, blue/green, canary) — out of v1 scope; the single-VM atomic-symlink model from ADR-013 is the launch shape.

### T-shirt
**L.** ~2–3 days of deploy-shaped work per ADR-015, plus external-dependency cycles that overlap: Finnhub commercial negotiation, pen-test vendor engagement and report cycle. Internal-facing work is small; external-facing work sets the calendar.

### Dependencies
- Slice 7 complete.
- **Procurement (owner: Ivan):** Hetzner, Bunny.net, Storage Box, Object Storage, domain registrar, DNS, outbound email (Postmark EU or SES EU), Better Stack or alternative uptime monitor, pen-test vendor, Finnhub commercial-tier, Twelve Data standby.
- AEPD breach-notification contact confirmed.
- Security-engineer signs off the ADR-015 §0b item set against the deployed stack (the "amber" items from 0a close here).

### Demo script
1. Merge-to-main triggers `deploy.yaml` (now enabled); atomic symlink swap rolls HEAD to `app.orbit.<tld>`.
2. Open `https://app.orbit.<tld>` from an EU IP; TLS 1.3 + HSTS verified with `curl -I` from a clean session.
3. Run the Slice 8 demo (bulk import Carta CSV + ETrade PDF) end-to-end against the production stack, followed by the Slice 7 demo (DSR export, account deletion with 30-day grace, optional TOTP enabled on the demo account).
4. Trigger a DSR export; confirm the archive lands in Object Storage with traceability stamped and that the download link honours the 7-day self-service SLA.
5. `systemctl stop orbit-api` on the production host; confirm the uptime monitor alert fires on the chosen route within its configured threshold; restart; confirm resolve-notification.
6. Perform a restore-drill from the latest `pg_basebackup` + WAL into a scratch instance; compare row counts; date the drill in the runbook.
7. Review the pen-test report; confirm no open blockers; publish privacy policy, sub-processor list, and DPA.

---

## Dependencies graph

```
[Slice 0a: local shell] → [Slice 1: first grant] → [Slice 2: portfolio fullness]
                                                         ↓
                                              [Slice 3: FX + M720 passive]
                                                         ↓
                                              [Slice 4: tax engine + scenarios]
                                                         ↓
                                              [Slice 5: sell-now (Finnhub dev tier)]
                                                         ↓
                                              [Slice 6: exports + recompute]
                                                         ↓
                                              [Slice 7: DSR + 2FA]  ← hand-entry-only MVP gate
                                                         ↓
                                              [Slice 8: bulk import (Carta/Shareworks/ETrade)]  ← internal-readiness gate
                                                         ↓
                                              [Slice 9: production deploy + pen-test + cutovers]
                                                         ↓
                                                  PUBLIC LAUNCH GATE
```

No parallelism in the critical path; this is a single-engineer v1 (ADR-001 rationale). Parallelizable items, if a second engineer ever joins, or that Ivan can interleave solo:

- Slice 6 PDF generator can start during Slice 5.
- Slice 8 fixture gathering (real Carta + Shareworks + ETrade exports) can happen any time from Slice 2 onward; collect them early so Slice 8 doesn't stall on data acquisition.
- Slice 9 pen-test vendor engagement and Finnhub commercial negotiation should start during Slice 7 or 8 because they have long external lead times; code-side Slice 9 work only begins once Slice 8 is green.
- Slice 9 procurement (Hetzner/DNS/registrar/email/uptime) can be staged during Slice 7–8 without blocking code-side work.

## Cross-slice acceptance checks

These apply to every slice from the moment they become relevant:

1. **"No es asesoramiento fiscal" disclaimer**: footer on every page from Slice 0a onward; modal at signup from Slice 0a onward; export confirm from Slice 6 onward; artefact stamping from Slice 6 onward.
2. **Ranges-and-sensitivity**: from Slice 4 onward, every projected tax number renders per UX §7 patterns.
3. **GDPR**: data-minimization in analytics applies from Slice 0a. DSR self-service is Slice 7. **Public legal surface (privacy policy, sub-processor list, DPA) is Slice 9** — before Slice 9, there is no public surface to publish on and no external user to be covered.
4. **Accessibility**: every slice's new screens must pass `axe` smoke in CI and keyboard-tab-order review.
5. **i18n**: every UI string shipped in ES first; EN fallback before the slice closes.
6. **Rule-set stamping**: every calculation from Slice 4 onward stamps `(rule_set_id, content_hash, inputs_hash, result_hash, engine_version)` per ADR-004.
7. **EU-only data plane**: no service added inside a slice without confirming EEA-only data-path (§7.2). Particular scrutiny for Finnhub (Slices 5/9) as a US-headquartered processor — SCCs + processor map updated. Dev-tier Finnhub usage in Slice 5 does not ship PII (only tickers + API key); the full processor-map sign-off is a Slice 9 gate before external users onboard. Stripe is no longer in scope (v1.2 PoC: no billing, no Stripe integration).
8. **No external user before Slice 9.** Slices 0a–8 are for the product owner's own use on `localhost` only. The moment external users (beta testers, paying customers, anyone not Ivan) need access, Slice 9 must have closed.

## Handoff

> **Next:** Slice 1 is landed; Slice 2 is the next piece of work (hand-entry portfolio completeness per v1.3). Slices 3+ can be designed lazily as each approaches. Slice 9 planning (procurement + vendor engagement) can start during Slice 7–8; Slice 9 *execution* begins only once Slice 8 is green.
