//! ECB FX fetch + parse + upsert (Slice 3 T29).
//!
//! ADR-007 authoritative for semantics; ADR-017 §4 pins the exact worker
//! shape. Two endpoints:
//!
//!   * `eurofxref-daily.xml` — the rate published today (weekdays only,
//!     excluding TARGET holidays). Used by the scheduler at 17:00
//!     Europe/Madrid.
//!   * `eurofxref-hist-90d.xml` — a 90-day rolling history, consumed once
//!     on cold-start bootstrap.
//!
//! Both XML documents share a `<gesmes:Envelope>` → `<Cube>` →
//! `<Cube time=YYYY-MM-DD>` → `<Cube currency=XXX rate=Y.YYYY>` shape.
//! The hand-rolled parser below walks the text for the opening `<Cube`
//! tags and scrapes the `time`, `currency`, and `rate` attributes — the
//! format has been stable for 20+ years (ADR-017 §10.1), so a dedicated
//! `quick-xml` dependency is overkill for Slice 3. A format drift would
//! produce an `fx.fetch_failure` audit row with `reason = "parse"` and
//! the walkback logic covers the data gap (SEC-050 + ADR-007).

use chrono::{DateTime, NaiveDate, Utc};
use orbit_db::Tx;
use sqlx::PgPool;
use std::time::Duration;

const ECB_DAILY_URL: &str = "https://www.ecb.europa.eu/stats/eurofxref/eurofxref-daily.xml";
const ECB_BOOTSTRAP_URL: &str = "https://www.ecb.europa.eu/stats/eurofxref/eurofxref-hist-90d.xml";

/// Which fetch to run.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FetchKind {
    Daily,
    Bootstrap,
}

impl FetchKind {
    pub fn as_str(self) -> &'static str {
        match self {
            FetchKind::Daily => "daily",
            FetchKind::Bootstrap => "bootstrap",
        }
    }
}

/// Summary of a successful fetch — used by the CLI and by callers that
/// want to log outcome fields after `run_once`.
#[derive(Debug, Clone)]
pub struct FetchOutcome {
    pub kind: FetchKind,
    pub rows_inserted: u64,
    pub oldest_date: Option<NaiveDate>,
    pub newest_date: Option<NaiveDate>,
}

/// Errors the fetch pipeline surfaces. `classify()` maps these to the
/// audit-payload `reason` allowlist.
#[derive(Debug, thiserror::Error)]
pub enum FetchError {
    #[error("network: {0}")]
    Network(String),
    #[error("timeout")]
    Timeout,
    #[error("parse: {0}")]
    Parse(String),
    #[error("db: {0}")]
    Db(#[from] sqlx::Error),
    #[error("orbit-db: {0}")]
    OrbitDb(#[from] orbit_db::Error),
}

impl FetchError {
    pub fn classify(&self) -> &'static str {
        match self {
            FetchError::Timeout => "timeout",
            FetchError::Network(_) => "network",
            FetchError::Parse(_) => "parse",
            FetchError::Db(_) | FetchError::OrbitDb(_) => "db",
        }
    }
}

/// One parsed rate from the ECB XML.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EcbRate {
    pub rate_date: NaiveDate,
    pub quote: String,
    pub rate: String,
}

// ---------------------------------------------------------------------------
// HTTP fetch helpers
// ---------------------------------------------------------------------------

pub async fn fetch_daily(http: &reqwest::Client) -> Result<Vec<EcbRate>, FetchError> {
    let body = fetch_xml(http, ECB_DAILY_URL).await?;
    parse_ecb_xml(&body)
}

pub async fn fetch_bootstrap(http: &reqwest::Client) -> Result<Vec<EcbRate>, FetchError> {
    let body = fetch_xml(http, ECB_BOOTSTRAP_URL).await?;
    parse_ecb_xml(&body)
}

async fn fetch_xml(http: &reqwest::Client, url: &str) -> Result<String, FetchError> {
    let resp = tokio::time::timeout(Duration::from_secs(5), http.get(url).send())
        .await
        .map_err(|_| FetchError::Timeout)?
        .map_err(|e| {
            if e.is_timeout() {
                FetchError::Timeout
            } else {
                FetchError::Network(e.to_string())
            }
        })?;
    if !resp.status().is_success() {
        return Err(FetchError::Network(format!(
            "HTTP {}",
            resp.status().as_u16()
        )));
    }
    tokio::time::timeout(Duration::from_secs(5), resp.text())
        .await
        .map_err(|_| FetchError::Timeout)?
        .map_err(|e| FetchError::Network(e.to_string()))
}

// ---------------------------------------------------------------------------
// Parser
// ---------------------------------------------------------------------------

