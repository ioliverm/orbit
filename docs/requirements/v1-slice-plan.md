# Orbit v1 — Implementation slice plan

| Field       | Value                                                      |
|-------------|------------------------------------------------------------|
| Version     | 1.0                                                        |
| Date        | 2026-04-18                                                 |
| Owner       | requirements-analyst (Ivan Oliver)                         |
| Sources     | `docs/specs/orbit-v1-persona-b-spain.md`, ADR-001..ADR-008, `docs/design/orbit-v1-ui-proposal.md`, `docs/requirements/open-questions-resolved.md` |
| Purpose     | Break v1 into ordered, independently shippable vertical slices. Each ends in a demo-able state. No layers; every slice crosses frontend + backend + data + ops. |

## Principles

1. **Vertical, not horizontal.** Every slice includes frontend, backend, data, and ops changes. No "build the whole backend, then the whole frontend" plan.
2. **Demo-able at the boundary.** A slice is done when a real Persona-B user could open the app and experience something they could not before.
3. **Non-goals are load-bearing.** Each slice explicitly lists what it does not ship; this is how scope creep is caught at the boundary.
4. **Respect the ADRs.** Tech stack, cloud, data-model outline, versioning scheme, FX source, vendor, and export traceability are all decided (ADR-001..ADR-008). Slices do not re-litigate these.
5. **Respect the free/paid boundary.** Slices 0–2 stay inside the free tier. Slice 3 is the first that requires a billing path. This matches the spec's gating strategy and lets the tool be usable without ever introducing payments.

## Summary table

| # | Slice | One-liner | Free/Paid | T-shirt | Demo |
|---|-------|-----------|-----------|---------|------|
| 0 | Foundation shell | Bootable app, auth, empty dashboard, CI, observability, cookie banner. | Free | M | Log in, land on empty dashboard, log out. |
| 1 | **First portfolio** | Sign up → residency → first grant → see vesting schedule. | Free | L | Persona B enters one grant, sees vesting timeline. Nothing else. |
| 2 | Portfolio completeness | Multiple grants, CSV import, dashboard tiles, Art. 7.p trip entry. | Free | L | Persona B imports 10 grants from Carta, views dashboard. |
| 3 | Paid shell + Modelo 720 alert | Billing, free-vs-paid gating, Modelo 720 passive threshold UI. | Paid (upgrade path live) | M | Free user sees paid preview-only; upgrade completes; threshold alert fires. |
| 4 | Tax engine + autonomía + scenario modeler | First tax numbers. Rule-set versioning goes live. Ranges-and-sensitivity NFR activates. | Paid | XL | Persona B runs the IPO/lockup/hold scenario and sees net proceeds with sensitivity. |
| 5 | Sell-now calculator (post-IPO leg) | Finnhub + ECB pipeline + US-013. | Paid | L | Persona B opens sell-now, enters lots, sees net-EUR-landing range. |
| 6 | Exports + Modelo 720 worksheet + recompute | Gestor PDF, CSV, traceability IDs, recompute-under-current-rules. | Paid | L | Persona B exports a scenario PDF; tests recompute after a rule-set bump. |
| 7 | GDPR DSR self-service + 2FA mandatory + pen-test | Data export, erasure, TOTP mandatory for paid, third-party pen-test. | Paid | M | User exports their data archive; deletes account; 30-day grace works. |

**Launch gate = end of Slice 7.** Slices 0–5 are internal / closed-beta-acceptable. Slice 6 is needed for the gestor promise. Slice 7 is the GDPR/security bar for public paid launch.

---

## Slice 0 — Foundation shell

### Scope one-liner
The smallest bootable Orbit that a user can sign up to and log in to. No product features.

### Entry state
Empty git repo + the accepted ADRs.

