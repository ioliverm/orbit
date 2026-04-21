//! Paper-gains pure function (Slice 3 T29, ADR-017 §5).
//!
//! The dashboard paper-gains tile and the Modelo 720 threshold banner's
//! securities derivation both call [`compute`]. Pure — no DB, no HTTP —
//! so the same function backs the shared fixture (`paper_gains_cases.json`)
//! that the frontend parity mirror consumes.
//!
//! # Algorithm (AC-5.2.3 + AC-5.4)
//!
//! For every eligible grant:
//!
//! 1. **Resolve current price** (AC-5.3.1 precedence):
//!    per-grant override > per-ticker price > None.
//! 2. **Compute native-currency paper gain** via the per-instrument rule:
//!    * RSU: `SUM over past vest events of (price - fmv_at_vest) * shares`.
//!      An event contributes iff `vest_date <= today`, `state == Vested`
//!      (AC-5.4.4 double-trigger exclusion), and `fmv_at_vest` is set.
//!    * ESPP: `SUM over espp_purchases of (price - fmv_at_purchase) * shares`
//!      (AC-5.4.2).
//!    * NSO / ISO (`iso_mapped_to_nso`, `nso`): excluded, `missing_reason
//!      = NsoDeferred` (AC-5.4.3).
//! 3. **Completeness** (AC-5.5.1): a grant is complete iff the current
//!    price was resolved AND every past vest event (or ESPP purchase)
//!    carries a non-NULL `fmv_at_vest` (resp. `fmv_at_purchase`).
//! 4. **EUR band** (AC-4.6.2): the native-currency gain is multiplied by
//!    `fx_rate_eur_native` (EUR per unit of the native currency) and
//!    three spreads (`0 %`, `1.5 %`, `3 %`) applied at render time.
//!
//! # ESPP treatment (documentation-of-record)
//!
//! Per AC-5.4.2 the ESPP basis is `espp_purchases.fmv_at_purchase` — NOT
//! the `vesting_events` path that RSUs use. Slice 3 therefore includes
//! ESPP grants in paper-gains when their purchases are populated. An
//! ESPP grant with no `espp_purchases` rows contributes nothing and is
//! not surfaced in `incomplete_grants` (AC-5.4.2 last sentence).
//!
//! # Decimal story
//!
//! Inputs cross the crate boundary as `String` decimals (matching the
//! `::text`-passthrough convention used by `orbit_db::fx_rates`,
//! `ticker_current_prices`, etc). Internal arithmetic uses `f64` — EUR
//! display is 2 dp and the multiplicative chain here is short enough
//! (price × shares × fx × spread; 4 multiplies) that f64 is well below
//! its 15-digit precision budget on the values Slice 3 handles (max a
//! few million EUR). The output EUR amounts are rendered as
//! `"{:.2}"`-formatted strings so the wire representation is stable.
//!
//! Traces to:
//!   - ADR-017 §5 (authoritative algorithm).
//!   - docs/requirements/slice-3-acceptance-criteria.md §5.

use std::collections::BTreeMap;

use chrono::NaiveDate;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::vesting::{Shares, VestingState, SHARES_SCALE};

// ---------------------------------------------------------------------------
// Public types
// ---------------------------------------------------------------------------

/// Slim grant view the pure function consumes. Handler maps
/// `orbit_db::grants::Grant` into this shape.
#[derive(Debug, Clone)]
pub struct GrantForPaperGains {
    pub id: Uuid,
    /// `rsu | nso | espp | iso_mapped_to_nso` — the DB enum value.
    pub instrument: String,
    /// `USD | EUR | GBP` — the grant's native currency. For RSU this is
    /// the currency of the `fmv_at_vest` values on the vesting rows; for
    /// ESPP it is the currency of the `fmv_at_purchase` values.
    pub native_currency: String,
    /// `UPPER(TRIM(grants.ticker))` or `None` for unlisted. Handler-side
    /// normalized — this function does not trim/upper itself.
    pub ticker: Option<String>,
    pub double_trigger: bool,
    pub liquidity_event_date: Option<NaiveDate>,
    pub vesting_events: Vec<VestingEventForPaperGains>,
    pub espp_purchases: Vec<EsppPurchaseForPaperGains>,
}

