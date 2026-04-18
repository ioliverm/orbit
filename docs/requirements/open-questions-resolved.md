# Orbit v1 — Open questions: analyst resolutions

| Field       | Value                                                      |
|-------------|------------------------------------------------------------|
| Version     | 1.0                                                        |
| Date        | 2026-04-18                                                 |
| Owner       | requirements-analyst (Ivan Oliver)                         |
| Sources     | `docs/specs/orbit-v1-persona-b-spain.md` §8 (OQ-01..OQ-15), `docs/design/orbit-v1-ui-proposal.md` §12 (D-1..D-11), ADR-001..ADR-008 |
| Purpose     | Close out the design-layer and spec-layer open questions to the extent an analyst can, flag the rest for product-owner escalation. |

## How to read this file

- **Decision**: the call I am making now so Slice 0/1 work is not blocked.
- **Bias**: ship > perfect. When in doubt I pick the option that lets the implementation-engineer start and that can be A/B-tested or reversed cheaply later.
- **Escalate**: `PO` = needs the product owner (Ivan) to sign; `legal` = needs a legal opinion I cannot substitute for; `security` / `solution-architect` = within their lane, I am only recording the UX/requirements implication.
- **What would flip the decision** is stated so the decision is honestly cheap to revisit.

Nothing here expands v1 scope. Where a question hinted at scope creep I have cut it out and noted the cut.

---

## Part A — UX open questions (ui-proposal §12, D-1..D-11)

### D-1. Is the headline range-first pattern (C) too visually heavy for daily use?

- **Decision (accept UX default):** Ship Pattern C on exactly two screens — sell-now headline "Neto en tu cuenta española" and scenario modeler headline "Net proceeds". Pattern B on all card-level sub-totals. Pattern A in dense tables.
- **Rationale:** Spec §7.4 is explicit that bare point estimates are forbidden. C is the only pattern that makes the range the headline read. Limiting C to one number per page caps the "alarming" risk D-1 worries about.
- **Escalate:** no.
- **What would flip:** qualitative feedback in the first 4-user usability test that users describe the range band as alarming or distrust-provoking — then degrade the headline to B with an explicit "rango" label.

### D-2. Is the disclaimer footer legally sufficient, or does legal require a banner?

- **Decision:** Ship the four-layer pattern (signup modal + per-page footer + export confirm checkbox + on-export artefact stamps). Prepare the banner variant behind a feature flag but do not ship it by default.
- **Escalate:** **legal**. Specifically — a written legal opinion that (a) the four-layer pattern satisfies CNMV / MiFID II "not investment advice" positioning (R-1), and (b) the AEPD / LOPDGDD disclosure duties are met by the footer rather than requiring a persistent top banner.
- **What would flip:** a legal written opinion requiring a top banner; the feature-flag flip is mechanical.

### D-3. Do users understand "rule set es-2026.1.0" or is that internal jargon leaking?

- **Decision (accept UX default):** Primary display is the AEAT guidance date, secondary is the semver. Format: `AEAT 15 mar 2026 · es-2026.1.0`. Both shown in the footer chip and on every export. The semver is never shown without the date to its left.
- **Escalate:** no.
- **What would flip:** usability test shows users stop reading after the date and never notice the version change signal. Remedy is to swap the typographic weights, not to remove the semver.

### D-4. Sell-now live-update vs explicit "Calculate" button?

- **Decision (accept UX default):** Live-update with 300 ms debounce. A single `aria-live="polite"` status line reads "Recalculando..." during debounce, "Actualizado a las HH:MM" after. No separate Calculate button — it would add a second trust surface on top of live numbers.
- **Escalate:** no. (Note to `solution-architect`: this has implications for market-data cache TTL and compute endpoint latency, but that is an architecture call, not a requirements call.)
- **What would flip:** compute endpoint cannot hit a 300 ms budget for typical lot sets, at which point the decision becomes "increase debounce" before "add button".

### D-5. Bilingual: parallel ES/EN or toggle?

- **Decision (accept UX default, with one constraint):** Toggle, ES primary. Tax terms (IRPF, rendimiento del trabajo, ahorro base, Modelo 720, Art. 7.p, autonomía) **remain in Spanish regardless of toggle**. The EN locale provides hover-glosses for these terms, not translations. **Constraint:** the "no es asesoramiento fiscal" disclaimer copy is shown in ES even in EN locale, followed by the EN gloss — this is a legal-surface-area reduction decision, not a UX one.
- **Escalate:** no.
- **What would flip:** legal opinion that EN disclaimer copy must legally be primary for EN-locale users; unlikely given users are Spain-tax-resident.

