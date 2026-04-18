# ADR-003: Hybrid tax-engine architecture

- **Status:** Proposed
- **Date:** 2026-04-18
- **Deciders:** Ivan (owner)

## Context

This is the most consequential architectural decision in Orbit. The spec (§7.3) requires the tax engine to be jurisdiction-extensible such that **adding UK (EMI / CSOP / unapproved options) does not require edits to Spain calculation logic or rules**. UK is paper-designed in v1 but must pass an acceptance gate now: if the UK design forces touching Spain code, the abstraction has failed.

Equally non-negotiable (§7.1): every calculation must be **stamped with a versioned rule set + AEAT guidance date**, and a calculation from 2026 must replay byte-identically in 2029. This couples directly to engine architecture because the engine consumes rule data, and rule data must be versioned independently from rule logic.

Three families of design exist:

- **(a) Rules-engine DSL.** Authors express IRPF brackets, ahorro-base tiers, Art. 7.p caps, etc. in a declarative DSL (YAML/JSON or a small expression language) interpreted by Rust. Maximum flexibility; minimum compile-time safety; risk of building a slow, bespoke programming language.
- **(b) Per-country Rust modules.** Each country is a separate Rust crate / module implementing its own end-to-end calculator. Maximum compile-time safety; heavy code duplication of shared primitives (Money, FX, residency periods); changes to shared concepts require touching every country.
- **(c) Hybrid — shared primitives + per-country trait impl + externalized rule data.** Shared Rust types for the universal concepts (`Money`, `TaxableEvent`, `ResidencyPeriod`, `FxRate`); a single `TaxCalculator` trait; per-country implementations (`SpainTaxCalculator`, future `UkTaxCalculator`); **rule parameters (bracket cutoffs, rates, caps, autonomía tables) externalized as versioned data tables** (Postgres rows + signed-off YAML in repo for diff-review), consumed at calc time by each country impl.

## Decision

**Adopt option (c): hybrid architecture.**

### Shared primitive types (single `orbit-tax-core` crate, jurisdiction-agnostic)

```rust
// Money is always (amount, currency); arithmetic forbidden across currencies.
pub struct Money { pub amount: Decimal, pub currency: Currency }

pub enum Currency { EUR, USD, GBP, /* extensible */ }

// FX conversion is explicit, audited, and stamped with source + date.
pub struct FxRate {
    pub from: Currency, pub to: Currency,
    pub rate: Decimal,
    pub source: FxSource,         // e.g., EcbDailyReference
    pub as_of: NaiveDate,
    pub user_spread_bps: Option<i32>, // None = no spread applied
}

// A taxable event is the universal input vocabulary — every country
// consumes the same event stream.
pub struct TaxableEvent {
    pub id: Uuid,
    pub kind: EventKind,          // see below
    pub occurred_on: NaiveDate,
    pub instrument: Instrument,   // Rsu | Nso | Espp | (extensible)
    pub quantity: Decimal,
    pub fmv_at_event: Money,
    pub strike_paid: Option<Money>,
    pub jurisdiction_at_event: JurisdictionCode, // ES | UK | ...
    pub source_grant_id: Uuid,
}

pub enum EventKind {
    Vest, Exercise, Purchase, Sale, LiquidityTrigger,
}

// Residency is time-bounded; the same person can move.
pub struct ResidencyPeriod {
    pub user_id: Uuid,
    pub jurisdiction: JurisdictionCode,
    pub sub_jurisdiction: Option<String>, // e.g., autonomía code "ES-MD"
    pub from: NaiveDate,
    pub to: Option<NaiveDate>,    // None = current
    pub regime_flags: BTreeSet<RegimeFlag>, // e.g., BeckhamLaw, NonDom
}

pub struct RuleSet {
    pub id: RuleSetId,            // e.g., "es-2026.1.0"
    pub jurisdiction: JurisdictionCode,
    pub aeat_guidance_date: NaiveDate,
    pub effective_from: NaiveDate,
    pub effective_to: Option<NaiveDate>,
    pub content_hash: String,     // SHA-256 of normalized rule data
    pub data: RuleData,           // jurisdiction-specific payload
}
```

### The single trait every country implements

```rust
pub trait TaxCalculator {
    fn jurisdiction(&self) -> JurisdictionCode;

    fn compute(
        &self,
        ctx: &CalcContext,        // user, residency periods, FX provider
        events: &[TaxableEvent],
        rules: &RuleSet,
    ) -> Result<TaxResult, TaxError>;
}

pub struct TaxResult {
    pub line_items: Vec<TaxLineItem>,    // per-event, per-bucket breakdown
    pub totals: TaxTotals,
    pub rule_set_id: RuleSetId,
    pub rule_set_content_hash: String,   // for replay verification
    pub formula_trace: FormulaTrace,     // backs the "show formula" UX
    pub sensitivity_inputs: Vec<SensitivityInput>, // backs §7.4 ranges
}
```

### Per-country implementations

- `orbit-tax-spain` crate exports `pub struct SpainTaxCalculator` implementing `TaxCalculator`. It contains all IRPF logic, ahorro-base routing, autonomía rate-table application, Art. 7.p exemption math, Modelo 720 threshold checks. Rule data (statewide brackets, per-autonomía tables, Art. 7.p cap) lives in `RuleData::Spain { ... }` typed enum variants.
- Future `orbit-tax-uk` crate exports `pub struct UkTaxCalculator` implementing the same trait. Its `RuleData::Uk { ... }` carries EMI tax-advantaged option data, CSOP limits, unapproved-option NICs, etc.