/// Per-event input. FMV is `NUMERIC(20,6)` as `String` per the repo
/// passthrough convention.
#[derive(Debug, Clone)]
pub struct VestingEventForPaperGains {
    pub vest_date: NaiveDate,
    pub state: VestingState,
    pub shares_vested_this_event: Shares,
    pub fmv_at_vest: Option<String>,
    pub fmv_currency: Option<String>,
}

/// Per-purchase input. FMV is `NUMERIC(20,6)` as `String`.
#[derive(Debug, Clone)]
pub struct EsppPurchaseForPaperGains {
    pub purchase_date: NaiveDate,
    pub shares_purchased: Shares,
    pub fmv_at_purchase: String,
    pub currency: String,
}

/// Per-ticker current price. Keyed by `(user_id, ticker)` on the DB.
#[derive(Debug, Clone)]
pub struct TickerPriceForPaperGains {
    pub ticker: String,
    pub price: String,
    pub currency: String,
}

/// Per-grant current-price override. Takes precedence over the per-ticker
/// price for its grant (AC-5.3.1).
#[derive(Debug, Clone)]
pub struct GrantPriceOverrideForPaperGains {
    pub grant_id: Uuid,
    pub price: String,
    pub currency: String,
}

/// Full input envelope.
#[derive(Debug, Clone)]
pub struct PaperGainsInput<'a> {
    pub grants: &'a [GrantForPaperGains],
    pub ticker_prices: &'a [TickerPriceForPaperGains],
    pub grant_overrides: &'a [GrantPriceOverrideForPaperGains],
    /// EUR per unit of native currency for today. `None` when
    /// `FxLookupResult::Unavailable` (AC-5.5.4). **Legacy** — treated
    /// as the fallback rate for grants whose `native_currency` has no
    /// entry in `fx_rates_by_currency`. The Slice-3 worker wires
    /// `fx_rates_by_currency` for every grant currency it resolved; a
    /// grant whose currency is present in `fx_rates_by_currency` with
    /// `None` (explicit missing) surfaces as
    /// [`MissingReason::UnsupportedCurrency`], distinct from the
    /// `fx_rate_eur_native = None` global case.
    pub fx_rate_eur_native: Option<String>,
    /// EUR-per-native rate keyed by the grant's `native_currency`. A
    /// `Some(rate)` entry supplies the conversion; a `None` entry
    /// signals "this currency has no FX today" and the grant lands as
    /// [`MissingReason::UnsupportedCurrency`]. Absent keys fall through
    /// to `fx_rate_eur_native` (Slice-3 back-compat for single-currency
    /// setups).
    pub fx_rates_by_currency: BTreeMap<String, Option<String>>,
    pub today: NaiveDate,
}

/// Per-grant output row.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PerGrantGains {
    #[serde(rename = "grantId")]
    pub grant_id: Uuid,
    pub complete: bool,
    /// Decimal string at 4 dp, or `None` when the grant is not complete.
    #[serde(rename = "gainNative", skip_serializing_if = "Option::is_none")]
    pub gain_native: Option<String>,
    /// EUR band at three spreads; `None` when FX unavailable OR the grant
    /// is incomplete.
    #[serde(rename = "gainEurBand", skip_serializing_if = "Option::is_none")]
    pub gain_eur_band: Option<EurBand>,
    /// Machine-readable reason. `None` when the grant is complete.
    #[serde(rename = "missingReason", skip_serializing_if = "Option::is_none")]
    pub missing_reason: Option<MissingReason>,
}

/// EUR amount at the three spread bands (AC-4.6.2).
///
/// `low` = 3 % retail spread, `mid` = 1.5 % central, `high` = 0 %
/// wholesale. All rendered at 2 dp.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EurBand {
    pub low: String,
    pub mid: String,
    pub high: String,
}