### D-6. "Scenario modeler not recommended on mobile" banner — paternalistic?

- **Decision (accept UX default):** Keep the non-blocking banner. Do not degrade layout; do not block. The banner wording is informative ("Esta pantalla funciona mejor en un ordenador"), not scolding.
- **Escalate:** no.

### D-7. Vesting timeline: Gantt vs cumulative curve vs both?

- **Decision (accept UX default):** Cumulative curve as primary, Gantt as a toggle. Double-trigger RSUs visually distinct (dashed fill) in both views.
- **Escalate:** no.

### D-8. "Recompute under current rules" — diff rendering?

- **Decision (accept UX default):** Side-by-side with per-line diff indicators (▲▼=). This is also the pattern the gestor export needs, so it is load-bearing twice.
- **Escalate:** no.

### D-9. Free-tier preview-only state: blur vs `€•,•••` vs empty?

- **Decision (accept UX default):** The `€•,•••` pattern with full layout rendered. Blurring is consumer-app language and conflicts with §1 of the UX doc ("don't design like a US consumer SaaS"). Empty feels punishing per the spec's "no silent half-compute" (US-012 AC).
- **Escalate:** no.

### D-10. Gestor export PDF language: ES default vs user-locale?

- **Decision (accept UX default):** ES by default, regardless of UI locale. Persona B's gestor is always a Spanish-speaking professional (§2 of UX doc). UI user can toggle to `ES+EN parallel` from the export dialog (already in UX §4.4). EN-only PDF is not offered in v1 — it has no user.
- **Escalate:** no.

### D-11. Sell-now passive Modelo 720/721 banner — static or threshold-smart?

- **Decision (accept UX default):** Static text only. Spec §4.2 explicitly puts "Modelo 720 / 721 calculations on sell-now" out of scope; a threshold check against the sell amount is a calculation by any honest reading. **This is a scope-creep filter — any "smart" variant lives in v1.1+.**
- **Escalate:** no.

---

## Part B — Spec open questions (spec §8, OQ-01..OQ-15)

### OQ-01. 2FA mandatory for paid tier?

- **Decision:** **Optional with strong nudge in v1**, mandatory in v1.1. Slice 1 (free tier) does not require 2FA to be functional; TOTP affordance should exist in the account settings.
- **Escalate:** `security-engineer` to confirm this is defensible given audit-log sensitivity.
- **Bias to ship:** making 2FA mandatory in v1 adds a support surface (lost seeds, recovery codes) before there are paid users.

### OQ-02. DPO required?

- **Decision:** Not required for v1 user volume (small MAU, no large-scale monitoring in the Art. 37(1)(b) sense; grant values are sensitive but below the DPO threshold at this scale). Publish a named privacy contact. Re-evaluate at 10k MAU or on first AEPD inquiry, whichever comes first.
- **Escalate:** `security-engineer` + `legal`. This is the one that can snap back hardest in audit.

### OQ-03. Billing provider for EU VAT?

- **Decision:** **Stripe Tax**. Reasoning: lowest integration cost, well-documented, EU-VAT-MOSS handled, sub-processor paperwork standard. Paddle-as-MoR is the fallback if VAT complexity or chargeback risk materializes.
- **Escalate:** no (within `solution-architect` lane to confirm, but default stands).

### OQ-04. CSV import size limit?

- **Decision:** **1,000 rows / 5 MB**, hard-rejecting above. Slice 1's US-001 manual-entry path handles tail cases.
- **Escalate:** no.

### OQ-05. Grace period for lapsed-paid access to existing scenarios?

- **Decision:** **90 days read-only, then soft-delete** (which means hidden but recoverable by reactivation within a further 30 days under the §7.2 soft-delete pattern, aligning the two grace clocks).
- **Escalate:** `PO` (Ivan). This is a pricing/retention policy call dressed as a requirement, and it affects churn measurement.

### OQ-06. Sensitivity range default width (±10% vs ±25%)?

