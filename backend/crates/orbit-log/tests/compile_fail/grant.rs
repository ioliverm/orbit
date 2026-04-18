// Logging a `Grant` must be rejected at compile time (SEC-050).
use orbit_log::{event, Level};
use orbit_tax_core::Grant;

fn main() {
    let g = Grant;
    event!(Level::Info, "grant_created", grant = g);
}
