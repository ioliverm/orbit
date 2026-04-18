# Orbit v1 — Scoping Spec (Persona B: Pre-IPO or Post-IPO Decision-Maker, Spain)

| Field       | Value                                                      |
|-------------|------------------------------------------------------------|
| Version     | 1.1.0-draft                                                |
| Date        | 2026-04-18                                                 |
| Status      | Draft                                                      |
| Owner       | Ivan                                                       |
| Spec ID     | orbit-v1-persona-b-spain                                   |
| Supersedes  | 1.0.0-draft (2026-04-17)                                   |
| Next review | On handoff acceptance by `solution-architect`              |

---

## 1. Problem statement

Employees of venture-backed startups in Spain routinely hold equity packages with paper values between €500k and €10M+, typically denominated in US-parent shares (directly or through a Spanish subsidiary / EOR arrangement) or in a Delaware-flipped entity where the employee remains Spain-tax-resident. These employees face high-stakes, time-sensitive decisions — whether to exercise options before a priced round, whether to sell at an IPO lockup expiry, whether to change residency — with tax consequences that span two jurisdictions and calculations that neither US-centric equity tools (Carta, Shareworks) nor generic Spanish tax calculators address.

The status quo is that these employees either pay a Big 4 asesor fiscal for bespoke modeling (costly, slow, not iterative) or build their own spreadsheets (error-prone, outdated, no sensitivity analysis). Neither option supports the exploratory "what if I sold 30% at lockup and held the rest" scenario modeling that actually drives the decision. The pain is acute in the 6–18 months leading up to a liquidity event, and the cost of a mistake — early-exercising into a down-round, tripping Modelo 720 unknowingly, mis-applying Art. 7.p — is frequently measured in tens of thousands of euros.

