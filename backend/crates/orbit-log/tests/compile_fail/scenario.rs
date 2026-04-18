// Logging a `Scenario` must be rejected at compile time (SEC-050).
use orbit_log::{event, Level};
use orbit_tax_core::Scenario;

fn main() {
    let s = Scenario;
    event!(Level::Info, "scenario_run", scenario = s);
}