- **Decision:** **±10% shown inline by default**; ±25% available via the "show more" affordance on the sensitivity table, not a toggle at the headline level (keeps the headline honest but not catastrophist). This matches the Pattern C bar rail convention.
- **Escalate:** no.

### OQ-07. Tender offers as liquidity events?

- **Decision:** **Yes for tender offers where the user actually transacts; no for secondary-market whispers**. The grant form gets a boolean `double_trigger_satisfied_by` with values `{ipo, acquisition, tender_offer_transacted}`. Documented in-product.
- **Escalate:** `PO` (Ivan) — confirm the edge case where a user participates in a company-sponsored tender for 20% of vested shares but not the rest (likely: the RSUs whose liquidity trigger was satisfied are the ones tendered, not the whole grant).

### OQ-08. Art. 7.p documentation checklist — in-product or export-only?

- **Decision (accept spec default):** Both. Short inline checklist in the trips entry screen; full checklist on export.
- **Escalate:** no.

### OQ-09. FX-rate source?

- **Decision:** **ECB daily reference rate** — already locked by ADR-007. Recording here as closed.
- **Escalate:** no.

### OQ-10. Professional indemnity insurance before paid launch?

- **Decision:** Assumed yes; this is a **launch blocker for Slice that enables paid-tier calculations** (not Slice 0 or 1).
- **Escalate:** `PO` (Ivan) — procurement task, not an analyst decision.

### OQ-11. Free tier: does it include CSV import?

- **Decision:** **Yes, CSV import is free**. It is an acquisition driver; scenarios and exports remain paid. This resolves cleanly in the Slice 2+ gating design.
- **Escalate:** no.

### OQ-12. Modelo 720 data-model granularity?

- **Decision:** The three regulatory categories (securities, bank accounts, real estate). v1 Orbit calculates only the securities category from the user's own grant data; the other two are user-entered numerical inputs used only for the threshold alert. **No bank-account or real-estate entities in v1**.
- **Escalate:** no.

### OQ-13. Specific market-data vendor?

- **Decision:** **Finnhub primary, Twelve Data standby** — already locked by ADR-006. Recording here as closed. **Launch-blocker**: Finnhub commercial-tier ToS must be verified to permit SaaS redistribution of delayed quotes before paid-tier launch (ADR-006 follow-up, carried into security review).
- **Escalate:** `security-engineer` (already in scope of their ADR-006 review); `legal` for the ToS read.

### OQ-14. Intraday-volatility band methodology?

- **Decision:** **Day's high–low range from the same vendor (Finnhub) as the quote**; fall back to prior-close ± 5% when intraday range unavailable. Formula surfaced inline in the "show formula" affordance per UX §7.
- **Escalate:** no (the analyst default in §8 already resolves this; recording as closed).

### OQ-15. ESPP purchase-date FMV source for sell-now?

- **Decision:** From the existing ESPP grant record (captured via US-008 on entry / CSV import). If missing, prompt user to enter before compute — no silent defaults. This aligns with §7.4 "unknown-input handling".
- **Escalate:** no.

---

## Part C — Unresolved requirements ambiguities I found while cross-reading

These are items the UX designed around, or the ADRs implied, but the spec did not explicitly state. Per the guardrail "don't expand v1 scope", I am either carving them explicitly out of v1 or flagging them as questions. None of them should expand the spec's scope without `PO` signoff.

### C-1. Sign-up flow sequencing: residency before first grant

- **Observation:** UX §4.1 step 2 puts the autonomía + Beckham-law flag collection **before** the first grant is entered. The spec describes US-006 (autonomía) and the Beckham flag separately and does not pin the order.
- **Decision:** Adopt the UX ordering (residency → Beckham → first grant). Rationale: foral-regime and Beckham-flag are both existential for what the tool computes; collecting them after grants means a visible "we computed the wrong thing, undo and recompute" user experience.
- **Implication for Slice 1:** The sign-up wizard needs three required steps, not one. AC reflected in `slice-1-acceptance-criteria.md`.

### C-2. Disclaimer-consent persistence

- **Observation:** UX §4.1 notes the first-login modal records consent in the audit log. The spec does not explicitly require a consent record beyond the persistent footer.
- **Decision:** Require the audit-log entry (`dsr.consent.disclaimer_accepted` or similar). It is cheap, it is needed for R-1 (CNMV positioning) defence, and ADR-005 already has an `audit_log` table ready.
- **Escalate:** no; this is a tightening, not a scope expansion.