Orbit v1 addresses this by providing a decision-support and tax-modeling tool, not a filing or advisory product. It is strictly read-only with respect to AEAT and broker systems: it calculates, visualizes, and exports worksheets that the user or their gestor can act on. v1 is scoped narrowly to one persona (Pre-IPO **or Post-IPO (if employer has since IPO'd)** Decision-Maker, Spain-resident, receiving US or Delaware-flipped equity) and to the instruments and tax regimes that actually apply to that persona. Everything else — other personas, other countries, e-filing, licensed advice — is out of scope for v1.

Persona B is intentionally defined as a **lifecycle**, not a point-in-time snapshot: the same individual may enter Orbit pre-IPO, experience a liquidity event, and continue to use Orbit post-IPO to decide when and how much to sell. v1.1 adds a **sell-now calculator** (see §5.13 US-013) that serves the post-IPO leg of that lifecycle. It remains decision-support, stateless-ish, and does not introduce a realized-sale ledger.

---

## 2. v1 persona profile

**Persona B — Pre-IPO OR Post-IPO (if employer has since IPO'd) Decision-Maker, Spain-resident.**

Persona B is a *lifecycle* persona. The same individual may be pre-IPO today and post-IPO next year; v1 supports both legs without requiring the user to switch personas.

- **Residency:** Spain tax resident under Art. 9 LIRPF (>183 days / center of economic interests / habitual residence of spouse). Territorio común autonomías only for v1; País Vasco and Navarra foral regimes are flagged as unsupported.
- **Employment patterns:**
  - **B1**: Employed by a US company either directly (rare), through a Spanish subsidiary, or through an EOR (Employer of Record, e.g., Deel, Remote). Compensation includes USD-denominated RSUs and/or NSOs on the US-parent.
  - **B4**: Employed by a Delaware-flipped startup where the HoldCo is US-Delaware but the employee and often the operating subsidiary are in Spain. Common in Spanish SaaS / fintech that raised from US VCs.
- **Lifecycle state:**
  - **Pre-IPO**: employer is still private; user is modeling exercise / hold / sell-at-future-liquidity decisions. Scenario modeler (US-004) is the primary tool.
  - **Post-IPO (same employer has since IPO'd)**: user holds vested RSUs / ESPP shares / NSO-eligible-for-same-day-exercise on a now-public US employer. Sell-now calculator (US-013) is the primary tool.
- **Instruments held:** RSUs (often double-trigger pre-IPO; single-trigger / vested post-IPO), NSOs (the majority of option grants from US entities to non-US employees, since ISOs require US residency), occasionally ESPP participation.
- **Paper value range:** €500k–€10M+. Multiple grants common (initial + refresh).
- **Decision horizon:** 6–24 months to a liquidity event (pre-IPO leg) **or** weeks-to-quarters around a discrete sell decision (post-IPO leg).
- **Sophistication:** Technical / analytical. Comfortable with spreadsheets. Will not accept a black box; wants to see formulas, inputs, and sensitivity.
- **Existing tools:** Carta or Shareworks for cap-table view, Spanish gestor for annual filing, ad-hoc spreadsheets for scenarios. No integrated view.

### 2.1 Illustrative scenarios (fictional-but-plausible)

**Pre-IPO leg.** María, 34, senior staff engineer at a Series C US-Delaware-flipped B2B SaaS company, lives in Madrid and is tax-resident in Comunidad de Madrid. She has 90,000 vested NSOs with an $8.00 strike, the most recent 409A values the common at $35.00, and the last preferred round implied a $55.00 preferred price. She also has 30,000 unvested double-trigger RSUs from a refresh grant last year (4-year vest, no cliff, liquidity event required). She spent six weeks of the prior year working from the company's NYC office on a specific client engagement and is wondering whether Art. 7.p applies. Her husband earns €95k locally, they own their home, and she is considering: (a) early-exercising a slice of her NSOs to start the capital-gains holding clock, (b) waiting for the IPO and selling 30% at lockup, or (c) relocating to Portugal or the US before the event. She does not want advice; she wants to see the numbers under each path, with clear assumptions, so she can decide with her gestor.

**Post-IPO leg (same persona, one year later).** María's employer IPO'd 14 months ago. Her double-trigger RSUs have now released and she holds a mix of (i) RSU shares with basis = FMV-at-release, (ii) ESPP shares purchased at a 15% discount (discount already taxed as rendimiento del trabajo at purchase, basis = FMV-at-purchase for Spain-tax), and (iii) vested NSOs she has not yet exercised. The stock is at $48. On a Tuesday morning she wants a 5-minute answer to "if I sell 3,000 RSU shares + 500 ESPP shares + do a same-day exercise-and-sell on 1,000 NSOs right now, roughly how many euros land in my Spanish bank account after US fees, US withholding (if any), FX, transfer fees, and Spanish tax on the gain?" — with honest uncertainty bands, not a false point estimate. That's what the sell-now calculator (US-013) answers.

---

## 3. Success metrics (first 6 months post-launch)

| # | Metric                                | Target                                             | Measurement                                                        |
|---|---------------------------------------|----------------------------------------------------|--------------------------------------------------------------------|
| 1 | Activation                            | ≥ 60% of signups complete first grant + first scenario within 7 days | Funnel event: `grant_created` AND `scenario_run` within 7d of signup |
| 2 | Week-4 retention (free tier)          | ≥ 35%                                              | DAU/WAU cohort analysis                                             |
| 3 | Free-to-paid conversion               | ≥ 5% of activated users within 60 days             | Subscription event / activated cohort                               |
| 4 | Scenario-modeling usage               | ≥ 3 scenarios run per paying user per month        | Event count: `scenario_run` per paid user                           |
| 5 | Qualitative "avoided a mistake" signal| ≥ 15 user-reported testimonials in first 6 months  | In-app feedback + post-cancellation survey + support emails         |

Metrics are tracked per the GDPR posture in §7.2 (pseudonymous product analytics only, no grant values in event payloads).

---

## 4. Scope

### 4.1 In scope (v1)

| Area                | Included                                                                                                                                                                                                                 |
|---------------------|----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------|
| Instruments         | RSUs (single- and double-trigger), NSOs, ESPP. ISOs are accepted as input but mapped to NSO treatment for Spanish-tax purposes, with a note that US-side treatment differs.                                              |
| Vesting             | Standard 4-year schedules, 1-year cliffs, monthly/quarterly vesting cadence, double-trigger RSUs (time + liquidity event), refresh-grant stacking.                                                                        |
| Tax — work income   | IRPF on rendimiento del trabajo at vest (RSU) / at exercise (NSO / ESPP discount). Statewide + autonomía rate tables for territorio común.                                                                                |
| Tax — capital gains | Ahorro base tiers (19 / 21 / 23 / 27 / 28%) applied at sale. Cost basis includes FMV-at-vest (RSU) or FMV-at-exercise (NSO) plus strike paid.                                                                              |
| Art. 7.p            | Exemption up to €60,100/year for work performed abroad, pro-rata by days worked abroad for a foreign employer benefit. User enters trip-level data; tool calculates eligibility and exempt portion.                       |
| Beckham Law         | Awareness flag only (binary "are you under the impatriate regime?") — if yes, show a clear "v1 does not compute Beckham-regime outputs, see your asesor fiscal" state.                                                   |
| Modelo 720          | Threshold alert (€50k foreign assets in any of the three categories) and worksheet export. No e-filing.                                                                                                                   |
| Autonomía selector  | Territorio común autonomías with 2026 rate tables. País Vasco and Navarra show "not supported v1" rather than producing numbers.                                                                                          |
| Gains calculation   | (a) Unrealized paper gains, (b) realized tax events at vest/exercise/sale, (c) scenario modeling ("IPO at $X, sell N% at lockup, hold rest"). (c) is the v1 differentiator.                                               |
| Sell-now calculator | Post-IPO decision-support: enter vested RSUs + ESPP shares + NSO same-day-exercise lots, see net EUR landing in Spanish bank at current (15-min delayed) price and ECB FX mid + user-adjustable spread, with price and FX sensitivity bands. Stateless-ish (no realized-sale ledger). See US-013. |
| Data entry          | Manual grant entry; CSV import from Carta and Shareworks (documented column mappings).                                                                                                                                    |
| Exports             | CSV and PDF worksheets for user / gestor consumption. Every export stamped with rule-set version + calculation date + input snapshot.                                                                                     |
| Monetization        | Freemium. Free = portfolio tracking, vesting visualization, basic paper-gains view. Paid = tax projections, scenario modeling, exports.                                                                                   |
| Disclaimers         | Aggressive, non-dismissable on first use, persistent footer on all calc outputs: "Esto no es asesoramiento fiscal ni financiero. Consulta con tu asesor fiscal."                                                          |

### 4.2 Out of scope (explicit, v1)

| Area                                      | Why it is excluded                                                                    |
|-------------------------------------------|---------------------------------------------------------------------------------------|
| US-side AMT / ISO modeling                | Persona is Spain-resident; US treatment left to user's US CPA.                        |
| AEAT e-filing (Modelo 100 / 720 / D-6)    | Regulatory + liability exposure; out of MVP risk appetite.                            |
| Realized-sale ledger / FIFO lot tracking  | v1 sell-now is decision-support only, not a tax-reporting system. No persisted post-sale lot history.                 |
| Modelo 100 worksheet (realized sales)     | Deferred; would require the ledger + FIFO lot tracking above.                         |
| Live / streaming market-data quotes       | 15-minute delayed quotes are sufficient for decision-support; streaming adds cost + licensing complexity.              |
| Vested-but-unexercised NSO "sell-later" (i.e., exercise now, hold, sell later) | Deferred. v1 supports NSO *same-day* exercise-and-sell only; holding exercised shares across days introduces basis = FMV-at-exercise tracking that belongs with the realized-sale ledger. |
| US qualifying-vs-non-qualifying ESPP disposition modeling (US-side) | Persona is Spain-resident; US-side treatment left to user's US CPA. Spain-side ESPP logic is fully in scope. |
| Modelo 720 / 721 calculations on sell-now | Passive banner only; no threshold check against the sell amount. Full Modelo 720 story (US-007) for the pre-IPO flow remains as-is. |
| Full wealth tax calc (Patrimonio / Solidaridad) | Autonomía-by-autonomía complexity, unstable regime; deferred to v1.1/v2.          |
| Full US–Spain treaty FTC mechanics        | Requires parallel US-side calc; scope explosion.                                      |
| Multi-jurisdiction per user per year      | v1 computes single-jurisdiction at a time (data model supports time-bounded residency; engine does not). |
| PSU / performance vesting                 | Deferred.                                                                              |
| Early-exercise with §83(b)                | Deferred; US-mechanics-heavy.                                                          |
| Phantom shares / SARs                     | Deferred.                                                                              |
| Spanish-law equity instruments            | Different legal construct; deferred.                                                   |
| Licensed / personalized financial advice  | CNMV/MiFID II exposure. Orbit is decision-support, not advisory.                       |
| Broker / cap-table API integrations       | Manual + CSV only in v1.                                                               |
| OCR / document ingestion                  | Deferred.                                                                              |
| Countries other than Spain                | UK is paper-design only (no code).                                                     |
| Native mobile apps                        | Web-only in v1.                                                                        |
| Multi-user / advisor collaboration        | Single-user accounts only.                                                             |
| Other personas (first-grant, RSU public-co, multi-company veteran) | Scoped for v1.1+.                                                    |
| País Vasco / Navarra foral regimes        | Explicit "not supported v1" state; flag user rather than produce wrong numbers.        |
| Ley de Startups €50k option exemption     | Deferred to v1.1.                                                                      |

### 4.3 Explicitly deferred (v1.1 / v2 backlog)

- Ley de Startups €50k/year startup-option exemption calculation.
- Full Impuesto sobre el Patrimonio + Impuesto de Solidaridad a las Grandes Fortunas calculation.
- Full US–Spain treaty FTC mechanics with parallel US-side calc.
- Basque / Navarra foral regimes.
- Spanish-law equity instruments.
- Ley de Startups impatriate regime variant (Beckham successor cases).
- PSU, early-exercise §83(b), phantom / SAR.
- Broker / cap-table API integrations, OCR ingestion.
- Additional countries (UK first, following the paper-design ADR).
- Additional personas (A: first-grant, C: public-co RSU holder, D: multi-company veteran).
- Realized-sale ledger, FIFO lot tracking, Modelo 100 worksheet, live streaming market-data, vested-unexercised NSO sell-later flow, US ESPP qualifying-disposition modeling, Modelo 720/721 calculations within sell-now.

---

## 5. User stories

All stories follow INVEST. Acceptance criteria use Given / When / Then. Priorities: **Must** (required for v1 launch), **Should** (strongly desired), **Could** (nice-to-have for v1, acceptable to cut).

### US-001 — Create and manage grants manually

**Priority:** Must

> As a Pre-IPO Decision-Maker, I want to enter my equity grants by hand, so that I can model my position without needing any broker integration.

**Acceptance criteria**

- Given a signed-in user on the empty-state dashboard, When they click "Add grant" and fill in instrument type, grant date, share count, strike (if applicable), vesting schedule, and cliff, Then the grant is saved and appears on the dashboard with a computed vested-to-date count.
- Given a user editing a grant, When they change the vesting start date, Then all derived vested/unvested counts and downstream calculations update and the change is recorded in the audit log.
- Given a user entering a grant with a cliff longer than the total vesting period (edge case), When they submit, Then the form rejects the input with an inline validation error and no partial save occurs.

### US-002 — Import grants from Carta / Shareworks CSV

**Priority:** Must

> As a user with many grants, I want to import my equity position from a Carta, ETrade or Shareworks export, so that I don't re-type dozens of rows.

**Acceptance criteria**

- Given a valid Carta CSV export, When the user uploads it and confirms the column-mapping preview, Then all grants are imported and the user is shown a summary (N imported, 0 errors).
- Given a Shareworks CSV with a recognised schema, When uploaded, Then the tool maps columns without requiring manual mapping and imports successfully.
- Given a malformed Carta export (missing required column, corrupted rows, unrecognised instrument type), When uploaded, Then the tool shows a row-level error report, imports only the valid rows, and the user can download a CSV of rejected rows with error reasons. No silent partial state.
- Given a CSV over the size limit (TBD in §8), When uploaded, Then the tool rejects it with a clear message.
- Given a valid ETrade grant document export (PDF), When the user uploads it, Then all grants are imported and the user is shown a summary (N imported, 0 errors).

### US-003 — Visualise a vesting schedule including double-trigger RSUs

**Priority:** Must

> As a user holding double-trigger RSUs, I want to see a vesting timeline that clearly distinguishes "time-vested but not liquidity-vested" shares, so that I don't mistake paper-vested RSUs for actually taxable income.

**Acceptance criteria**

- Given a standard 4-year / 1-year-cliff NSO grant, When viewed on the vesting timeline, Then the monthly cliff and post-cliff tranches are shown on a date axis with a cumulative vested line.
- Given a double-trigger RSU grant where the liquidity event has not occurred, When viewed, Then the timeline shows time-vested shares in a distinct visual state ("time-vested, awaiting liquidity event") and the tool asserts zero taxable income to date for that grant.
- Given the user enters a hypothetical liquidity-event date in scenario mode, When applied, Then the time-vested RSUs at that date flip to "both triggers satisfied" and a projected taxable event is computed — but only within the scenario context, not persisted as realized.
- Given multiple stacked refresh grants, When viewed together, Then a combined cumulative-vesting chart is shown with per-grant drill-down.

### US-004 — Model a scenario: "IPO at $X, sell N% at lockup, hold rest"

**Priority:** Must (v1 differentiator)

> As a user facing a liquidity event, I want to model a scenario with IPO price, lockup sell percentage, and hold behaviour, so that I can compare tax outcomes across strategies.

**Acceptance criteria**

- Given a user with at least one grant, When they open the scenario modeler, set IPO date, IPO price, lockup duration, sell-at-lockup percentage, and FX rate assumption, Then the tool returns (a) projected work-income IRPF at vest/exercise trigger, (b) projected ahorro-base capital gains at sale, (c) net proceeds after tax, (d) sensitivity ranges for ±10% and ±25% IPO price movements.
- Given the scenario inputs, When outputs are displayed, Then each numeric output shows: the rule-set version, the AEAT guidance date it is based on, the formula, and a persistent "no es asesoramiento fiscal" footer.
- Given a scenario where the user has selected "Beckham Law: yes" in profile, When they attempt to run a scenario, Then the tool shows an informational block "v1 does not compute Beckham-regime outputs — consult your asesor fiscal" and does not produce numbers that assume general regime.
- Given a scenario that would cross the Modelo 720 €50k threshold on foreign assets for the scenario year, When computed, Then a prominent informational alert appears on the results page linking to the Modelo 720 worksheet.
- Given a scenario input that is logically impossible (e.g., sell percentage > 100%, negative price), When submitted, Then the form rejects with inline validation.

### US-005 — Apply Art. 7.p exemption with partial-year days-abroad data

**Priority:** Must

> As a user who worked abroad during part of the year for the benefit of a non-resident entity, I want to input my days-abroad and qualifying trips, so that the tool computes the Art. 7.p exempt portion up to €60,100.

**Acceptance criteria**

- Given a user who adds qualifying trips (destination, dates, purpose, employer-benefit entity), When annual IRPF is computed, Then the Art. 7.p exempt portion is pro-rated by qualifying days and capped at €60,100/year.
- Given a user with qualifying days but whose employer is a Spanish entity benefitting a foreign group company, When they mark the "benefit accrues to foreign group entity" flag and supply documentation reminder, Then the tool proceeds with the exemption calculation and surfaces a "documentation required" checklist in the export.
- Given zero qualifying days, When computed, Then the exemption is 0 and is not shown as a line item (to avoid user confusion).
- Given qualifying days that would imply more than €60,100 exempt, When computed, Then the exemption is capped, the cap is shown explicitly, and the remainder is taxed normally.
- Given overlapping trips or trips entirely within Spain (edge case), When submitted, Then the tool rejects with a validation error or auto-deduplicates and warns the user.

### US-006 — Select autonomía and apply correct IRPF rate table

**Priority:** Must

> As a user in a specific Spanish autonomía, I want to select my autonomía of residence, so that the IRPF calculation uses the correct combined statewide + autonomía rate tables.

**Acceptance criteria**

- Given a user on onboarding, When they select any territorio común autonomía (e.g., Madrid, Cataluña, Valencia), Then the tool uses that autonomía's 2026 rate table combined with the statewide brackets for all IRPF calculations.
- Given a user who selects País Vasco or Navarra, When they attempt to proceed, Then the tool shows a clear "Foral regime not supported in v1 — your calculation would be incorrect. See your asesor fiscal." state and does not produce tax numbers. The user can still use portfolio-tracking and vesting-visualization (free-tier) features.
- Given a user who changes autonomía mid-year (e.g., moved from Madrid to Valencia in July), When they indicate the change date, Then v1 prompts the user to select a single-autonomía basis for the year (the one with >183 days) with a note that proper split-year handling is deferred.
- Given a user with no autonomía selected, When they attempt to run a tax calculation, Then the tool blocks with "Select your autonomía to continue."

### US-007 — Modelo 720 threshold alert

**Priority:** Must

> As a user whose foreign assets may cross €50k, I want the tool to alert me on approach or crossing of the Modelo 720 threshold, so that I don't miss the filing obligation.

**Acceptance criteria**

- Given a user's total foreign-asset value (as entered) is below €50k across all three Modelo 720 categories, When the portfolio is viewed, Then no alert is shown.
- Given the user's foreign-asset value in any category (securities, bank accounts, real estate) crosses €50k in a scenario or in realized state, When the portfolio or scenario is viewed, Then a prominent alert is shown with category, approximate value, and a link to a Modelo 720 worksheet export.
- Given the user exports the Modelo 720 worksheet, When the PDF/CSV is generated, Then it contains category breakdown, per-asset detail, and a disclaimer that Orbit does not e-file.
- Given FX-rate volatility that would push a near-threshold position over the line, When the portfolio is viewed, Then the alert shows the FX assumption used and a sensitivity note ("at current FX: €48,500; at ±5% FX: €46k–€51k").

### US-008 — Handle ESPP with Spanish-tax treatment including discount

**Priority:** Should

> As a user participating in an ESPP, I want to model the discount and lookback treatment in Spanish terms, so that I understand the work-income vs capital-gains split at purchase and sale.

**Acceptance criteria**

- Given an ESPP grant with purchase price, FMV at purchase, and (optionally) FMV at offering-start for lookback plans, When the purchase is recorded, Then the discount is treated as rendimiento del trabajo at purchase and the cost basis for capital-gains purposes is FMV-at-purchase.
- Given a subsequent sale, When computed, Then the gain/loss is computed against FMV-at-purchase basis and routed through the ahorro-base tiers.
- Given an ESPP plan where lookback data is missing, When computed, Then the tool asks for the missing input and, if unavailable, computes on purchase-date FMV with a clear warning and sensitivity note.
- Given a qualifying-vs-non-qualifying distinction in the US (which affects US-side but not Spanish-side), When flagged, Then the tool shows an informational note that US-side treatment may differ and is out of scope.

### US-009 — Export a calculation worksheet for the user's gestor

**Priority:** Must

> As a user working with a Spanish gestor, I want to export a PDF and CSV worksheet of my calculations, so that my gestor can review and use the numbers in my annual filing.

**Acceptance criteria**

- Given any computed calculation (current year, scenario, or Modelo 720), When the user clicks "Export", Then a PDF and CSV are generated containing: inputs, formula, intermediate values, final result, rule-set version, AEAT guidance date, computation timestamp, user identifier (email), and the non-advice disclaimer.
- Given the export is generated, When inspected, Then it contains a traceability ID that matches an entry in the user's audit log (so a later dispute can reconstruct exactly what was computed).
- Given a free-tier user, When they attempt export, Then they are shown the paid-tier upgrade prompt (exports are paid).
- Given an export generated under rule-set version X, When viewed six months later under rule-set version Y, Then the export's stamped version clearly indicates it was computed under version X; the tool offers a "recompute under current rules" action.

### US-010 — Show ranges and sensitivity, never bare point estimates

**Priority:** Must

> As a user making a high-stakes decision, I want to see a sensitivity range around every tax number, so that I understand the uncertainty in my position.

**Acceptance criteria**

- Given any projected tax number based on a 409A, FMV, or FX-rate input, When displayed, Then the UI shows a central estimate and a range reflecting ±10% on the key input, with the key input named.
- Given a realized (past) tax event with deterministic inputs, When displayed, Then the range is collapsed to a point estimate but the formula and inputs are still expandable.
- Given a scenario with multiple uncertain inputs, When displayed, Then a one-at-a-time sensitivity table is shown for the top 3 drivers (e.g., IPO price, FX, holding period).
- Given the user hovers or clicks "show formula" on any number, When activated, Then the exact formula, inputs, and rule-set version are shown inline.

### US-011 — GDPR / LOPDGDD data-subject self-service

**Priority:** Must

> As a user subject to GDPR, I want to export my data and delete my account, so that my data-subject rights are respected.

**Acceptance criteria**

- Given a signed-in user on the account-settings page, When they click "Export my data", Then within the SLA (see §7.2) they receive a machine-readable archive of all their grants, scenarios, calculations, exports, and audit-log entries.
- Given a signed-in user who clicks "Delete my account", When they confirm (two-step), Then their personal data and grant data are deleted per the retention policy in §7.2; anonymised aggregate analytics may be retained.
- Given a user who submits a rectification request, When processed, Then the change is applied and noted in the audit log with timestamp and actor.
- Given a user who requests processing restriction, When submitted, Then calculations and exports are suspended but data retained until the restriction is lifted.

### US-013 — Sell-now calculator (post-IPO leg of Persona B)

**Priority:** Must (new in v1.1)

> As a Spain-resident employee of a now-public US employer holding vested RSUs, ESPP shares, and/or vested NSOs, I want to see what lands in my Spanish bank after selling now — with honest uncertainty bands on both price and FX — so that I can decide whether to sell today.

**Scope reminder.** This is **decision-support only**. The calculator is stateless-ish: the user's grants (share counts, grant dates, basis where known) come from their existing Orbit portfolio; the sell-now inputs below are entered per-session and the *output* is not persisted as a realized sale. There is no lot-level FIFO, no Modelo 100 worksheet, no realized-sale ledger.

**Inputs**

Per sell-now session, the user specifies:

- **Per lot to sell**: share count, instrument type (RSU / ESPP / NSO-same-day).
- **For NSO same-day exercise-and-sell lots only**: strike price (USD/share).
- **User price override** (optional): USD/share. If blank, use the 15-minute delayed quote from the configured market-data vendor.
- **User FX-spread override** (optional): percentage applied to ECB daily reference rate. Default 1.5%.
- **User broker-fee override** (optional): single field, absolute USD or % of gross. Default 0. Covers commission + SEC/TAF/FINRA fees combined.
- **User US-withholding override** (optional): single field, absolute USD or % of gross. Default 0. Tooltip explains W-8BEN + US–Spain treaty treatment of capital gains (generally no US withholding on cap-gain portion for Spain treaty residents) and notes RSU wage-income was already withheld at vest.
- **User transfer-fee override** (optional): single field, absolute EUR. Default 0. Covers receiving-bank / SWIFT / correspondent-bank fees.

**Outputs (every number shown with a sensitivity band per US-010 + §7.4)**

- Gross proceeds USD (per lot + total).
- US fees USD (from user broker-fee + user US-withholding fields).
- Net USD after US fees and withholding.
- Applied FX rate = ECB daily reference mid × (1 − user FX spread), with bands computed at **spread = 0%** (best case, best-execution wholesale) and **spread = 3%** (worst case, retail bank).
- Applied price bands: central = 15-min delayed quote (or user override); band = intraday volatility range (methodology per OQ-14).
- Gross EUR at each of the three FX rates.
- Transfer-fee deduction EUR.
- **Net EUR landing in Spanish bank** — shown as a range, not a point.
- Spanish-tax-estimate line items per instrument (work-income at exercise for NSO same-day; cap gain at sale for RSU / ESPP / NSO same-day where applicable). Cap-gain estimate uses ahorro-base tiers per §7.1 rule set.
- Net EUR **after Spanish tax estimate** (also as a range) — this is the "what actually lands after you reconcile with your gestor next year" number.

**Acceptance criteria**

- Given a post-IPO user with at least one vested RSU, ESPP, or NSO-eligible lot in their Orbit portfolio, When they open the sell-now calculator, Then the tool pre-populates the price field with the 15-minute delayed quote for the configured ticker and pre-populates the FX field with today's ECB daily reference rate, each labelled with source + timestamp.
- Given the user enters a share count and selects instrument type RSU, When computed, Then the tool treats cost basis as FMV-at-vest (from the existing grant record), computes cap gain = (sell price − basis) × shares, and routes the gain through ahorro-base tiers.
- Given the user enters an ESPP lot, When computed, Then the tool treats the discount as already taxed at purchase as rendimiento del trabajo (no double-count), uses purchase-date FMV as basis, computes cap gain = (sell price − purchase-date FMV) × shares. US qualifying-disposition treatment is NOT modeled; an informational note reminds the user that US-side treatment may differ and is out of scope.
- Given the user selects instrument type NSO same-day exercise-and-sell and enters a strike price, When computed, Then the tool computes bargain element = (FMV-at-exercise − strike) × shares as rendimiento del trabajo at exercise, cap gain at sale = 0 (because same-day sale means sell price ≈ FMV-at-exercise), and shows both lines separately.
- Given the user overrides the price, FX spread, broker fees, US withholding, or transfer fees, When computed, Then the override replaces the default and the output is recomputed; the UI clearly labels that a user override is in effect.
- Given every output number, When displayed, Then the UI shows central estimate + price band + FX band (bands at FX spread 0% and 3% per §7.4, intraday volatility range on price), and the formula is expandable per US-010.
- Given the output page is rendered, When displayed, Then a **non-dismissable "Esto no es asesoramiento fiscal ni financiero"** disclaimer is persistent at the footer, and a passive static banner reads: *"Heads up — if your total US-held securities exceeded €50k at any point last year you may owe Modelo 720; Modelo 721 (virtual-asset reporting) may apply if relevant. Orbit does not file these."* No calculation, no threshold check against the sell amount.
- Given the market-data vendor is unavailable or the quote is stale beyond a defined threshold, When the user opens the calculator, Then the tool surfaces a clear "quote unavailable — enter price manually" state with the last-known quote timestamp, and does not silently compute with stale data.
- Given a free-tier user opens the calculator, When attempting to compute, Then they see a preview-only state with the upgrade CTA (consistent with US-012 gating; sell-now is a paid feature).
- Given a user under Beckham Law (OQ-flag), When opening the calculator, Then the tool shows the same "v1 does not compute Beckham-regime outputs — consult your asesor fiscal" block as elsewhere and does not produce tax numbers that assume the general regime.
- Given the user inputs logically impossible values (share count > vested count for that lot, negative price, negative share count, NSO lot with strike ≥ current price producing negative bargain element), When submitted, Then the form rejects with inline validation.

**Tax-treatment notes per instrument (Spain-side only; v1 scope)**

| Instrument | Treatment at exercise/purchase | Basis for cap-gain on sale | Cap-gain routing |
|---|---|---|---|
| **RSU** (already vested, basis known) | No event at sale-trigger (already happened at vest as rendimiento del trabajo). | FMV-at-vest (from grant record). | (sell price − FMV-at-vest) × shares, through ahorro-base tiers. |
| **ESPP** (already purchased, Spain-side only) | Discount already taxed at purchase as rendimiento del trabajo. No double-count here. | FMV-at-purchase. Lookback handling per US-008. | (sell price − FMV-at-purchase) × shares, through ahorro-base tiers. US qualifying-disposition NOT modeled. |
| **NSO same-day exercise-and-sell** | Bargain element = (FMV-at-exercise − strike) × shares, treated as rendimiento del trabajo at exercise. | FMV-at-exercise (≈ sell price, same-day). | Cap gain ≈ 0 by construction. |

**Out of scope for US-013** (reiterated for clarity): realized-sale ledger, FIFO lot tracking, Modelo 100 worksheet, live streaming quotes, vested-unexercised NSO sell-later (exercise now / hold / sell later), US qualifying-disposition ESPP modeling, Modelo 720/721 calculations (passive banner only).

### US-012 — Freemium gating and upgrade flow

**Priority:** Must

> As a free-tier user, I want clear visibility into which features are paid, so that I can decide whether to upgrade without surprise paywalls mid-task.

**Acceptance criteria**

- Given a free-tier user viewing the feature matrix, When displayed, Then free and paid features are clearly labelled; the user can see sample screenshots of paid features.
- Given a free-tier user who reaches a paid feature (e.g., scenario modeler), When they attempt to run, Then the modeler is visible in a preview-only state and the upgrade CTA is shown; no calculation is silently half-computed.
- Given a paying user whose subscription lapses, When they sign in, Then previously-created scenarios are retained read-only for a defined grace period (TBD in §8); new scenarios require reactivation.
- Given a user in the EU/EEA subject to VAT, When they upgrade, Then the appropriate VAT is applied and an invoice is issued (billing-provider-mediated; the provider is a §8 open question).

---

## 6. Story-to-priority summary

| ID     | Title                                                | Priority |
|--------|-------------------------------------------------------|----------|
| US-001 | Create and manage grants manually                     | Must     |
| US-002 | Import grants from Carta / Shareworks CSV             | Must     |
| US-003 | Visualise vesting including double-trigger RSUs       | Must     |
| US-004 | Scenario modeling (IPO / lockup / hold)               | Must     |
| US-005 | Art. 7.p partial-year exemption                       | Must     |
| US-006 | Autonomía selection + foral opt-out                   | Must     |
| US-007 | Modelo 720 threshold alert                            | Must     |
| US-008 | ESPP Spanish-tax treatment                            | Should   |
| US-009 | Worksheet export for gestor                           | Must     |
| US-010 | Ranges and sensitivity on every tax number            | Must     |
| US-011 | GDPR / LOPDGDD data-subject self-service              | Must     |
| US-012 | Freemium gating and upgrade flow                      | Must     |
| US-013 | Sell-now calculator (post-IPO Persona B leg)          | Must (new in v1.1) |

---

## 7. Non-functional requirements

### 7.1 Tax-rule versioning (non-negotiable)

| Requirement | Detail |
|---|---|
| Versioned rule sets | Every IRPF bracket, ahorro-base tier, autonomía table, Art. 7.p cap, and Modelo 720 threshold is stored in a versioned rule set with a semver-style identifier (e.g., `es-2026.1.0`) and an AEAT-guidance-date anchor. |
| Stamped outputs | Every calculation, every scenario, every export is stamped with the rule-set version it was computed under. |
| UI disclosure | The UI surfaces the active rule-set version on every calculation page, with a tooltip "Calculated under rule set es-2026.1.0, AEAT guidance as of YYYY-MM-DD". |
| Recompute action | Users can explicitly recompute a saved scenario under the current rule set; the prior computation is preserved. |
| Change log | A public changelog of rule-set versions is maintained (e.g., `/changelog/tax-rules`). |

### 7.2 GDPR / LOPDGDD posture (v1-ready, not deferred)

| Requirement | Detail / target |
|---|---|
| Hosting | EU region only. No transfer outside EEA without SCCs + TIA. |
| Encryption | TLS 1.3 in transit; AES-256 (or equivalent) at rest for all user data. |
| Authentication | Email + password with bcrypt/argon2id; TOTP 2FA available in v1 (mandatory a §8 open question). |
| Data minimization | Grant values stored as entered; product analytics MUST NOT include grant values, share counts, or tax outputs in event payloads. |
| Retention | Active account: indefinite. Deleted account: 30-day soft-delete grace, then hard delete (audit-log retention separately per §7.9). |
| DSR SLA | Access / portability export within 30 days (GDPR Art. 12(3)); in practice target 7 days via self-service. |
| Breach notification | 72-hour notification plan to AEPD per GDPR Art. 33; user notification template prepared. |
| DPA | Data Processing Agreement available for paid users; sub-processor list published. |
| DPO | DPO appointment status is a §8 open question; if not required, a clear privacy contact is published. |
| Cookies | Spanish cookie-banner compliant with AEPD 2023 guidance; analytics cookies opt-in. |

### 7.3 Extensibility to country N+1 (UK paper-design gate)

| Requirement | Detail |
|---|---|
| Hybrid engine architecture | Tax logic MUST be separable by jurisdiction. The solution-architect ADR (see §10) will decide rules-engine-vs-per-country-modules-vs-hybrid. |
| UK smell test | Every Spain-engine design decision MUST pass the "does this accommodate UK EMI / CSOP / unapproved options?" smell test. UK is paper-designed (not coded) in v1. |
| Residency model | Data model separates residency from citizenship. Residency is modelled as time-bounded periods (so a mobile worker's 2024 = Spain, 2025 = Portugal is representable). v1 engine computes single-jurisdiction at a time; data model does not block future multi-jurisdiction. |
| Acceptance | Adding UK paper-design to the ADR MUST NOT require edits to Spain calculation logic or Spain rule sets. If it does, the abstraction failed. |

### 7.4 Accuracy-communication standards

| Requirement | Detail |
|---|---|
| Never a bare point estimate | Every projected tax number shows a central estimate AND a sensitivity range around the key input. **Reaffirmed for sell-now (US-013): outputs must carry a price band (intraday volatility range, methodology per OQ-14) AND an FX band (at 0% and 3% spread around ECB daily mid).** No exemption for sell-now, despite its "current price" framing — current price is still an estimate, not a guarantee of execution price. |
| Formula transparency | Every number is expandable to show: inputs, formula, intermediate steps, rule-set version, AEAT guidance date. |
| Disclaimer footer | Every calc page and every export carries the "no es asesoramiento fiscal ni financiero" footer, non-dismissable. |
| No personalized recommendation | The tool MUST NOT output recommendations ("you should exercise", "you should wait", "you should sell now"). It outputs numbers and visualizations; interpretation is the user's (or their asesor's). This is a CNMV / MiFID II positioning requirement. Particular care applies to the sell-now calculator, whose name and UX risk being read as a "sell" recommendation — product copy must frame it as "estimate your net-EUR landing for a hypothetical sale today." |
| Unknown-input handling | When an input is unknown (e.g., missing ESPP lookback FMV, stale or unavailable market quote), the tool requests it or proceeds with a clearly-flagged default plus wider sensitivity band; it never silently substitutes a last-known value without disclosure. |

### 7.5 Autonomía coverage

| Requirement | Detail |
|---|---|
| Supported | All territorio común autonomías with 2026 rate tables: Andalucía, Aragón, Asturias, Baleares, Canarias, Cantabria, Castilla–La Mancha, Castilla y León, Cataluña, Extremadura, Galicia, Madrid, Murcia, La Rioja, Valencia, plus Ceuta and Melilla. |
| Not supported v1 | País Vasco and Navarra (foral regimes). Users in these regions see a clear "not supported" state, retain free-tier portfolio/vesting features, and are blocked from tax calculations. |
| Rate-table source | Each rate table stamped with its BOE / autonomía gazette publication date and stored in the versioned rule set (§7.1). |

### 7.6 Market-data vendor selection (new in v1.1)

| Requirement | Detail |
|---|---|
| Quote type | **15-minute delayed** US equity quotes. No streaming / no real-time in v1. Sufficient for decision-support; streaming adds cost, licensing, and exchange-reporting complexity that is out of v1 risk appetite. |
| Vendor candidates | IEX Cloud, Yahoo Finance-tier providers, Alpha Vantage, or equivalent. Specific choice is OQ-13 (solution-architect to pick). |
| EU-hosting / GDPR posture | Vendor choice MUST NOT compromise §7.2. Data plane: only ticker symbols leave EU hosting (not PII, not grant values). If vendor requires user-identifying metadata in requests, it is rejected. |
| Staleness handling | Quote timestamp is surfaced to the user on every sell-now computation. If the quote is older than a defined staleness threshold (TBD, solution-architect), the UI forces a user override or re-fetch before computing. |
| Override | User may always override the vendor-supplied price with a manually-entered price; the UI clearly labels that an override is in effect. |
| Licensing | Vendor's terms-of-use must permit redistribution of delayed quotes to end-users inside a SaaS UI. |

### 7.7 FX-source selection (new in v1.1)

| Requirement | Detail |
|---|---|
| Canonical mid-rate source | **ECB daily reference rate** (published ~16:00 CET each TARGET business day). This is the canonical source for all USD↔EUR conversions in Orbit, aligning with OQ-09's existing default. |
| Spread model | A configurable spread is subtracted from mid to approximate what the user's Spanish receiving bank actually applies. Default spread: **1.5%**. User-adjustable per sell-now session. |
| Sensitivity bands | Sell-now outputs MUST display EUR conversions at **spread = 0%** (best-case wholesale) and **spread = 3%** (worst-case retail) alongside the user's chosen spread, per §7.4. |
| Staleness | If ECB reference rate is unavailable (e.g., weekend / holiday), fall back to the most recent published rate with the timestamp surfaced. No silent substitution. |
| Override | User may override the mid-rate as well as the spread; both overrides are clearly labelled in outputs and exports. |

### 7.8 Performance targets (educated guesses, to be validated)

| Operation | Target |
|---|---|
| Scenario recalc (single user, typical grant set) | < 500 ms P95 |
| CSV import (1,000 rows) | < 10 s end-to-end including validation |
| Page load — dashboard | < 2 s P75 on EU broadband |
| PDF export generation | < 5 s P95 |
| Data-subject export archive generation | < 24 h asynchronous |
| Availability SLO | 99.5% v1 (paid users); error budget reviewed monthly |

### 7.9 Security

| Requirement | Detail |
|---|---|
| Authentication | Email + password (strong-password policy per NIST SP 800-63B). TOTP 2FA available; mandatory-for-paid is a §8 open question. |
| Session management | Short-lived access tokens, idle timeout, device list visible to user, revoke-session action. |
| Audit log | Every calculation, every export, every grant edit, every auth event recorded with timestamp, actor, IP (hashed), and traceability ID. Retained 6 years (AEAT prescription window) separately from personal data and access-restricted. |
| Export traceability | Each export carries a traceability ID that reverse-maps to the audit-log entry containing inputs + rule-set version. |
| Rate limiting | Login, CSV upload, export, **sell-now compute**, and market-data-fetch endpoints rate-limited. |
| Dependency posture | No secrets in client bundle; SBOM produced; vulnerability scanning in CI. (Implementation detail deferred to architecture.) |
| Pen-test | Third-party pen-test before paid-tier launch. |

### 7.10 Accessibility & i18n

| Requirement | Detail |
|---|---|
| WCAG | Target WCAG 2.1 AA on all core flows (grant entry, scenario modeler, export, account settings). |
| Languages v1 | Spanish (es-ES) and English (en). |
| Currency display | User-selected primary (EUR or USD); all tax outputs shown in EUR with FX-rate disclosed. |
| Date formats | ISO 8601 internally; user locale for display. |

### 7.11 Cost / quota (directional)

| Item | Note |
|---|---|
| Hosting (EU) | Cost scales with paid users; free tier intentionally lightweight. |
| PDF generation | Rendered on-demand; expensive operations rate-limited. |
| Data retention | Audit logs at 6 years will dominate storage for long-lived accounts; storage tiering expected (hot 12 months, cold beyond). |
| Market-data vendor | Per-quote or per-symbol subscription cost; cached at session granularity for 15-minute delayed data to minimise vendor-call volume. Sell-now is a paid feature (US-012 gating), so vendor cost is bounded by paid-user activity. |

---

## 8. Assumptions and open questions

Each open question is numbered, with a proposed default so that v1 work can proceed if the question is not resolved by the architect / security-engineer / product owner.

| # | Question | Proposed default | Owner |
|---|---|---|---|
| OQ-01 | Is 2FA mandatory for paid tier, or optional with strong nudge? | Optional with strong nudge in v1; mandatory in v1.1. | security-engineer |
| OQ-02 | DPO appointment required under Art. 37 GDPR? | Assume not-required for v1 user volume; publish a privacy contact. Re-evaluate at 10k MAU. | security-engineer / legal |
| OQ-03 | Which billing provider (Stripe, Paddle, Chargebee) for EU VAT handling? | Default: Stripe Tax. Revisit if VAT-MOSS complexity requires Paddle-as-MoR. | solution-architect |
| OQ-04 | CSV import size limit? | Default: 1,000 rows / 5 MB. | solution-architect |
| OQ-05 | Grace period for lapsed-paid access to existing scenarios? | 90 days read-only, then soft-delete. | product / Ivan |
| OQ-06 | Sensitivity range default width (±10% shown; what about ±25% as a toggle)? | Default ±10% shown inline; ±25% available as an expanded view. | product |
| OQ-07 | Double-trigger RSU liquidity-event inputs: do we model tender offers as liquidity events in v1? | Default: Yes for tender offers where the user actually transacts; No for secondary-market whispers. Documented in user-facing help. | product |
| OQ-08 | Art. 7.p documentation checklist: do we surface it in-product or only in the export? | Default: both; short inline checklist plus full checklist on export. | product |
| OQ-09 | FX-rate source (ECB reference rate, daily or monthly average)? | Default: ECB daily reference rate; user can override. | solution-architect |
| OQ-10 | Professional indemnity insurance in place before paid launch? | Assumed yes; must be confirmed before paid-tier enablement. | Ivan |
| OQ-11 | Free tier: does it include CSV import or is CSV import paid? | Default: CSV import is free (acquisition driver); exports and scenarios are paid. | product |
| OQ-12 | Modelo 720 categories: how granular is the v1 data model? | Default: the three regulatory categories (securities, bank accounts, real estate); v1 Orbit focuses on securities only; the other two categories are user-self-reported numerical inputs. | product |
| OQ-13 | Specific market-data vendor for 15-min delayed US equity quotes (IEX Cloud vs Yahoo-tier vs Alpha Vantage vs other)? | Default: start with the cheapest vendor whose ToS permits redistribution of delayed quotes inside a SaaS UI and whose data plane does not require PII. Architect selects concretely in the ADR. | solution-architect |
| OQ-14 | Intraday-volatility band methodology for sell-now price band (e.g., day's high/low vs prior-close ± N·σ vs other)? | Default: day's high–low range as published by the same vendor supplying the quote; fall back to prior-close ± 5% when intraday range unavailable. Document the formula in-app per §7.4. | solution-architect / product |
| OQ-15 | ESPP purchase-date FMV source for sell-now: user input or broker CSV? | Default: pulled from the existing ESPP grant record if present (US-008 captures it on entry / CSV import); if missing, prompt user to enter before compute. No silent defaults. | product |

### 8.1 Assumptions (standing until falsified)

- AEAT will not publish a disruptive rule change between spec approval and v1 launch; if it does, the versioning system in §7.1 absorbs it.
- 2026 autonomía rate tables are stable enough to be captured pre-launch and refreshed at the next-year boundary.
- Spanish users accept an English + Spanish product; full multilingual (Catalan, Euskara, Galego) is deferred.
- Users are willing to enter grant data manually for high-stakes decisions; CSV import is a convenience, not a blocker.
- The CNMV / MiFID II positioning risk (see §9) is mitigable via disclaimers, no-recommendation outputs, and clear product framing; a legal opinion will be obtained before launch.

---

## 9. Risk register

Likelihood (L / M / H) × Impact (L / M / H). Owners default to TBD.

| # | Risk | L | I | Mitigation | Owner |
|---|---|---|---|---|---|
| R-1 | CNMV / MiFID II — scenario modeling crosses into personalized investment recommendation | M | H | Strict positioning (decision-support, not advice); no "should" outputs; aggressive disclaimers; legal opinion before launch; explicit product copy review. | TBD (legal + product) |
| R-2 | Tax-advisor scope — users mistake Orbit for a licensed asesor fiscal | M | H | Non-dismissable disclaimers; "consult your asesor fiscal" CTAs throughout; version-stamped outputs tied to AEAT guidance dates; professional indemnity insurance before launch. | TBD (legal + Ivan) |
| R-3 | Tax-rule volatility — AEAT or autonomía changes break calculations silently | M | H | Versioned rule sets per §7.1; stamped outputs; recompute-under-current action; changelog. | solution-architect |
| R-4 | GDPR + LOPDGDD violation — EU hosting, DSR handling, breach-notification | L | H | v1-ready GDPR posture per §7.2; EU-only hosting; DPA; 72-hour breach-notification plan; DPO or privacy contact published; AEPD awareness. | security-engineer |
| R-5 | Autonomía complexity — País Vasco / Navarra produce wrong numbers | L | H | Explicit "not supported v1" state per §7.5; user blocked from tax calculations in foral regions; retained for free-tier features. | solution-architect |
| R-6 | Accuracy liability — a confidently-wrong number drives a large-€ mistake | M | H | Ranges + sensitivity per §7.4; never bare point estimates; formula and inputs always surfaced; "no es asesoramiento fiscal" footer; audit log + export traceability. | solution-architect + product |
| R-7 | Market-data vendor dependency — vendor ToS changes, pricing changes, licensing revocation, or EU-hosting/GDPR incompatibility disrupts sell-now | M | M | Treat vendor as an abstracted port (OQ-13); keep user price override as first-class input so the feature degrades gracefully; contractual review before go-live; monitor vendor health. | solution-architect |
| R-8 | FX-spread assumption accuracy — 1.5% default diverges meaningfully from what user's actual receiving bank applies | M | M | Bands at 0% and 3% per §7.7 honestly communicate the range; spread is user-adjustable; in-product copy explains the default is an approximation not a guarantee. | product |
| R-9 | User confusion — sell-now decision-support is mistaken for actual realized-sale tax reporting, leading users to skip their gestor / mis-file Modelo 100 | M | H | Clear UI framing ("estimate your net-EUR landing for a hypothetical sale today"); non-dismissable "no es asesoramiento" disclaimer per §7.4; passive Modelo 720/721 banner; copy explicitly states "this is not a tax-filing worksheet" and directs users to their gestor for realized-sale reporting. | product + legal |

---

## 10. Handoff

The spec is ready for the following sequence of next actors.

1. **`solution-architect` (first pass)** — Produce an ADR on the hybrid tax-engine architecture: rules-engine vs. per-country modules vs. hybrid. The UK paper-design exercise (§7.3) is part of this ADR and is the acceptance gate — the ADR is not done if it cannot demonstrate that UK EMI/CSOP/unapproved options slot into the design without touching Spanish calculation logic. The ADR MUST also address:
   - the versioned rule-set mechanism (§7.1),
   - the residency-period data model (§7.3),
   - the export traceability scheme (§7.9).
   - **market-data vendor selection (§7.6)** as a first-class architectural concern: vendor choice, caching strategy, staleness handling, EU-hosting / GDPR compatibility, licensing for redistribution of delayed quotes.
   - **FX-source selection (§7.7)** as a first-class architectural concern: ECB reference-rate ingestion, fallback on non-publication days, spread model, audit-log implications.
   Out of scope for this pass: picking any specific database, language, or framework.

2. **`security-engineer`** — Review the spec (and any draft from the architect) for:
   - GDPR / LOPDGDD posture (§7.2) including EU hosting, DSR endpoints, DPO question (OQ-02), breach-notification plan.
   - CNMV / MiFID II positioning-risk review (R-1): is anything in the scenario-modeler UI or copy crossing into recommendation territory?
   - Data-handling model for grant values (data-minimization in analytics, retention policy, audit-log access controls).
   - Authentication / 2FA posture (OQ-01).
   Output: a security review note appended to or referenced from this spec, plus any must-fix items before architecture is finalized.

3. **`solution-architect` (second pass, post-security-review)** — Produce:
   - The concrete data model, with time-bounded residency, versioned rule sets, and export-traceability IDs.
   - The calculation-versioning scheme (how rule sets are authored, tested, released).
   - Any revised ADR content in response to the security review.

4. **`implementation-engineer`** — Only after (1)–(3) are signed off.

> **Next:** invoke `solution-architect` with `/Users/ivan/Development/projects/orbit/docs/specs/orbit-v1-persona-b-spain.md` to produce the ADR on the hybrid tax engine architecture and the UK paper-design gate. The ADR scope now **additionally** covers market-data vendor selection (§7.6) and FX-source selection (§7.7) as first-class architectural concerns, not addenda.

---

## 11. Glossary

| Term | Definition |
|---|---|
| **IRPF** | Impuesto sobre la Renta de las Personas Físicas — Spanish personal income tax. |
| **Rendimiento del trabajo** | "Work income" category in IRPF; covers salary, RSU-at-vest value, NSO bargain element at exercise, ESPP discount. |
| **Ahorro base** | "Savings base" in IRPF; the separate tax base for capital gains and investment income, taxed at 19 / 21 / 23 / 27 / 28% tiers. |
| **Art. 7.p** | IRPF Article 7.p exemption of up to €60,100/year for work income earned for work physically performed abroad for a non-resident beneficiary. |
| **Beckham Law** | Régimen especial de impatriados — optional IRPF regime for inbound workers, taxing Spanish-source income at a flat 24% up to €600k. v1 only flags eligibility; does not compute. |
| **Modelo 720** | Informational declaration of foreign-held assets (securities, bank accounts, real estate) when total in any category exceeds €50,000. Non-filing penalties historically severe; post-CJEU ruling reformed. |
| **Territorio común** | The 15 autonomías + Ceuta + Melilla under the common Spanish tax regime (as opposed to foral). |
| **Foral regime** | Special tax regimes of País Vasco (three diputaciones forales) and Navarra, with their own IRPF rules. Unsupported in v1. |
| **Autonomía** | Spanish autonomous community; territorio común autonomías set their own IRPF rate tables on top of the statewide brackets. |
| **409A** | US IRC §409A valuation — an independent appraisal of a private US company's common stock, used as FMV for option strike and tax purposes. |
| **Double-trigger RSU** | RSU that vests only when both triggers are met: (1) time-based service vesting, and (2) a liquidity event (IPO, acquisition, sometimes tender). Prevents taxable income on private-company RSUs before liquidity. |
| **Delaware flip** | Restructuring of a non-US startup so that a newly-created US-Delaware HoldCo owns the original operating entity; common among Spanish startups raising from US VCs. |
| **EOR** | Employer of Record — a third-party company that formally employs a worker on behalf of a foreign client company (e.g., Deel, Remote). |
| **NSO** | Non-qualified Stock Option — US option type taxed on the bargain element at exercise as ordinary income. Default for non-US employees. |
| **ISO** | Incentive Stock Option — US tax-favoured option; requires US residency for full benefit. Mapped to NSO for Spanish-tax purposes in v1. |
| **ESPP** | Employee Stock Purchase Plan — periodic share purchase at a discount (often 15%) with optional lookback. |
| **AEAT** | Agencia Estatal de Administración Tributaria — Spanish tax authority. |
| **AEPD** | Agencia Española de Protección de Datos — Spanish data-protection authority; active, high-profile enforcement. |
| **LOPDGDD** | Ley Orgánica de Protección de Datos Personales y garantía de los derechos digitales — Spain's GDPR implementing law. |
| **DSR** | Data Subject Rights — GDPR rights of access, rectification, erasure, portability, restriction, objection. |
| **CNMV** | Comisión Nacional del Mercado de Valores — Spanish securities regulator; gatekeeper for investment-advice activity. |
| **Patrimonio / Solidaridad** | Spanish wealth taxes (regional Patrimonio + state-level Impuesto de Solidaridad a las Grandes Fortunas). Full calculation out of scope v1. |

---

## 12. Changelog

### 1.1.0-draft — 2026-04-18

- **Persona B extended from "pre-IPO only" to "pre-IPO OR post-IPO (if employer has since IPO'd)".** Framed as a lifecycle persona spanning both legs in the same spec file.
- **New US-013: Sell-now calculator** (post-IPO leg). Decision-support only, stateless-ish, paid feature.
  - Supported instruments: RSUs, ESPP, NSO same-day exercise-and-sell (with strike-price input). Vested-unexercised NSO sell-later flow is deferred.
  - Tax treatment per instrument documented: RSU cap gain on basis = FMV-at-vest; ESPP cap gain on basis = FMV-at-purchase with discount already taxed at purchase (US qualifying-disposition NOT modeled); NSO same-day = bargain element as rendimiento del trabajo at exercise + ~0 cap gain same-day.
  - Quote source: 15-min delayed (IEX/Yahoo-tier vendor) with user price override. No streaming.
  - FX source: ECB daily reference rate + user-adjustable spread (default 1.5%, bands at 0% and 3%).
  - User-override fields for broker fees, US withholding, transfer fees — all defaulting to 0 with explanatory tooltips.
  - Non-dismissable "no es asesoramiento" disclaimer on output; passive Modelo 720/721 banner (no calculation, no threshold check).
- **New NFR sections** §7.6 (market-data vendor selection) and §7.7 (FX-source selection) as first-class architectural concerns.
- **Security (§7.9)** adds rate-limiting for sell-now compute + market-data-fetch endpoints.
- **Cost (§7.11)** adds market-data vendor line item.
- **Accuracy-communication (§7.4)** reaffirmed that ranges-and-sensitivity NFR applies to sell-now (price band + FX band). No exemption despite the "current price" framing.
- **Open questions** OQ-13 (specific market-data vendor), OQ-14 (intraday-volatility band methodology), OQ-15 (ESPP purchase-date FMV source).
- **Risks** R-7 (market-data vendor dependency), R-8 (FX-spread assumption accuracy), R-9 (user confusion between decision-support and realized-sale reporting).
- **Out of scope (explicit)** added: realized-sale ledger, FIFO lot tracking, Modelo 100 worksheet, live streaming quotes, vested-unexercised NSO sell-later flow, US ESPP qualifying-disposition modeling, Modelo 720/721 calculations within sell-now.
- **Handoff** updated: solution-architect ADR scope now explicitly covers market-data vendor and FX-source selection.

### 1.0.0-draft — 2026-04-17

- Initial scoping spec for Persona B Pre-IPO Decision-Maker, Spain. Twelve user stories (US-001 through US-012), eleven NFR sections, twelve open questions, six risks. Handoff to `solution-architect` for hybrid tax-engine ADR with UK paper-design acceptance gate.

---

*End of spec.*