/// Stable enum shipped to the UI to pick the banner copy (AC-5.5).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MissingReason {
    /// `≥1` past vest event on an RSU carries `fmv_at_vest = NULL`.
    FmvMissing,
    /// Neither per-grant override nor per-ticker price resolved.
    NoCurrentPrice,
    /// NSO/ISO grants excluded in Slice 3 (AC-5.4.3).
    NsoDeferred,
    /// Double-trigger RSU with `liquidity_event_date IS NULL` (AC-5.4.4).
    DoubleTriggerPreLiquidity,
    /// The grant's `native_currency` has no FX rate in
    /// `fx_rates_by_currency` (Slice-3 T33 S4). The grant is
    /// otherwise complete; it surfaces on the incomplete-data banner
    /// as "FX no disponible para {currency}".
    UnsupportedCurrency,
}

/// Aggregate result.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PaperGainsResult {
    #[serde(rename = "perGrant")]
    pub per_grant: Vec<PerGrantGains>,
    /// Sum of the per-grant EUR bands across grants with `complete = true`.
    /// `None` when FX is unavailable or no grant is complete.
    #[serde(rename = "combinedEurBand", skip_serializing_if = "Option::is_none")]
    pub combined_eur_band: Option<EurBand>,
    /// UUIDs of grants that are incomplete AND actionable (i.e., NOT
    /// NsoDeferred and NOT DoubleTriggerPreLiquidity). Drives the partial-
    /// data banner per AC-5.5.1.
    #[serde(rename = "incompleteGrants")]
    pub incomplete_grants: Vec<Uuid>,
}

// ---------------------------------------------------------------------------
// Entry point
// ---------------------------------------------------------------------------

/// Pure paper-gains computation per ADR-017 §5. See the module docs for
/// the ESPP treatment and FX-unavailable semantics.
pub fn compute(input: &PaperGainsInput<'_>) -> PaperGainsResult {
    let fallback_fx = input.fx_rate_eur_native.as_deref().and_then(parse_decimal);

    let mut per_grant: Vec<PerGrantGains> = Vec::with_capacity(input.grants.len());
    let mut incomplete_grants: Vec<Uuid> = Vec::new();

    // Running band accumulators for the combined envelope.
    let mut combined_low: f64 = 0.0;
    let mut combined_mid: f64 = 0.0;
    let mut combined_high: f64 = 0.0;
    let mut combined_any = false;

    for g in input.grants {
        // Resolve the per-grant FX rate. An explicit entry in the map
        // wins (even if `None` — that's the "unsupported currency"
        // signal). Otherwise fall back to the global rate.
        let grant_fx_entry = input.fx_rates_by_currency.get(&g.native_currency);
        let (grant_fx, currency_explicitly_missing) = match grant_fx_entry {
            Some(Some(s)) => (parse_decimal(s), false),
            Some(None) => (None, true),
            None => (fallback_fx, false),
        };
        let row = compute_grant(g, input, grant_fx, currency_explicitly_missing);

        // Aggregate only when the grant is complete AND we have a band.
        if row.complete {
            if let Some(ref band) = row.gain_eur_band {
                combined_low += parse_decimal(&band.low).unwrap_or(0.0);
                combined_mid += parse_decimal(&band.mid).unwrap_or(0.0);
                combined_high += parse_decimal(&band.high).unwrap_or(0.0);
                combined_any = true;
            }
        } else if let Some(reason) = row.missing_reason {
            // Only grants with actionable gaps surface in the banner.
            if matches!(
                reason,
                MissingReason::FmvMissing
                    | MissingReason::NoCurrentPrice
                    | MissingReason::UnsupportedCurrency
            ) {
                incomplete_grants.push(row.grant_id);
            }
        }

        per_grant.push(row);
    }

    // The combined band is available iff at least one grant landed a
    // band — in the per-currency world the "global FX" is irrelevant
    // for presence; individual grants drove the aggregate.
    let combined_eur_band = if combined_any {
        Some(EurBand {
            low: format_eur(combined_low),
            mid: format_eur(combined_mid),
            high: format_eur(combined_high),
        })
    } else {
        None
    };

    PaperGainsResult {
        per_grant,
        combined_eur_band,
        incomplete_grants,
    }
}

