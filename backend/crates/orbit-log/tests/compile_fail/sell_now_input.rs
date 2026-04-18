// Logging a `SellNowInput` must be rejected at compile time (SEC-050).
use orbit_log::{event, Level};
use orbit_tax_core::SellNowInput;

fn main() {
    let s = SellNowInput;
    event!(Level::Info, "sell_now_preview", input = s);
}