### C-3. "Rule-set-version" visibility on free-tier screens

- **Observation:** Spec §7.1 says "UI surfaces the active rule-set version on every calculation page". Free tier has no tax calculations. The UX still shows the rule-set chip in the footer on all screens.
- **Decision:** In Slice 1 (free, no tax math), **do not show the rule-set chip**. The chip is coupled to the existence of a calculation output. Showing a rule-set chip on a page with no calculation is dishonest ambient signal and will confuse the user. Re-introduce from the first slice that produces tax numbers.
- **This is the load-bearing reason §7.1 NFRs partially do not apply to Slice 1** — see `slice-1-acceptance-criteria.md` §9.

### C-4. "Currency" concept on the dashboard before any sale

- **Observation:** The spec assumes paper-gains in EUR with FX disclosed (§7.10). The UX dashboard shows USD-denominated grant values with EUR conversions.
- **Decision:** For Slice 1, show grant values in the **grant's native currency only (USD for US-parent grants)**; no EUR conversion, no paper-gains number requiring FX. This defers the ECB FX ingestion pipeline to the slice that actually needs it. An explicit user-visible note: "Conversión a EUR disponible al activar seguimiento fiscal" (or similar). This is a **cut from Slice 1 scope** to keep the boundary clean.
- **Escalate:** `PO` (Ivan) — this is a user-visible cut and may feel regressive versus the UX mock. Recommend accepting; it only affects Slice 1 and is restored by Slice 2 (FX pipeline).

### C-5. "Ticker" required pre-IPO

- **Observation:** ADR-005 has `ticker (nullable until IPO)` on grants. UX grant form collects ticker in step 3 of onboarding. Spec does not make this explicit.
- **Decision:** Ticker is optional at grant creation; required before running sell-now (US-013). US-001 AC does not currently say "ticker required" — keep it that way.
- **Escalate:** no.

### C-6. "Employer" as a first-class entity vs a free-text field

- **Observation:** ADR-005 has `employer_name` as a string on `grants`. UX shows employer consistently per grant. Persona B §2 notes "Multiple grants common (initial + refresh)" from the same employer — which implies deduplicating employers.
- **Decision:** v1 keeps employer as a free-text string per grant. No separate `employers` entity. The stacking behaviour in US-003 works off grant rows, not employer rows. **This is a scope cut** versus what a "proper" data model would do; it is the right cut for v1.
- **Escalate:** no.

### C-7. Session / device management (§7.9)

- **Observation:** §7.9 requires "device list visible to user, revoke-session action". The UX has no screen for this. ADR-005 has a `sessions` table.
- **Decision:** Ship the backend (sessions table, revoke endpoint) in Slice 0. Ship the UI in the Slice that carries the Account/Privacy panel (Slice 2 or 3). **Flag that §7.9 will not be fully satisfied at end of Slice 1** — acceptable because Slice 1 is free-tier only and 2FA is optional (OQ-01).
- **Escalate:** `security-engineer` to confirm this phasing.

### C-8. Audit-log retention vs Slice 1

- **Observation:** §7.9 requires 6-year audit-log retention. This is architectural (ADR-005) and not a Slice 1 feature per se, but it is also **not free** — retention policy and storage tiering have cost implications (§7.11 cost note).
- **Decision:** Slice 0 stands up the `audit_log` table with a 6-year retention *policy column*. No actual tiering or cold-storage in v1; storage cost is bounded at this scale (ADR-002 gives ~€60–70/mo budget). Revisit at 5,000+ MAU.
- **Escalate:** no.

### C-9. "Recompute under current rules" — when does the rule-set history start?

- **Observation:** UX §4.4 shows a "Computed under superseded rule set es-2026.1.0" badge. This only has meaning if **two rule-set versions have been published**. v1 launches with exactly one (`es-2026.1.0`).
- **Decision:** The "recompute" affordance ships dormant in the first slice that produces tax numbers; it visually activates the first time `es-2026.1.1` or later is published. Do not synthesize a fake prior version for UI testing.
- **Escalate:** no.

### C-10. "i18n: Catalan, Euskara, Galego"