// ---------------------------------------------------------------------------
// Per-grant computation
// ---------------------------------------------------------------------------

fn compute_grant(
    g: &GrantForPaperGains,
    input: &PaperGainsInput<'_>,
    fx: Option<f64>,
    currency_explicitly_missing: bool,
) -> PerGrantGains {
    // AC-5.4.3 NSO deferral.
    if matches!(g.instrument.as_str(), "nso" | "iso_mapped_to_nso" | "iso") {
        return PerGrantGains {
            grant_id: g.id,
            complete: false,
            gain_native: None,
            gain_eur_band: None,
            missing_reason: Some(MissingReason::NsoDeferred),
        };
    }

    // AC-5.4.4 double-trigger pre-liquidity: a double-trigger RSU without
    // a liquidity_event_date has zero realized shares → gain 0 but flagged
    // incomplete so it is excluded from the aggregate.
    if g.double_trigger && g.liquidity_event_date.is_none() {
        return PerGrantGains {
            grant_id: g.id,
            complete: false,
            gain_native: None,
            gain_eur_band: None,
            missing_reason: Some(MissingReason::DoubleTriggerPreLiquidity),
        };
    }

    // Resolve current price: override wins per AC-5.3.1.
    let price = resolve_price(g, input);
    let Some(price) = price else {
        return PerGrantGains {
            grant_id: g.id,
            complete: false,
            gain_native: None,
            gain_eur_band: None,
            missing_reason: Some(MissingReason::NoCurrentPrice),
        };
    };

    // Per-instrument gain computation.
    let (gain_native, complete) = match g.instrument.as_str() {
        "rsu" => rsu_gain(g, &price, input.today),
        "espp" => espp_gain(g, &price),
        _ => {
            // Unknown instrument — defensive; handler-layer validators
            // should gate this. Exclude with NoCurrentPrice as the least
            // surprising fallback.
            return PerGrantGains {
                grant_id: g.id,
                complete: false,
                gain_native: None,
                gain_eur_band: None,
                missing_reason: Some(MissingReason::NoCurrentPrice),
            };
        }
    };

    if !complete {
        return PerGrantGains {
            grant_id: g.id,
            complete: false,
            gain_native: None,
            gain_eur_band: None,
            missing_reason: Some(MissingReason::FmvMissing),
        };
    }

    // The grant is price+fmv-complete. If the FX for this grant's
    // currency was *explicitly* missing (handler wired an empty entry
    // for it), surface `UnsupportedCurrency` so the banner mentions it
    // rather than silently omitting the grant from the aggregate.
    // This is distinct from the "global fx unavailable" case, which
    // leaves `complete = true, gainEurBand = None` — an implicit
    // fallback handled at the handler layer.
    if currency_explicitly_missing {
        return PerGrantGains {
            grant_id: g.id,
            complete: false,
            gain_native: Some(format_native(gain_native)),
            gain_eur_band: None,
            missing_reason: Some(MissingReason::UnsupportedCurrency),
        };
    }

    let gain_eur_band = fx.map(|fx_rate| apply_bands(gain_native, fx_rate));

    PerGrantGains {
        grant_id: g.id,
        complete: true,
        gain_native: Some(format_native(gain_native)),
        gain_eur_band,
        missing_reason: None,
    }
}

/// Resolve the effective current price for a grant. Returns `None` when
/// no per-grant override exists AND no per-ticker price exists (either
/// because the grant has no ticker or no row was entered).
fn resolve_price(g: &GrantForPaperGains, input: &PaperGainsInput<'_>) -> Option<f64> {
    // Per-grant override wins (AC-5.3.1).
    if let Some(o) = input.grant_overrides.iter().find(|o| o.grant_id == g.id) {
        return parse_decimal(&o.price);
    }
    // Per-ticker fallback. Case-insensitive, trimmed — but the handler
    // is expected to have normalized both the grant.ticker and the
    // ticker_prices.ticker to `UPPER(TRIM(...))` on write.
    let ticker = g.ticker.as_deref()?;
    let row = input
        .ticker_prices
        .iter()
        .find(|t| t.ticker.eq_ignore_ascii_case(ticker))?;
    parse_decimal(&row.price)
}

