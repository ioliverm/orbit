# Orbit v1 — Implementation handoff

| Field    | Value                         |
|----------|-------------------------------|
| Version  | 1.0                           |
| Date     | 2026-04-18                    |
| Owner    | Ivan Oliver Martínez (product-owner + sole operator) |
| Purpose  | Single entry point for the implementation-engineer. Points at every artefact, states what's decided and what's escalated, prescribes the starting order. |

Read this doc first. It does not duplicate content — it indexes. Each linked artefact is the source of truth for its area.

---

## 1. What Orbit is (30-second context)

Decision-support web platform for Spain-tax-resident employees holding US-company equity (RSUs, NSOs, ESPP). **Not licensed advice**; four-layer disclaimer pattern enforced. B2C freemium: free = tracking + vesting; paid = tax projections + scenarios + exports + sell-now. Persona B only: territorio común, US-company employee via Spanish subsidiary/EOR or Delaware-flipped startup. See [`README.md`](../README.md) and [`specs/orbit-v1-persona-b-spain.md`](specs/orbit-v1-persona-b-spain.md) (v1.1.0-draft, 629 lines, **read in full before building**).

## 2. Build order (do not deviate)

```
Slice 0 (foundations) ──▶ Slice 1 (signup → grant → vesting timeline) ──▶ Slice 2 … 7
```

Each slice is independently shippable and demo-able. Do not start Slice 1 until every Slice-0 security-checklist item is green.

- **Slice-0 starting point**: [`security/security-checklist-slice-0.md`](security/security-checklist-slice-0.md) (30 items) + [`adr/ADR-013-repository-and-deployment-scaffold.md`](adr/ADR-013-repository-and-deployment-scaffold.md) (repo tree, CI, deploy).
- **Slice-1 starting point**: [`requirements/slice-1-acceptance-criteria.md`](requirements/slice-1-acceptance-criteria.md) + [`adr/ADR-014-slice-1-technical-design.md`](adr/ADR-014-slice-1-technical-design.md) + the six Slice-1 UX references (see §5).
- **Slice 2 and beyond**: sequenced in [`requirements/v1-slice-plan.md`](requirements/v1-slice-plan.md). Do not speculate; come back when Slice 1 is green.

**Slice 1 boundary (verbatim)**: sign up → disclaimer → residency (autonomía + Beckham flag + primary currency) → enter one grant manually → land on a dashboard tile and a vesting timeline including the double-trigger "awaiting liquidity event" state. **Free tier, no tax math, no FX, no EUR conversion, no CSV import, no exports, no rule-set chip, no Modelo 720 banner.**

## 3. Artefact map

### 3.1 Requirements
| File | Role |
|---|---|
| [`specs/orbit-v1-persona-b-spain.md`](specs/orbit-v1-persona-b-spain.md) | Source-of-truth product spec (v1.1.0-draft). |
| [`requirements/v1-slice-plan.md`](requirements/v1-slice-plan.md) | Ordered 8-slice delivery plan. |
| [`requirements/slice-1-acceptance-criteria.md`](requirements/slice-1-acceptance-criteria.md) | Implementation-ready AC for Slice 1. |
| [`requirements/open-questions-resolved.md`](requirements/open-questions-resolved.md) | Analyst's decisions on 11 UX + 15 spec + 15 cross-reading ambiguities. Treat as decisions, not suggestions. |

### 3.2 Architecture (ADRs)
| ADR | Decision | Status for Slice 0/1 |
|---|---|---|
| [ADR-001](adr/ADR-001-tech-stack-baseline.md) | Tech stack baseline | Locked |
| [ADR-002](adr/ADR-002-cloud-provider-and-deployment-topology.md) | Hetzner Falkenstein, EEA-only | Locked |
| [ADR-003](adr/ADR-003-hybrid-tax-engine-architecture.md) | Hybrid tax engine | Locked; not used in Slice 1 |
| [ADR-004](adr/ADR-004-rule-set-versioning-and-traceability.md) | Rule-set versioning | Locked; trigger scaffolded in Slice 0 |
| [ADR-005](adr/ADR-005-data-model-outline.md) | Data model outline + RLS | Locked |
| [ADR-006](adr/ADR-006-market-data-vendor-selection.md) | Market-data vendor | Locked; not used in Slice 1 |
| [ADR-007](adr/ADR-007-fx-source-ecb-integration.md) | ECB FX source | Locked; not used in Slice 1 (per C-4) |
| [ADR-008](adr/ADR-008-export-traceability.md) | Export traceability | Locked; not used in Slice 1 |
| [ADR-009](adr/ADR-009-frontend-architecture.md) | Frontend framework, i18n, CSP-strict packaging | Slice 0 |
| [ADR-010](adr/ADR-010-api-contract-shape.md) | Same-origin REST, cookie auth, error envelope | Slice 0 |
| [ADR-011](adr/ADR-011-authentication-session-mfa.md) | Signup/signin/reset/MFA flows + sequence diagrams | Slice 0/1 |
| [ADR-012](adr/ADR-012-rule-set-pipeline-and-engine-contract.md) | Engine public API + two-step publish | Slice 0 scaffold, used Slice 3+ |
| [ADR-013](adr/ADR-013-repository-and-deployment-scaffold.md) | Monorepo tree, migrations, CI, deploy | **Slice 0 blueprint** |
| [ADR-014](adr/ADR-014-slice-1-technical-design.md) | Slice 1 DDL + vesting algorithm + wizard state machine | **Slice 1 blueprint** |