### Exit state
- A deployed app at `app.orbit.<tld>` (Hetzner, per ADR-002) reachable from an EU browser.
- User can sign up with email + password (bcrypt/argon2id), verify email, log in, log out.
- Landing page post-login: an empty dashboard shell with the sidebar from UX §3.1 (Portfolio / Decisions / Compliance / Account). Most links go to "próximamente" placeholders.
- First-login disclaimer modal displays (UX §8 layer 1); acceptance is recorded in `audit_log`.
- Persistent footer strip renders on every page. **Footer in Slice 0 shows only the "Esto no es asesoramiento fiscal ni financiero" copy** — no rule-set chip yet (see C-3 in open-questions-resolved).
- Cookie banner live (AEPD 2023, analytics opt-in default off).
- `/healthz` and `/readyz` endpoints respond.
- Baseline observability: structured logs shipped off-VM to Better Stack or equivalent; uptime monitor pings `/healthz`.
- CI pipeline: `cargo test`, `cargo clippy -D warnings`, `cargo audit`, frontend `npm run build` + unit tests + `axe` a11y smoke on the sign-in page, and a smoke `curl` against a preview deploy.
- Migrations framework in place (`orbit migrate`), `users`, `sessions`, `audit_log`, `dsr_requests` tables created per ADR-005.
- Postgres RLS scaffolded: the `Tx::for_user(user_id)` helper is the only connection-acquisition path; CI lint rejects direct pool.acquire.
- Locale switcher (ES/EN) works at the page-chrome level.

### Explicit non-goals
- No grants, no vesting, no calculations, no scenarios, no sell-now, no billing.
- No TOTP (deferred to Slice 7 when it becomes mandatory for paid).
- No device/session management UI (backend exists, UI deferred to C-7 resolution).
- No market-data vendor integration, no ECB FX ingestion (ADR-006/007 pipelines not yet wired).
- No PDF/CSV export plumbing.
- No rule-set ingestion, no tax engine stub, no `rule_sets` table populated.
- No FX display on the dashboard (see C-4).

### T-shirt
**M.** Boring but critical; the shell is where architecture mistakes cost most to retrofit.

### Dependencies
- ADR-001, ADR-002 accepted. (Today: `Proposed`; security-engineer review is on the path.)
- ADR-005 entity outline (for the auth tables). Full DDL is deferred to solution-architect second pass, but Slice 0 needs `users`, `sessions`, `audit_log`, `dsr_requests`.
- Cloud account provisioned (Hetzner, Bunny.net, Storage Box, Object Storage). This is the Ivan procurement step.

### Demo script
1. Open `https://app.orbit.<tld>` from an EU IP.
2. Click "Regístrate", enter email + password.
3. Verify email.
4. Log in; see the disclaimer modal; accept.
5. Land on the empty dashboard; see the sidebar and footer.
6. Click the locale switcher; see UI swap ES↔EN.
7. Log out. Log back in. Disclaimer modal is not shown again (one-time acceptance recorded).

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
- **No scenario modeler, no sell-now** (preview-only stubs exist in the nav but open to the free-tier blurred-layout state from UX D-9; that state is just a visual, no compute).
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

## Slice 2 — Portfolio completeness

### Scope one-liner
The user can load a realistic portfolio — multiple grants, CSV imports, Art. 7.p trip entry, ESPP purchase records — without any tax calculation yet.

### Entry state
End of Slice 1.

