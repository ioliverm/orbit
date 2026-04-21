//! Modelo 720 threshold-alert endpoint (Slice 3 T29, ADR-017 §3 +
//! acceptance-criteria §6).
//!
//! Computes the three-category totals (bank accounts, real estate,
//! derived securities) and the threshold breach flags. The derived
//! securities total reuses the paper-gains per-grant pipeline with the
//! FMV-at-vest as the basis and today's ECB mid as the FX (AC-6.1.1).
//!
//! When any past-vest FMV is missing the securities total is `null` —
//! the UI renders the AC-6.1.1 footnote.

use axum::extract::State;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::{Extension, Json};
use chrono::Utc;
use serde_json::json;

use crate::error::AppError;
use crate::middleware::session::SessionAuth;
use crate::state::AppState;

const THRESHOLD_EUR: f64 = 50_000.0;
const FX_BAND_PCT: f64 = 0.05;

/// `GET /api/v1/dashboard/modelo-720-threshold`
pub async fn threshold(
    State(state): State<AppState>,
    Extension(auth): Extension<SessionAuth>,
) -> Result<Response, AppError> {
    let today = Utc::now().date_naive();

    // FX lookup (reference data, pool read).
    let fx = orbit_db::fx_rates::lookup_walkback(&state.pool, "EUR", "USD", today, 7).await?;
    let fx_rate_str = fx.as_ref().map(|r| r.rate.clone());
    let fx_date = fx
        .as_ref()
        .map(|r| r.rate_date.format("%Y-%m-%d").to_string());
    let fx_rate = fx_rate_str.as_deref().and_then(|s| s.parse::<f64>().ok());

    // Gather Slice-2 category totals + Slice-3 derived securities.
    let mut tx = orbit_db::Tx::for_user(&state.pool, auth.user_id).await?;
    let bank = orbit_db::modelo_720_inputs::current(&mut tx, auth.user_id, "bank_accounts").await?;
    let real_estate =
        orbit_db::modelo_720_inputs::current(&mut tx, auth.user_id, "real_estate").await?;

    // Derived securities: walk every grant, sum
    // (shares_vested_to_date * fmv_at_vest) + (shares_purchased * fmv_at_purchase)
    // then convert to EUR. If any past-vest row on any eligible grant
    // carries NULL fmv, securities_total = null (AC-6.1.1 footnote).
    let grants = orbit_db::grants::list_grants(&mut tx, auth.user_id).await?;
    let mut securities_native: f64 = 0.0;
    let mut securities_complete = true;

    for g in &grants {
        // NSO/ISO excluded from Slice-3 derivation (same as paper-gains).
        if matches!(g.instrument.as_str(), "nso" | "iso_mapped_to_nso" | "iso") {
            continue;
        }
        // Double-trigger pre-liquidity excluded (zero realized).
        if g.double_trigger && g.liquidity_event_date.is_none() {
            continue;
        }

        if g.instrument == "espp" {
            let purchases =
                orbit_db::espp_purchases::list_for_grant(&mut tx, auth.user_id, g.id).await?;
            for p in purchases {
                if let Ok(fmv) = p.fmv_at_purchase.parse::<f64>() {
                    let shares = (p.shares_purchased as f64) / (orbit_core::SHARES_SCALE as f64);
                    securities_native += fmv * shares;
                }
            }
        } else if g.instrument == "rsu" {
            let events =
                orbit_db::vesting_events::list_for_grant(&mut tx, auth.user_id, g.id).await?;
            for e in events {
                if e.vest_date > today {
                    continue;
                }
                match e.fmv_at_vest.as_deref().and_then(|s| s.parse::<f64>().ok()) {
                    Some(fmv) => {
                        let shares =
                            (e.shares_vested_this_event as f64) / (orbit_core::SHARES_SCALE as f64);
                        securities_native += fmv * shares;
                    }
                    None => {
                        securities_complete = false;
                    }
                }
            }
        }
    }
    tx.commit().await?;

    let securities_eur: Option<f64> = match (securities_complete, fx_rate) {
        (true, Some(rate)) => Some(securities_native * rate),
        _ => None,
    };

    let bank_eur: Option<f64> = bank.and_then(|r| r.amount_eur.parse::<f64>().ok());
    let real_estate_eur: Option<f64> = real_estate.and_then(|r| r.amount_eur.parse::<f64>().ok());

    let per_category = [bank_eur, real_estate_eur, securities_eur];
    let aggregate: f64 = per_category.iter().map(|v| v.unwrap_or(0.0)).sum();
    let per_category_breach = per_category
        .iter()
        .any(|v| v.map(|x| x >= THRESHOLD_EUR).unwrap_or(false));
    let aggregate_breach = aggregate >= THRESHOLD_EUR;

    // FX sensitivity band: render ±5% of the USD-denominated portion.
    // For Slice 3 that's just the securities_eur figure (bank + real
    // estate are user-entered EUR). Compute only when we have a rate
    // and the securities total.
    let fx_sensitivity_band = match (fx_rate, securities_eur) {
        (Some(_rate), Some(s)) if (s - THRESHOLD_EUR).abs() < THRESHOLD_EUR * FX_BAND_PCT => {
            Some(json!({
                "low": format!("{:.2}", s * (1.0 - FX_BAND_PCT)),
                "mid": format!("{:.2}", s),
                "high": format!("{:.2}", s * (1.0 + FX_BAND_PCT)),
            }))
        }
        _ => None,
    };

    Ok((
        StatusCode::OK,
        Json(json!({
            "bankAccountsEur": bank_eur.map(|v| format!("{v:.2}")),
            "realEstateEur": real_estate_eur.map(|v| format!("{v:.2}")),
            "securitiesEur": securities_eur.map(|v| format!("{v:.2}")),
            "perCategoryBreach": per_category_breach,
            "aggregateBreach": aggregate_breach,
            "thresholdEur": format!("{THRESHOLD_EUR:.2}"),
            "fxSensitivityBand": fx_sensitivity_band,
            "fxDate": fx_date,
        })),
    )
        .into_response())
}