- **Observation:** Spec §8.1 assumes "full multilingual (Catalan, Euskara, Galego) is deferred". Spec §7.10 says "Languages v1: Spanish (es-ES) and English (en)".
- **Decision:** Explicitly confirmed out of v1. No action beyond recording.
- **Escalate:** no.

### C-11. Consent record for product analytics (§7.2 "analytics cookies opt-in")

- **Observation:** §7.2 requires "analytics cookies opt-in" per AEPD 2023 guidance. UX §4.1 does not show the cookie banner.
- **Decision:** Cookie banner is a Slice 0 concern (needs to be live from the moment anonymous traffic is possible). Default posture: analytics disabled until opt-in. This is a cut against the UX proposal's omission, not the spec's.
- **Escalate:** `security-engineer` (already in their lane via §7.2).

### C-12. "Export traceability ID" surfaced to the user

- **Observation:** UX §4.4 step 4 shows the traceability ID as copyable in the exports list. ADR-008 covers the generation. Spec US-009 AC #2 requires "a traceability ID that matches an entry in the user's audit log".
- **Decision:** Confirmed as a requirement for the Slice that ships exports (not Slice 1). The user-visible surface is the exports list and the PDF footer. No separate "traceability search" screen in v1.
- **Escalate:** no.

### C-13. What happens if the user changes their autonomía after calculations exist?

- **Observation:** US-006 AC #3 handles mid-year change by prompting "select a single-autonomía basis for the year (the one with >183 days)". It does not say what happens to **previously computed scenarios** stamped under the old autonomía.
- **Decision:** Previously computed scenarios retain their stamp (the whole point of rule-set versioning — reproducibility). On reopen, the user sees a notice: "Esta proyección se calculó bajo tu residencia en [Madrid]. Tu residencia actual es [Valencia]. [Recalcular con residencia actual]". Same pattern as the "recompute under current rule set" action. ADR-005's time-bounded `residency_periods` supports this structurally.
- **Escalate:** `PO` (Ivan) — confirm this UX is acceptable. This is where the spec's "v1 computes single-jurisdiction at a time" meets a real user moment, and the analyst default preserves reproducibility at the cost of some UI noise.

### C-14. Sell-now: what if the user's grant has `ticker = null` (still pre-IPO on paper but they claim it IPO'd)?

- **Observation:** The sell-now calculator assumes a ticker. If the user opens sell-now with only pre-IPO grants, the flow cannot proceed.
- **Decision:** Sell-now empty-state guides the user to edit the grant and add the ticker before opening. This is a Slice 4+ concern, not Slice 1.
- **Escalate:** no.

### C-15. "Copy-paste from gestor" workflow

- **Observation:** The spec says gestor receives the PDF. Gestor does not log into Orbit. But gestores commonly want to push numbers back (corrections, what-ifs from their side). v1 has no mechanism for this.
- **Decision:** **Explicitly out of v1.** The user, not the gestor, remains the single operator of the tool. Gestor corrections are handled by the user re-entering the corrected values and regenerating the export. Note for v1.1: "advisor collaboration" is already called out as deferred in §4.2.
- **Escalate:** no.

---

## Part D — Items that need product-owner escalation (top-of-list)

From the above, the items **only** the product owner can close:

1. **OQ-10 — Professional indemnity insurance** before paid-tier launch. This is a procurement fact, not an analyst call; it blocks the slice that activates paid-tier calculations (Slice 3+).
2. **OQ-05 — Grace period for lapsed-paid access** (90 days read-only default). Pricing/retention policy, affects churn metric design.
3. **C-4 — Cutting EUR conversion from Slice 1 dashboard.** User-visible cut versus the UX mock; analyst default is to cut for scope, but Ivan may prefer to keep and absorb the FX-pipeline work into Slice 1.

See `v1-slice-plan.md` for the slicing that these three close-outs feed into.

## Part E — Items that need legal / security escalation (for reference only; not the analyst's call)

- **D-2** (disclaimer sufficiency, legal opinion).
- **OQ-01** (2FA mandatory for paid tier — security-engineer).
- **OQ-02** (DPO requirement — security/legal).
- **OQ-13 / ADR-006 follow-up** (Finnhub ToS permits SaaS redistribution of delayed quotes — legal).
- **C-7** (session/device management phasing — security-engineer).
- **C-11** (cookie banner posture in Slice 0 — security-engineer).

None of these block Slice 0 or Slice 1 as currently scoped below.