### Exit state
- **CSV import** from Carta and Shareworks (US-002 Musts). Column-mapping preview, row-level error report, rejected-rows CSV download. 1,000-row / 5 MB cap (OQ-04).
- ETrade PDF import (US-002 AC #5).
- **ESPP purchases** captured alongside their parent grant (backs US-008 and US-013 basis lookup). Lookback FMV input optional.
- **Art. 7.p trip entry** (US-005 AC form side only — no calculation yet). Trips stored; inline checklist visible.
- **Dashboard** now renders multiple grant tiles, stacked refresh-grant cumulative view (US-003 AC #4).
- **Modelo 720 category inputs** on the profile (user-self-reports current bank-account total foreign value and real-estate total foreign value; securities are derived from grants once FX is live — which it is not yet, so securities line is "calculation requires activar seguimiento fiscal"). See `modelo_720_check` calculation kind in ADR-005 — that is Slice 3's job.
- **Session/device list UI** in Account (closes C-7 backend/UI phase-gap).

### Explicit non-goals
- Still **no tax numbers**.
- Still **no FX conversion**.
- Still **no Modelo 720 threshold-crossing alert** (the passive banner pattern ships in Slice 3).
- No billing / paid tier yet.
- No sell-now, no scenarios.

### T-shirt
**L.** CSV import is the big one (column mapping, validation, error reporting); the rest is incremental forms and lists.

### Dependencies
- Slice 1 complete.
- `espp_purchases`, `art_7p_trips` tables in Postgres (ADR-005).
- Real Carta + Shareworks CSV exports as test fixtures (owner: Ivan to supply).

---

## Slice 3 — Paid shell + Modelo 720 passive UX

### Scope one-liner
Stripe Tax is integrated, free-vs-paid gating is live everywhere, and the Modelo 720 threshold UI (alert + worksheet stub — not the worksheet PDF yet) works.

### Entry state
End of Slice 2.

### Exit state
- **Billing** via Stripe Tax (OQ-03). Subscription upgrade flow; VAT applied per jurisdiction; invoice issued. Grace-period logic (OQ-05, 90 days read-only then soft-delete) stubbed — it only matters once users can cancel, but the subscription state machine is complete.
- **Feature matrix screen** (US-012 AC #1) listing free vs paid.
- **Preview-only state** on scenario modeler and sell-now screens per UX D-9 (`€•,•••` pattern). No compute yet, so the paid state is still "coming soon" — Slice 4/5 ship the actual engine.
- **Modelo 720 threshold alert** (US-007 ACs 1, 2, 4) against the user-entered totals + the grant-side securities number. The securities number **now requires FX** — so this slice also stands up the ECB FX ingestion pipeline (ADR-007). FX is the pre-req for any EUR-denominated number.
- **Dashboard paper-gains tile** displays gains in EUR for the first time (uses ECB FX, bands at 0% / 1.5% / 3% per UX and ADR-007).
- **Rule-set chip in footer** on pages that now carry an FX-dependent number. **Note: no tax rule-set yet** — what the chip surfaces in Slice 3 is ECB FX date + Orbit engine version. The full tax rule-set stamping starts in Slice 4.

### Explicit non-goals
- **No tax math on the dashboard still.** Paper gains = (current price − grant price) × shares, converted to EUR. No IRPF projection.
- **No Modelo 720 worksheet PDF export** (Slice 6).
- **No scenarios, no sell-now compute** (Slices 4–5).
- **No market-data vendor** yet (the dashboard paper-gains can use user-entered current price in this slice; Finnhub wires up in Slice 5 when it is decision-load-bearing).

### T-shirt
**M.** Billing is always fiddly but Stripe Tax removes the tax-compliance burden; the ECB pipeline is bounded (ADR-007 is explicit).

### Dependencies
- Slice 2 complete.
- Stripe account, Stripe Tax enabled, webhook endpoint provisioned.
- ADR-007 ECB pipeline.
- `fx_rates` and `subscriptions` tables populated per ADR-005.
- **OQ-10 procurement** — professional indemnity insurance confirmed before this slice closes (since paid-tier cap is now open).

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
- **Scenario modeler** (US-004) fully functional, paid. Sensitivity per US-010. Uncertainty patterns per UX §7 (C for headline, B for sub-totals, A for tabular).
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
- **Finnhub** integration per ADR-006. 15-min cache in `market_quotes_cache`. Staleness UX wired (ADR-006 + UX §4.2).
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
- Finnhub commercial-tier contract signed and ToS-confirmed for SaaS redistribution (ADR-006 launch-blocker; OQ-13 escalation).
- Twelve Data standby contract also in place (ADR-006 resilience).

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

## Slice 7 — GDPR DSR self-service + 2FA mandatory + pen-test

### Scope one-liner
The compliance and security bar for public paid launch.

### Entry state
End of Slice 6.

### Exit state
- **US-011 ACs in full:** data export (access/portability) with 7-day self-service SLA / 30-day hard SLA. Two-step account deletion with 30-day grace. Rectification-request form. Restrict-processing action.
- **Account → Data & privacy screen** (UX §4.5) delivers all four DSR actions.
- **TOTP 2FA mandatory for paid users** (OQ-01 resolution at v1.1). Recovery-code flow. Opt-in for free users.
- **Third-party pen-test** completed; findings resolved (§7.9).
- **DPA** published for paid users. Sub-processor list published.
- **Breach-notification runbook** exercised in tabletop (§7.2).
- **Audit-log pseudonymization on erasure** verified (ADR-005 + security-engineer follow-up).

### Explicit non-goals
- No additional jurisdictions.
- No additional personas.
- No additional instruments.

### T-shirt
**M.** Mostly wiring known pieces together. The pen-test may surface work that expands this slice; budget accordingly.

### Dependencies
- Slice 6 complete.
- Pen-test vendor engaged.
- AEPD breach-notification contact confirmed.

---

## Dependencies graph

```
[Slice 0: shell] → [Slice 1: first grant] → [Slice 2: portfolio fullness]
                                                  ↓
                                         [Slice 3: paid + FX + M720 passive]
                                                  ↓
                                         [Slice 4: tax engine + scenarios]
                                                  ↓
                                         [Slice 5: sell-now]
                                                  ↓
                                         [Slice 6: exports + recompute]
                                                  ↓
                                         [Slice 7: DSR + 2FA + pen-test]
                                                                 ↓
                                                           LAUNCH GATE
```

No parallelism in the critical path; this is a single-engineer v1 (ADR-001 rationale). Parallelizable items, if a second engineer ever joins:

- Slice 2 CSV import can run in parallel with Slice 3 billing.
- Slice 6 PDF generator can start during Slice 5.
- Slice 7 pen-test engagement runs asynchronously during Slice 5/6.

## Cross-slice acceptance checks

These apply to every slice from the moment they become relevant:

1. **"No es asesoramiento fiscal" disclaimer**: footer on every page from Slice 0 onward; modal at signup from Slice 0 onward; export confirm from Slice 6 onward; artefact stamping from Slice 6 onward.
2. **Ranges-and-sensitivity**: from Slice 4 onward, every projected tax number renders per UX §7 patterns.
3. **GDPR**: data-minimization in analytics applies from Slice 0. DSR self-service is Slice 7.
4. **Accessibility**: every slice's new screens must pass `axe` smoke in CI and keyboard-tab-order review.
5. **i18n**: every UI string shipped in ES first; EN fallback before the slice closes.
6. **Rule-set stamping**: every calculation from Slice 4 onward stamps `(rule_set_id, content_hash, inputs_hash, result_hash, engine_version)` per ADR-004.
7. **EU-only data plane**: no service added inside a slice without confirming EEA-only data-path (§7.2). Particular scrutiny for Finnhub (Slice 5) and Stripe (Slice 3) as US-headquartered processors — SCCs + processor map updated.

## Handoff

> **Next:** invoke `solution-architect` with `/Users/ivan/Development/projects/orbit/docs/requirements/v1-slice-plan.md` and `/Users/ivan/Development/projects/orbit/docs/requirements/slice-1-acceptance-criteria.md` to produce the Slice 0 + Slice 1 technical design (concrete DDL for `users`, `sessions`, `audit_log`, `dsr_requests`, `grants`, `vesting_events`, `residency_periods`; the vesting-derivation algorithm; the sign-up wizard state machine; the auth cookie / session strategy). Slices 2+ can be designed lazily as each approaches.