### Rule data: externalized, versioned, hashed

Rule data is authored as **YAML in `/rules/<jurisdiction>/<version>.yaml`**, code-reviewed, and ingested into a Postgres `rule_sets` table at deploy time. The content hash is computed over the canonicalized YAML — any change to a published rule set produces a new version (immutability per ADR-004). The Rust calculators never embed numeric rates in code; they read from `RuleData`. This is what makes "replay 2026 calculations in 2029 byte-identically" achievable.

### UK paper-design acceptance gate (test of the abstraction)

Adding `orbit-tax-uk` requires:

1. New crate `orbit-tax-uk` depending on `orbit-tax-core`.
2. New `JurisdictionCode::UK` enum variant. (This is the only edit outside the new crate; it is a discriminator add, not Spain logic.)
3. New `RuleData::Uk` variant on the rule-data enum. (Same — additive enum variant.)
4. New rule files under `/rules/uk/uk-2026.1.0.yaml`.
5. New `pub struct UkTaxCalculator` implementing `TaxCalculator`.
6. Calculator dispatch (a `match` on `JurisdictionCode`) gains a `UK => UkTaxCalculator { ... }` arm.

**No edits required in `orbit-tax-spain`.** No edits to Spain rule files. No changes to shared primitives' semantics. The acceptance gate passes.

The instrument vocabulary is the place this could fail: if UK's EMI / CSOP / unapproved options cannot be expressed as `EventKind::{Vest, Exercise, Purchase, Sale}` against `Instrument::{Rsu, Nso, Espp}`. EMI and unapproved options map cleanly onto `Exercise` of an option-shaped instrument with a strike. EMI's tax-advantaged status is a per-country *interpretation* of an Exercise event under UK rules — i.e., logic inside `UkTaxCalculator`, not a new event kind. CSOP is the same shape. The `Instrument` enum should be extended to include `OptionUkEmi` / `OptionUkCsop` / `OptionUkUnapproved` *only if* per-instrument metadata (qualifying conditions, holding-period flags) needs to be carried on the event itself; otherwise UK plan-type metadata lives in the source grant, not the event. **Decision:** keep `Instrument` minimal and carry plan-type as `grant.metadata` consulted by the calculator. Extending `Instrument` is reversible and does not break Spain.

## Alternatives considered

- **Pure DSL (option a).** Rejected. A DSL good enough to express Art. 7.p partial-year proration, Modelo 720 multi-category thresholds, and UK EMI qualifying-disposition rules is effectively a programming language — and writing a correct, testable, reviewable interpreter is a project larger than Orbit v1 itself. Worse, it pushes correctness checks from compile-time to data-validation-time.
- **Pure per-country crates with no shared primitives (option b).** Rejected. Forces re-implementing `Money`, `TaxableEvent`, residency math, and FX handling in every country. Spain code would need to change shape every time a future country needs a slight twist on `Money`. Code review of country N+1 becomes a review of country N+1 *and* N more reimplementations.
- **Hybrid with rules-as-code (e.g., `rhai` or `rune` embedded scripting).** Considered. The flexibility is real, but introduces a runtime dependency, a sandboxing concern, and a debugging story (stack traces inside an embedded scripting VM are awful). Rejected in favour of pure-Rust calculators consuming typed rule data.

## Consequences

**Positive:**
- UK can be added in a self-contained crate without touching Spain. The acceptance gate (§7.3) is structurally satisfied, not just verbally claimed.
- Rule changes (e.g., AEAT publishes 2027 brackets) are a YAML PR + a new `RuleSet` row, not a code change. Rule-data review is separable from engine-code review.
- The trait + typed events keeps every calculation auditable: `FormulaTrace` is a first-class output, not a stringly-typed log.
- Replayability is a property of the design, not a feature added later: pinning `rule_set_id` + `content_hash` is sufficient.

**Negative / risks:**
- Designing the universal `TaxableEvent` / `Instrument` vocabulary correctly *now* matters. If a future country (Germany? France?) introduces a fundamentally new event shape, the enum may need extending. This is a manageable schema-evolution problem (additive enum variants are non-breaking) but warrants vigilance.
- Two layers of versioning to communicate to users: engine version (Rust binary) and rule-set version (data). The UI must show rule-set version prominently (§7.1 already requires this) and may need to show engine version on detailed traces.
- The YAML-to-typed-`RuleData` ingestion is a non-trivial parser/validator that itself needs tests. Mitigated by `serde` + a schema test suite that asserts every published rule set parses and round-trips.
- Beckham Law and foral regimes (País Vasco, Navarra) are explicit "not supported" states in v1 — the engine must short-circuit before invoking the calculator for these regimes (§7.5). This is a `CalcContext` precheck, not country logic.

**Follow-ups:**
- Implementation engineer: define the exact `RuleData::Spain` shape in code; sketch the YAML schema for `es-2026.1.0`.
- Implementation engineer: write a "UK paper-design conformance test" — a Rust test that constructs a stub `UkTaxCalculator` with `unimplemented!()` body and verifies the dispatch + trait-impl compiles, proving structural fit. Re-run on every Spain change.
- Solution-architect (second pass): decide where the `FormulaTrace` is rendered (server-side PDF vs client-side React) — affects whether trace data crosses the wire.
- Security-engineer: confirm that rule-set YAML files in the repo are subject to a code-review / sign-off process appropriate for the regulatory exposure (R-3, R-6).