Follow-ups and open tensions: [`adr/README.md`](adr/README.md).

### 3.3 Security
| File | Role |
|---|---|
| [`security/threat-model.md`](security/threat-model.md) | STRIDE + 63 threats (S1–S63), prioritized. |
| [`security/security-requirements.md`](security/security-requirements.md) | 150+ numbered requirements (SEC-001..SEC-305). Design inputs, not review comments. |
| [`security/security-checklist-slice-0.md`](security/security-checklist-slice-0.md) | 30-item non-negotiable floor. **Slice 0 exit gate.** |

Every PR touching auth, crypto, RLS, rule-sets, exports, or third-party integrations carries the `security-review` label (SEC-200).

### 3.4 UX / design
| File | Role |
|---|---|
| [`design/orbit-v1-ui-proposal.md`](design/orbit-v1-ui-proposal.md) | Proposal + visual direction + §13 implementation-ready refinement. |
| [`design/style-guide.md`](design/style-guide.md) | Tokens + primitives (WCAG AA). |
| [`design/screens/shared.css`](design/screens/shared.css) | Canonical tokens + primitive CSS (copy into frontend per ADR-009). |

**Slice 1 reference screens (build these first, in this order):**
1. [`signup.html`](design/screens/signup.html) — 5-state wizard; matches ADR-014 state machine 1:1.
2. [`signin.html`](design/screens/signin.html) — 5 states including CAPTCHA + rate-limit; SEC-003/004/161 copy.
3. [`password-reset.html`](design/screens/password-reset.html) — request + token-landing forms.
4. [`residency-setup.html`](design/screens/residency-setup.html) — wizard's densest step, as standalone reference.
5. [`first-grant-form.html`](design/screens/first-grant-form.html) — form shape aligns with `derive_vesting_events()` inputs.
6. [`dashboard-slice-1.html`](design/screens/dashboard-slice-1.html) — the **Slice 1 dashboard**. No tax, no FX, no rule-set chip.

**Governance surfaces (required by security, build alongside Slice 1 or in Slice 2):**
7. [`session-management.html`](design/screens/session-management.html) — SEC-010 Active sessions.
8. [`dsr-self-service.html`](design/screens/dsr-self-service.html) — SEC-123 DSR flows.

**Slice 3+ targets (do not build yet, but informative):** `dashboard.html` (EUR + tax tiles), `sell-now.html`, `scenario-modeler.html`, `grant-detail.html` (tax-event table), `export.html`, `export-confirm-modal.html`, `uncertainty-patterns.html`.

## 4. Cross-cutting rules (apply to every slice)

- **Disclaimer: "no es asesoramiento fiscal"** in the persistent footer on every authenticated page; mandatory confirm checkbox at export (gates the CTA); embedded in every PDF/CSV artefact. See UX proposal §6 and SEC-201.
- **Ranges-and-sensitivity on every tax number** — Pattern C (range-first headline) for the one decision-driving number per screen; Pattern B (bar + dot) for sub-totals; Pattern A (inline parenthetical) only in dense tables. Not applicable in Slice 1 (no tax math yet).
- **Rule-set version stamping** on every calculation output (SEC-086). Not applicable in Slice 1.
- **Bilingual ES/EN.** ES primary; EN fallback. Tax terminology stays Spanish. i18n framework lands in Slice 0 per ADR-009.
- **Logging allowlist (SEC-050)**: `orbit_log::event!` wrapper is the **only** logging API. Attempting to log `Money`, `Grant`, `Scenario`, `Calculation`, `SellNowInput`, `Export`, or raw `&str` matching NIF/NIE is a compile error. Scaffold this first — it's a Slice-0 item (S0-09).
- **RLS everywhere (SEC-020..SEC-024)**: `orbit_app` is not superuser and does not hold `BYPASSRLS`. `Tx::for_user(user_id)` is the only query-handle acquisition path; a CI lint forbids `pool.acquire()` elsewhere. Cross-tenant integration-test probes required.
- **No LLMs in v1** (assumption pinned in threat model §1.5). If this changes, re-run the threat model (S37, SEC-302).
- **CSP-strict**: no `'unsafe-inline'`, no `'unsafe-eval'` (SEC-180). The existing reference HTML honours this for Slice 1; earlier Slice-3+ screens contain inline styles that must be extracted before those slices build.

## 5. Escalations (need product-owner decision before affected slice ships)

These were raised by the specialists and are **not engineering decisions**. Address these before the slice that depends on them.