/// Minimal hand-rolled parser for the ECB XML. Scans for opening
/// `<Cube ` tags, extracts `time`, `currency`, `rate` attributes.
///
/// The daily file has a single `<Cube time="...">` container with
/// per-currency children. The 90-day file has many containers, each
/// with per-currency children. We walk the text top-to-bottom tracking
/// the most-recent `time=` attribute and pairing it with subsequent
/// `<Cube currency=... rate=...>` children until the next `time=`
/// appears.
pub fn parse_ecb_xml(body: &str) -> Result<Vec<EcbRate>, FetchError> {
    let mut rates: Vec<EcbRate> = Vec::new();
    let mut current_date: Option<NaiveDate> = None;

    // Walk every `<Cube ` opening tag.
    for slice in body.split("<Cube").skip(1) {
        // The attribute block is the text up to the first '>' or '/>'.
        let Some(end_idx) = slice.find('>') else {
            continue;
        };
        let attrs = &slice[..end_idx];

        let time_attr = find_attr(attrs, "time");
        let currency_attr = find_attr(attrs, "currency");
        let rate_attr = find_attr(attrs, "rate");

        if let Some(t) = time_attr {
            match NaiveDate::parse_from_str(t, "%Y-%m-%d") {
                Ok(d) => current_date = Some(d),
                Err(e) => {
                    return Err(FetchError::Parse(format!("bad time={t:?}: {e}")));
                }
            }
        }

        if let (Some(cur), Some(r)) = (currency_attr, rate_attr) {
            let Some(d) = current_date else {
                return Err(FetchError::Parse(
                    "currency Cube without preceding time= container".into(),
                ));
            };
            // Defensive: only accept 3-letter ISO quotes; the CHECK on
            // `fx_rates.quote` enforces this at the DB layer too.
            if cur.len() != 3 {
                continue;
            }
            // Defensive: reject non-numeric rates now rather than let
            // Postgres reject them per-row.
            if r.parse::<f64>().is_err() {
                return Err(FetchError::Parse(format!(
                    "bad rate={r:?} for currency {cur}"
                )));
            }
            rates.push(EcbRate {
                rate_date: d,
                quote: cur.to_string(),
                rate: r.to_string(),
            });
        }
    }

    Ok(rates)
}

fn find_attr<'a>(attrs: &'a str, name: &str) -> Option<&'a str> {
    // Look for `name="..."`. Simple + sufficient for the ECB format.
    let needle = format!("{name}=\"");
    let start = attrs.find(&needle)? + needle.len();
    let end_rel = attrs[start..].find('"')?;
    Some(&attrs[start..start + end_rel])
}

// ---------------------------------------------------------------------------
// Upsert + audit
// ---------------------------------------------------------------------------

/// Run the daily fetch with exponential backoff (1s / 4s / 16s) and
/// upsert the resulting rows in a single transaction. Emits a
/// `fx.fetch_success` audit row on success; the caller handles the
/// failure-audit path.
///
/// Returns a [`FetchOutcome`] on success.
pub async fn run_daily_with_retry(
    pool: &PgPool,
    http: &reqwest::Client,
) -> Result<FetchOutcome, FetchError> {
    orbit_log::event!(orbit_log::Level::Info, "fx.fetch_start", attempt = 1u64);

    let mut last_err: Option<FetchError> = None;
    for attempt in 0..3u32 {
        if attempt > 0 {
            let backoff = match attempt {
                1 => Duration::from_secs(1),
                2 => Duration::from_secs(4),
                _ => Duration::from_secs(16),
            };
            tokio::time::sleep(backoff).await;
        }
        match fetch_daily(http).await {
            Ok(rates) => {
                let outcome = upsert_batch(pool, FetchKind::Daily, rates).await?;
                orbit_log::event!(
                    orbit_log::Level::Info,
                    "fx.fetch_success",
                    kind = "daily",
                    rows_inserted = outcome.rows_inserted
                );
                return Ok(outcome);
            }
            Err(e) => {
                last_err = Some(e);
            }
        }
    }
    let err = last_err.unwrap_or(FetchError::Network("unknown".into()));
    orbit_log::event!(
        orbit_log::Level::Warn,
        "fx.fetch_failure",
        kind = "daily",
        reason = err.classify()
    );
    Err(err)
}

/// Run the 90-day bootstrap fetch (single attempt — ADR-007 does not
/// retry here; a failing bootstrap does not block the scheduler,
/// the daily path will fill in as it runs).
pub async fn run_bootstrap(
    pool: &PgPool,
    http: &reqwest::Client,
) -> Result<FetchOutcome, FetchError> {
    orbit_log::event!(orbit_log::Level::Info, "fx.bootstrap_start");
    let rates = fetch_bootstrap(http).await.inspect_err(|e| {
        orbit_log::event!(
            orbit_log::Level::Warn,
            "fx.fetch_failure",
            kind = "bootstrap",
            reason = e.classify()
        );
    })?;
    let outcome = upsert_batch(pool, FetchKind::Bootstrap, rates).await?;
    orbit_log::event!(
        orbit_log::Level::Info,
        "fx.bootstrap_success",
        rows_inserted = outcome.rows_inserted,
        span_days = outcome
            .newest_date
            .zip(outcome.oldest_date)
            .map(|(n, o)| (n - o).num_days())
            .unwrap_or(0)
    );
    Ok(outcome)
}

