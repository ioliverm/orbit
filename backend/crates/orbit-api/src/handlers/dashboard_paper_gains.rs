//! Dashboard paper-gains tile endpoint (Slice 3 T29, ADR-017 §5).
//!
//! `GET /api/v1/dashboard/paper-gains` aggregates the user's grants,
//! `vesting_events`, `ticker_current_prices`, `grant_current_price_overrides`,
//! and today's EUR/USD fx rate, then calls the pure
//! `orbit_core::compute_paper_gains` function.
//!
//! On FX unavailable the response still lands (200) with
//! `stalenessFx = "unavailable"` and a null `combinedEurBand` per AC-5.5.4.

use axum::extract::State;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::{Extension, Json};
use chrono::Utc;
use orbit_core::{
    compute_paper_gains, EsppPurchaseForPaperGains, GrantForPaperGains,
    GrantPriceOverrideForPaperGains, PaperGainsInput, TickerPriceForPaperGains,
    VestingEventForPaperGains,
};
use serde_json::json;

use crate::error::AppError;
use crate::middleware::session::SessionAuth;
use crate::state::AppState;

/// `GET /api/v1/dashboard/paper-gains`
pub async fn paper_gains(
    State(state): State<AppState>,
    Extension(auth): Extension<SessionAuth>,
) -> Result<Response, AppError> {
    let today = Utc::now().date_naive();

    // FX lookup (pool-level, not RLS-scoped).
    let fx = orbit_db::fx_rates::lookup_walkback(&state.pool, "EUR", "USD", today, 7).await?;
    let fx_rate = fx.as_ref().map(|r| r.rate.clone());
    let fx_date = fx
        .as_ref()
        .map(|r| r.rate_date.format("%Y-%m-%d").to_string());
    let staleness = match fx.as_ref() {
        None => "unavailable",
        Some(r) => match r.staleness {
            orbit_db::fx_rates::Staleness::Fresh => "fresh",
            orbit_db::fx_rates::Staleness::Walkback => "walkback",
            orbit_db::fx_rates::Staleness::Stale => "stale",
            orbit_db::fx_rates::Staleness::Unavailable => "unavailable",
        },
    };

    // Per-user tx: grants + vesting_events + espp_purchases + current
    // prices + grant overrides. Same scanning discipline as the Slice-2
    // stacked dashboard; Slice 3 adds three read round-trips.
    let mut tx = orbit_db::Tx::for_user(&state.pool, auth.user_id).await?;
    let grants = orbit_db::grants::list_grants(&mut tx, auth.user_id).await?;

    let mut pg_grants: Vec<GrantForPaperGains> = Vec::with_capacity(grants.len());
    for g in &grants {
        let events = orbit_db::vesting_events::list_for_grant(&mut tx, auth.user_id, g.id).await?;
        let vs: Vec<VestingEventForPaperGains> = events
            .iter()
            .map(|e| VestingEventForPaperGains {
                vest_date: e.vest_date,
                state: e.state,
                shares_vested_this_event: e.shares_vested_this_event,
                fmv_at_vest: e.fmv_at_vest.clone(),
                fmv_currency: e.fmv_currency.clone(),
            })
            .collect();
        let purchases: Vec<EsppPurchaseForPaperGains> = if g.instrument == "espp" {
            orbit_db::espp_purchases::list_for_grant(&mut tx, auth.user_id, g.id)
                .await?
                .into_iter()
                .map(|p| EsppPurchaseForPaperGains {
                    purchase_date: p.purchase_date,
                    shares_purchased: p.shares_purchased,
                    fmv_at_purchase: p.fmv_at_purchase,
                    currency: p.currency,
                })
                .collect()
        } else {
            Vec::new()
        };
        pg_grants.push(GrantForPaperGains {
            id: g.id,
            instrument: g.instrument.clone(),
            native_currency: g
                .strike_currency
                .clone()
                .unwrap_or_else(|| "USD".to_string()),
            ticker: g.ticker.clone(),
            double_trigger: g.double_trigger,
            liquidity_event_date: g.liquidity_event_date,
            vesting_events: vs,
            espp_purchases: purchases,
        });
    }

    let ticker_rows = orbit_db::ticker_current_prices::list_for_user(&mut tx, auth.user_id).await?;
    let ticker_prices: Vec<TickerPriceForPaperGains> = ticker_rows
        .into_iter()
        .map(|r| TickerPriceForPaperGains {
            ticker: r.ticker,
            price: r.price,
            currency: r.currency,
        })
        .collect();

    // Per-grant overrides — one query per grant (no list_all helper in
    // orbit-db today; keeping this consistent with the repo's existing
    // single-grant read shape).
    let mut grant_overrides: Vec<GrantPriceOverrideForPaperGains> = Vec::new();
    for g in &grants {
        if let Some(row) =
            orbit_db::grant_current_price_overrides::get(&mut tx, auth.user_id, g.id).await?
        {
            grant_overrides.push(GrantPriceOverrideForPaperGains {
                grant_id: row.grant_id,
                price: row.price,
                currency: row.currency,
            });
        }
    }

    tx.commit().await?;

    let input = PaperGainsInput {
        grants: &pg_grants,
        ticker_prices: &ticker_prices,
        grant_overrides: &grant_overrides,
        fx_rate_eur_native: fx_rate,
        today,
    };
    let result = compute_paper_gains(&input);

    // Build per-grant DTOs with the employer + instrument metadata the
    // UI needs to render the banner labels without a second round-trip.
    let per_grant_dto: Vec<serde_json::Value> = result
        .per_grant
        .iter()
        .map(|p| {
            let g = grants.iter().find(|g| g.id == p.grant_id);
            json!({
                "grantId": p.grant_id,
                "employer": g.map(|g| g.employer_name.clone()),
                "instrument": g.map(|g| g.instrument.clone()),
                "complete": p.complete,
                "gainNative": p.gain_native,
                "gainEurBand": p.gain_eur_band,
                "missingReason": p.missing_reason,
            })
        })
        .collect();

    let incomplete_dto: Vec<serde_json::Value> = result
        .incomplete_grants
        .iter()
        .map(|id| {
            let g = grants.iter().find(|g| g.id == *id);
            json!({
                "grantId": id,
                "employer": g.map(|g| g.employer_name.clone()),
                "instrument": g.map(|g| g.instrument.clone()),
            })
        })
        .collect();

    Ok((
        StatusCode::OK,
        Json(json!({
            "perGrant": per_grant_dto,
            "combinedEurBand": result.combined_eur_band,
            "incompleteGrants": incomplete_dto,
            "stalenessFx": staleness,
            "fxDate": fx_date,
        })),
    )
        .into_response())
}