/// RSU native-currency gain. Returns `(gain, complete)`.
fn rsu_gain(g: &GrantForPaperGains, price: &f64, today: NaiveDate) -> (f64, bool) {
    let mut gain: f64 = 0.0;
    let mut complete = true;
    let mut had_past_row = false;

    for ev in &g.vesting_events {
        if ev.vest_date > today {
            continue;
        }
        had_past_row = true;

        // AC-5.4.4: TimeVestedAwaitingLiquidity contributes zero (shares
        // are not realized). We skip these for the sum but still check
        // FMV for completeness because Slice-4 tax will want it.
        match ev.state {
            VestingState::Vested => {}
            VestingState::TimeVestedAwaitingLiquidity => {
                if ev.fmv_at_vest.is_none() {
                    complete = false;
                }
                continue;
            }
            VestingState::Upcoming => {
                // state machine drift — defensive: treat as not realized.
                continue;
            }
        }

        let Some(fmv_str) = ev.fmv_at_vest.as_deref() else {
            complete = false;
            continue;
        };
        let Some(fmv) = parse_decimal(fmv_str) else {
            complete = false;
            continue;
        };
        let shares = (ev.shares_vested_this_event as f64) / (SHARES_SCALE as f64);
        gain += (price - fmv) * shares;
    }

    // A grant with no past vest rows at all is NOT incomplete — it is
    // simply "nothing vested yet", gain=0. That lets the pre-cliff case
    // render cleanly rather than showing a "complete your data" banner
    // that the user cannot act on.
    if !had_past_row {
        return (0.0, true);
    }

    (gain, complete)
}

/// ESPP native-currency gain. Always complete (if there are purchases,
/// they carry FMV per Slice-2 NOT NULL; an ESPP with no purchases drops
/// to `gain = 0` with `complete = true` — nothing to show, nothing to
/// surface on the banner per AC-5.4.2 last sentence).
fn espp_gain(g: &GrantForPaperGains, price: &f64) -> (f64, bool) {
    let mut gain: f64 = 0.0;
    for p in &g.espp_purchases {
        let Some(fmv) = parse_decimal(&p.fmv_at_purchase) else {
            // Defensive — Slice-2 CHECK enforces NOT NULL; if we
            // somehow see a parse failure, skip the row quietly.
            continue;
        };
        let shares = (p.shares_purchased as f64) / (SHARES_SCALE as f64);
        gain += (price - fmv) * shares;
    }
    (gain, true)
}

/// Apply the 0 / 1.5 / 3 % spread bands per AC-4.6.2.
///
/// `high = gain_native * fx * (1 - 0.00)` (wholesale, best-case)
/// `mid  = gain_native * fx * (1 - 0.015)` (central)
/// `low  = gain_native * fx * (1 - 0.03)` (retail, worst-case)
fn apply_bands(gain_native: f64, fx: f64) -> EurBand {
    let base = gain_native * fx;
    EurBand {
        low: format_eur(base * (1.0 - 0.03)),
        mid: format_eur(base * (1.0 - 0.015)),
        high: format_eur(base),
    }
}

// ---------------------------------------------------------------------------
// Decimal helpers
// ---------------------------------------------------------------------------

fn parse_decimal(s: &str) -> Option<f64> {
    s.trim().parse::<f64>().ok()
}

fn format_eur(v: f64) -> String {
    format!("{v:.2}")
}