/// Upsert every rate in a single system-scoped transaction. Audit row
/// rides inside the same tx (SEC-101 atomicity; ADR-017 §4). Only
/// `quote = "USD"` rows are inserted for Slice 3 (AC-4.1.2 — Orbit's
/// v1 ingestion whitelist); other rates in the XML are ignored.
pub async fn upsert_batch(
    pool: &PgPool,
    kind: FetchKind,
    rates: Vec<EcbRate>,
) -> Result<FetchOutcome, FetchError> {
    let now: DateTime<Utc> = Utc::now();
    let mut tx = Tx::system(pool).await?;
    let mut rows_inserted: u64 = 0;
    let mut oldest: Option<NaiveDate> = None;
    let mut newest: Option<NaiveDate> = None;
    let mut newest_usd: Option<NaiveDate> = None;

    for r in rates.iter().filter(|r| r.quote == "USD") {
        oldest = Some(oldest.map_or(r.rate_date, |d| d.min(r.rate_date)));
        newest = Some(newest.map_or(r.rate_date, |d| d.max(r.rate_date)));
        newest_usd = Some(newest_usd.map_or(r.rate_date, |d| d.max(r.rate_date)));
        let n = orbit_db::fx_rates::upsert_ecb(&mut tx, "EUR", &r.quote, r.rate_date, &r.rate, now)
            .await?;
        rows_inserted += n;
    }

    crate::record_success_audit_in_tx(
        &mut tx,
        kind,
        rows_inserted,
        newest_usd,
        oldest.zip(newest).map(|(o, n)| (n - o).num_days()),
    )
    .await?;

    tx.commit().await?;

    Ok(FetchOutcome {
        kind,
        rows_inserted,
        oldest_date: oldest,
        newest_date: newest,
    })
}

// ---------------------------------------------------------------------------
// Tests — the parser is pure and covered here; HTTP + DB round-trips run
// against a live Postgres in the integration suite and against a real
// ECB request under the `orbit worker --once fx` runbook step.
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    const DAILY_SAMPLE: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<gesmes:Envelope xmlns:gesmes="http://www.gesmes.org/xml/2002-08-01" xmlns="http://www.ecb.int/vocabulary/2002-08-01/eurofxref">
  <gesmes:subject>Reference rates</gesmes:subject>
  <gesmes:Sender>
    <gesmes:name>European Central Bank</gesmes:name>
  </gesmes:Sender>
  <Cube>
    <Cube time="2026-04-17">
      <Cube currency="USD" rate="1.0823"/>
      <Cube currency="JPY" rate="163.45"/>
      <Cube currency="GBP" rate="0.8523"/>
    </Cube>
  </Cube>
</gesmes:Envelope>"#;

    const HIST_SAMPLE: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<gesmes:Envelope>
  <Cube>
    <Cube time="2026-04-17">
      <Cube currency="USD" rate="1.0823"/>
    </Cube>
    <Cube time="2026-04-16">
      <Cube currency="USD" rate="1.0810"/>
    </Cube>
    <Cube time="2026-04-15">
      <Cube currency="USD" rate="1.0790"/>
    </Cube>
  </Cube>
</gesmes:Envelope>"#;

    #[test]
    fn parses_daily_with_multiple_currencies() {
        let rates = parse_ecb_xml(DAILY_SAMPLE).unwrap();
        assert_eq!(rates.len(), 3);
        let usd = rates.iter().find(|r| r.quote == "USD").unwrap();
        assert_eq!(usd.rate_date, NaiveDate::from_ymd_opt(2026, 4, 17).unwrap());
        assert_eq!(usd.rate, "1.0823");
    }

    #[test]
    fn parses_historical_with_multiple_dates() {
        let rates = parse_ecb_xml(HIST_SAMPLE).unwrap();
        assert_eq!(rates.len(), 3);
        let dates: Vec<_> = rates.iter().map(|r| r.rate_date).collect();
        assert!(dates.contains(&NaiveDate::from_ymd_opt(2026, 4, 17).unwrap()));
        assert!(dates.contains(&NaiveDate::from_ymd_opt(2026, 4, 16).unwrap()));
        assert!(dates.contains(&NaiveDate::from_ymd_opt(2026, 4, 15).unwrap()));
    }

    #[test]
    fn rejects_bad_rate() {
        let body = r#"<Cube><Cube time="2026-04-17"><Cube currency="USD" rate="not-a-number"/></Cube></Cube>"#;
        let err = parse_ecb_xml(body).unwrap_err();
        assert!(matches!(err, FetchError::Parse(_)));
    }

    #[test]
    fn rejects_bad_date() {
        let body = r#"<Cube><Cube time="nope"><Cube currency="USD" rate="1.00"/></Cube></Cube>"#;
        let err = parse_ecb_xml(body).unwrap_err();
        assert!(matches!(err, FetchError::Parse(_)));
    }

    #[test]
    fn classify_reason_names_match_audit_allowlist() {
        assert_eq!(FetchError::Timeout.classify(), "timeout");
        assert_eq!(FetchError::Network("x".into()).classify(), "network");
        assert_eq!(FetchError::Parse("x".into()).classify(), "parse");
    }
}
