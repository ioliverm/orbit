// Logging a `Money` must be rejected at compile time (SEC-050).
use orbit_core::Money;
use orbit_log::{event, Level};

fn main() {
    let m = Money;
    event!(Level::Info, "priced", amount = m);
}