fn format_native(v: f64) -> String {
    format!("{v:.4}")
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::vesting::whole_shares;

    fn d(y: i32, m: u32, day: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(y, m, day).unwrap()
    }

    fn rsu_event(date: NaiveDate, shares: i64, fmv: Option<&str>) -> VestingEventForPaperGains {
        VestingEventForPaperGains {
            vest_date: date,
            state: VestingState::Vested,
            shares_vested_this_event: whole_shares(shares),
            fmv_at_vest: fmv.map(|s| s.to_string()),
            fmv_currency: fmv.map(|_| "USD".to_string()),
        }
    }

    fn base_grant(id: Uuid, instrument: &str) -> GrantForPaperGains {
        GrantForPaperGains {
            id,
            instrument: instrument.to_string(),
            native_currency: "USD".to_string(),
            ticker: Some("ACME".to_string()),
            double_trigger: false,
            liquidity_event_date: None,
            vesting_events: Vec::new(),
            espp_purchases: Vec::new(),
        }
    }

    #[test]
    fn nso_grant_is_deferred() {
        let id = Uuid::new_v4();
        let g = base_grant(id, "nso");
        let prices = vec![TickerPriceForPaperGains {
            ticker: "ACME".to_string(),
            price: "45.00".to_string(),
            currency: "USD".to_string(),
        }];
        let input = PaperGainsInput {
            grants: std::slice::from_ref(&g),
            ticker_prices: &prices,
            grant_overrides: &[],
            fx_rate_eur_native: Some("0.93".to_string()),
            fx_rates_by_currency: BTreeMap::new(),
            today: d(2026, 4, 19),
        };
        let result = compute(&input);
        assert_eq!(result.per_grant.len(), 1);
        assert!(!result.per_grant[0].complete);
        assert_eq!(
            result.per_grant[0].missing_reason,
            Some(MissingReason::NsoDeferred)
        );
        assert!(result.incomplete_grants.is_empty());
    }

    #[test]
    fn rsu_complete_grant_produces_band() {
        let id = Uuid::new_v4();
        let mut g = base_grant(id, "rsu");
        g.vesting_events = vec![
            rsu_event(d(2025, 1, 15), 100, Some("40.00")),
            rsu_event(d(2025, 4, 15), 100, Some("42.00")),
        ];
        let prices = vec![TickerPriceForPaperGains {
            ticker: "ACME".to_string(),
            price: "50.00".to_string(),
            currency: "USD".to_string(),
        }];
        let input = PaperGainsInput {
            grants: std::slice::from_ref(&g),
            ticker_prices: &prices,
            grant_overrides: &[],
            fx_rate_eur_native: Some("0.90".to_string()),
            fx_rates_by_currency: BTreeMap::new(),
            today: d(2026, 4, 19),
        };
        let result = compute(&input);
        assert!(result.per_grant[0].complete);
        // (50-40)*100 + (50-42)*100 = 1000 + 800 = 1800 USD
        let native = result.per_grant[0].gain_native.as_ref().unwrap();
        assert_eq!(native, "1800.0000");
        // EUR mid = 1800 * 0.90 * 0.985 = 1595.70
        let band = result.per_grant[0].gain_eur_band.as_ref().unwrap();
        assert_eq!(band.mid, "1595.70");
        assert!(result.combined_eur_band.is_some());
    }

    #[test]
    fn rsu_missing_fmv_surfaces_incomplete() {
        let id = Uuid::new_v4();
        let mut g = base_grant(id, "rsu");
        g.vesting_events = vec![
            rsu_event(d(2025, 1, 15), 100, Some("40.00")),
            rsu_event(d(2025, 4, 15), 100, None),
        ];
        let prices = vec![TickerPriceForPaperGains {
            ticker: "ACME".to_string(),
            price: "50.00".to_string(),
            currency: "USD".to_string(),
        }];
        let input = PaperGainsInput {
            grants: std::slice::from_ref(&g),
            ticker_prices: &prices,
            grant_overrides: &[],
            fx_rate_eur_native: Some("0.90".to_string()),
            fx_rates_by_currency: BTreeMap::new(),
            today: d(2026, 4, 19),
        };
        let result = compute(&input);
        assert!(!result.per_grant[0].complete);
        assert_eq!(
            result.per_grant[0].missing_reason,
            Some(MissingReason::FmvMissing)
        );
        assert_eq!(result.incomplete_grants, vec![id]);
    }

    #[test]
    fn no_current_price_surfaces_incomplete() {
        let id = Uuid::new_v4();
        let mut g = base_grant(id, "rsu");
        g.vesting_events = vec![rsu_event(d(2025, 1, 15), 100, Some("40.00"))];
        let input = PaperGainsInput {
            grants: std::slice::from_ref(&g),
            ticker_prices: &[],
            grant_overrides: &[],
            fx_rate_eur_native: Some("0.90".to_string()),
            fx_rates_by_currency: BTreeMap::new(),
            today: d(2026, 4, 19),
        };
        let result = compute(&input);
        assert!(!result.per_grant[0].complete);
        assert_eq!(
            result.per_grant[0].missing_reason,
            Some(MissingReason::NoCurrentPrice)
        );
        assert_eq!(result.incomplete_grants, vec![id]);
    }

    #[test]
    fn per_grant_override_wins() {
        let id = Uuid::new_v4();
        let mut g = base_grant(id, "rsu");
        g.vesting_events = vec![rsu_event(d(2025, 1, 15), 100, Some("40.00"))];
        let prices = vec![TickerPriceForPaperGains {
            ticker: "ACME".to_string(),
            price: "50.00".to_string(),
            currency: "USD".to_string(),
        }];
        let overrides = vec![GrantPriceOverrideForPaperGains {
            grant_id: id,
            price: "100.00".to_string(),
            currency: "USD".to_string(),
        }];
        let input = PaperGainsInput {
            grants: std::slice::from_ref(&g),
            ticker_prices: &prices,
            grant_overrides: &overrides,
            fx_rate_eur_native: Some("1.00".to_string()),
            fx_rates_by_currency: BTreeMap::new(),
            today: d(2026, 4, 19),
        };
        let result = compute(&input);
        // (100 - 40) * 100 = 6000, not 1000.
        assert_eq!(
            result.per_grant[0].gain_native.as_deref(),
            Some("6000.0000")
        );
    }

    #[test]
    fn double_trigger_pre_liquidity_is_excluded() {
        let id = Uuid::new_v4();
        let mut g = base_grant(id, "rsu");
        g.double_trigger = true;
        g.liquidity_event_date = None;
        g.vesting_events = vec![VestingEventForPaperGains {
            vest_date: d(2025, 1, 15),
            state: VestingState::TimeVestedAwaitingLiquidity,
            shares_vested_this_event: whole_shares(100),
            fmv_at_vest: Some("40.00".into()),
            fmv_currency: Some("USD".into()),
        }];
        let prices = vec![TickerPriceForPaperGains {
            ticker: "ACME".to_string(),
            price: "50.00".to_string(),
            currency: "USD".to_string(),
        }];
        let input = PaperGainsInput {
            grants: std::slice::from_ref(&g),
            ticker_prices: &prices,
            grant_overrides: &[],
            fx_rate_eur_native: Some("0.90".to_string()),
            fx_rates_by_currency: BTreeMap::new(),
            today: d(2026, 4, 19),
        };
        let result = compute(&input);
        assert!(!result.per_grant[0].complete);
        assert_eq!(
            result.per_grant[0].missing_reason,
            Some(MissingReason::DoubleTriggerPreLiquidity)
        );
        assert!(result.incomplete_grants.is_empty());
    }

    #[test]
    fn espp_uses_purchases() {
        let id = Uuid::new_v4();
        let mut g = base_grant(id, "espp");
        g.espp_purchases = vec![EsppPurchaseForPaperGains {
            purchase_date: d(2025, 1, 15),
            shares_purchased: whole_shares(50),
            fmv_at_purchase: "30.00".into(),
            currency: "USD".into(),
        }];
        let prices = vec![TickerPriceForPaperGains {
            ticker: "ACME".to_string(),
            price: "40.00".to_string(),
            currency: "USD".to_string(),
        }];
        let input = PaperGainsInput {
            grants: std::slice::from_ref(&g),
            ticker_prices: &prices,
            grant_overrides: &[],
            fx_rate_eur_native: Some("1.00".to_string()),
            fx_rates_by_currency: BTreeMap::new(),
            today: d(2026, 4, 19),
        };
        let result = compute(&input);
        // (40 - 30) * 50 = 500
        assert_eq!(result.per_grant[0].gain_native.as_deref(), Some("500.0000"));
    }

    #[test]
    fn fx_unavailable_drops_combined_band() {
        let id = Uuid::new_v4();
        let mut g = base_grant(id, "rsu");
        g.vesting_events = vec![rsu_event(d(2025, 1, 15), 100, Some("40.00"))];
        let prices = vec![TickerPriceForPaperGains {
            ticker: "ACME".to_string(),
            price: "50.00".to_string(),
            currency: "USD".to_string(),
        }];
        let input = PaperGainsInput {
            grants: std::slice::from_ref(&g),
            ticker_prices: &prices,
            grant_overrides: &[],
            fx_rate_eur_native: None,
            fx_rates_by_currency: BTreeMap::new(),
            today: d(2026, 4, 19),
        };
        let result = compute(&input);
        assert!(result.per_grant[0].complete);
        assert!(result.per_grant[0].gain_eur_band.is_none());
        assert!(result.combined_eur_band.is_none());
    }

    #[test]
    fn gbp_native_grant_uses_per_currency_fx() {
        let id = Uuid::new_v4();
        let mut g = base_grant(id, "rsu");
        g.native_currency = "GBP".to_string();
        g.ticker = Some("GBCO".to_string());
        g.vesting_events = vec![rsu_event(d(2025, 1, 15), 100, Some("40.00"))];
        let prices = vec![TickerPriceForPaperGains {
            ticker: "GBCO".to_string(),
            price: "50.00".to_string(),
            currency: "GBP".to_string(),
        }];
        let mut fx_map: BTreeMap<String, Option<String>> = BTreeMap::new();
        fx_map.insert("GBP".to_string(), Some("1.20".to_string()));
        let input = PaperGainsInput {
            grants: std::slice::from_ref(&g),
            ticker_prices: &prices,
            grant_overrides: &[],
            // `fx_rate_eur_native` is stale EUR/USD; the per-currency
            // map must win for GBP grants.
            fx_rate_eur_native: Some("0.90".to_string()),
            fx_rates_by_currency: fx_map,
            today: d(2026, 4, 19),
        };
        let result = compute(&input);
        assert!(result.per_grant[0].complete);
        // (50 - 40) * 100 = 1000 GBP. EUR mid = 1000 * 1.20 * 0.985 = 1182.00
        let band = result.per_grant[0].gain_eur_band.as_ref().unwrap();
        assert_eq!(band.mid, "1182.00");
    }

    #[test]
    fn unsupported_currency_surfaces_reason_and_banner() {
        let id = Uuid::new_v4();
        let mut g = base_grant(id, "rsu");
        g.native_currency = "JPY".to_string();
        g.ticker = Some("JPCO".to_string());
        g.vesting_events = vec![rsu_event(d(2025, 1, 15), 100, Some("40.00"))];
        let prices = vec![TickerPriceForPaperGains {
            ticker: "JPCO".to_string(),
            price: "50.00".to_string(),
            currency: "JPY".to_string(),
        }];
        let mut fx_map: BTreeMap<String, Option<String>> = BTreeMap::new();
        fx_map.insert("JPY".to_string(), None);
        let input = PaperGainsInput {
            grants: std::slice::from_ref(&g),
            ticker_prices: &prices,
            grant_overrides: &[],
            fx_rate_eur_native: Some("0.90".to_string()),
            fx_rates_by_currency: fx_map,
            today: d(2026, 4, 19),
        };
        let result = compute(&input);
        assert!(!result.per_grant[0].complete);
        assert_eq!(
            result.per_grant[0].missing_reason,
            Some(MissingReason::UnsupportedCurrency)
        );
        // Native-currency gain is still surfaced for UI context.
        assert_eq!(
            result.per_grant[0].gain_native.as_deref(),
            Some("1000.0000")
        );
        assert!(result.per_grant[0].gain_eur_band.is_none());
        assert_eq!(result.incomplete_grants, vec![id]);
        assert!(result.combined_eur_band.is_none());
    }
}
