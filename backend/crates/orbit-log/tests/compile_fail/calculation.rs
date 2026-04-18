// Logging a `Calculation` must be rejected at compile time (SEC-050).
use orbit_log::{event, Level};
use orbit_tax_core::Calculation;

fn main() {
    let c = Calculation;
    event!(Level::Info, "calc_done", calc = c);
}
