# Orbit

Equity decision-support and tax-modeling platform for Spain-tax-resident employees holding US-company equity.

## What it does

Orbit helps employees of US/Delaware-flipped companies who are Spain-tax-resident model the tax and net-proceeds impact of equity decisions (exercise, sale, residency change) across both US-grant structure and Spanish IRPF.

It is **decision-support, not licensed advice** — no CNMV registration, no personalized recommendations, no e-filing. Every calculation output carries a non-dismissable "no es asesoramiento fiscal" disclaimer.

## v1 scope

v1 targets a single persona (Persona B): Pre-IPO or post-IPO decision-maker, Spain-tax-resident (territorio común only), employed by a US company via Spanish subsidiary/EOR or by a Delaware-flipped startup.

**Supported instruments:** RSUs (incl. double-trigger), NSOs, ESPP. ISOs mapped to NSO for Spanish tax treatment.

**Two use cases:**
1. **Pre-IPO scenario modeling** — vesting schedules, exercise-vs-hold decisions, residency planning, Modelo 720 awareness, Art. 7.p expatriate exemption modeling.
2. **Post-IPO sell-now calculator** — "if I sell today, what lands in my Spanish bank account in EUR after everything" round-trip using 15-min delayed quotes + ECB FX + user-overridable fees.

**Out of scope for v1:** PSU, §83(b) early-exercise, phantom/SAR, Spanish-law options, País Vasco/Navarra foral regimes, realized-sale ledger with FIFO lot tracking, Modelo 100 worksheet, live streaming quotes, broker API integrations, e-filing.

## Non-negotiable NFRs

- Versioned tax rule sets anchored to AEAT guidance dates
- EU-only hosting with full GDPR/LOPDGDD posture
- Ranges-and-sensitivity on every tax number (never bare point estimates)
- Hybrid tax-engine architecture with UK as paper-design acceptance gate
- Territorio común only; País Vasco/Navarra explicitly unsupported

## Monetization

B2C freemium. Free tier = tracking + vesting. Paid tier = tax projections, scenario modeling, exports, sell-now calculator.

## Status

Greenfield — requirements spec only, no code yet.

- Spec: [`docs/specs/orbit-v1-persona-b-spain.md`](docs/specs/orbit-v1-persona-b-spain.md) (v1.1.0-draft, 2026-04-18)
- Next: solution-architect ADR covering hybrid tax engine, rule-set versioning, market-data vendor selection, and FX-source selection.