| # | Decision | Surfaced by | Blocks | Status |
|---|---|---|---|---|
| **E-1** | Professional indemnity insurance before paid-tier launch (OQ-10). | requirements-analyst | Slice 3 (paid-tier shell) | Open — product-owner |
| **E-2** | Cut EUR conversion from Slice 1 dashboard? Alternative is expanding Slice 1 to include the ECB FX pipeline (+XL work). Analyst default: cut. | requirements-analyst (C-4) | Slice 1 | **Default taken**: cut. Confirm. |
| **E-3** | Grace period for lapsed-paid access to existing scenarios (90 days default, OQ-05). | requirements-analyst | Slice 3 | Open — product-owner |
| **E-4** | Mandatory MFA for paid in v1 or v1.1? (SEC-011, S8, OQ-01) Security default: v1.1. | security-engineer | Paid launch | Open — product-owner |
| **E-5** | Rule-set PR reviewer model at single-operator stage (S23, S56). External reviewer on `/rules/**` PRs, or accept CI+tests alone? | security-engineer | First `active` rule set (Slice 3+) | Open — product-owner |
| **E-6** | CNMV / MiFID-II legal sign-off on the four-layer disclaimer pattern (AC-5, R-1). | security-engineer | Paid launch | Open — legal + product-owner |
| **E-7** | Billing provider: Stripe Tax vs Paddle MoR (OQ-03). Affects PCI scope + sub-processor register (SEC-121, SEC-303). | requirements-analyst | Slice 3 | Open — product-owner |
| **E-8** | Pen-test engagement scoped and booked (O-6). | security-engineer | Paid launch | Open — product-owner |

## 6. Known gaps flagged by specialists (address during build)

These are engineering decisions that need a follow-up decision inside the first build round, not escalations.

- **G-1 — SEC-102 Slice-1 interim mitigation.** Append-only `audit_log` requires the `orbit_support` role to exist; ADR-014 defers that role to Slice 2/3 because Slice 1 has no support surface. Interim mitigation: CI lint rejecting any call site on `audit_log` other than `INSERT`. **Security-engineer must bless the interim mitigation, or provision `orbit_support` in Slice 0** (~½ engineer day). See architect's report + ADR-014 §"Upstream ambiguities resolved unilaterally" item 7.
- **G-2 — CSP vs inline-style Slice-3+ screens.** Slice-1 references (signup.html through dashboard-slice-1.html) are CSP-clean. `sell-now.html`, `scenario-modeler.html`, and others use `style="width: N%"` for data-driven fills. Extract to data-attributes + CSS custom properties before the owning slice builds. Non-blocking for Slice 0/1.
- **G-3 — `onboarding.required` 403 degrades on direct URL load without JS.** UX proposed a `<meta http-equiv="refresh">` shell as fallback. **Architect + engineer must bless this** before the auth screens ship.
- **G-4 — SEC-003 / SEC-004 copy on `signin.html` states C and D** (rate-limit + generic-error without enumeration leak). UX copy provided. **Security-engineer bless before auth screens ship.**
- **G-5 — "¿No recibiste el correo de verificación? Reenviar enlace" persistent link on signin.html.** UX's answer to Conflict C. Security-engineer confirm this is not an enumeration oracle.

## 7. What the engineer does next

1. Read [`specs/orbit-v1-persona-b-spain.md`](specs/orbit-v1-persona-b-spain.md) in full. Everything else indexes into this.
2. Read [`adr/ADR-013-repository-and-deployment-scaffold.md`](adr/ADR-013-repository-and-deployment-scaffold.md) and [`security/security-checklist-slice-0.md`](security/security-checklist-slice-0.md) together. Start Slice 0.
3. Work through S0-01 … S0-30 in order. Check each off as landed. Do not start Slice 1 before all 30 are green.
4. Resolve G-1 (SEC-102 interim mitigation) with the security-engineer before closing Slice 0.
5. For Slice 1: read [`adr/ADR-014-slice-1-technical-design.md`](adr/ADR-014-slice-1-technical-design.md) + [`requirements/slice-1-acceptance-criteria.md`](requirements/slice-1-acceptance-criteria.md) + the six Slice-1 UX references. Build.
6. Raise E-1 … E-8 with the product-owner on the cadence that matches the slice that depends on them.

---

## Appendix — Folder map

```
docs/
├── implementation-handoff.md              ← this file
├── specs/
│   └── orbit-v1-persona-b-spain.md        ← source-of-truth spec
├── requirements/
│   ├── v1-slice-plan.md
│   ├── slice-1-acceptance-criteria.md
│   └── open-questions-resolved.md
├── adr/
│   ├── README.md                          ← ADR index + follow-ups
│   ├── ADR-001..ADR-008                   ← pre-existing decisions
│   └── ADR-009..ADR-014                   ← synthesis round (2026-04-18)
├── security/
│   ├── threat-model.md
│   ├── security-requirements.md
│   └── security-checklist-slice-0.md      ← Slice-0 exit gate
└── design/
    ├── orbit-v1-ui-proposal.md            ← §1–§12 initial; §13 refinement
    ├── style-guide.md
    └── screens/
        ├── shared.css
        ├── signup.html …                  ← Slice 1 critical path
        ├── session-management.html, dsr-self-service.html
        └── dashboard.html, sell-now.html …  ← Slice 3+ targets
```

*End of handoff.*
