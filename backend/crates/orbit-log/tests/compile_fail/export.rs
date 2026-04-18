// Logging an `Export` must be rejected at compile time (SEC-050).
use orbit_log::{event, Level};
use orbit_tax_core::Export;

fn main() {
    let e = Export;
    event!(Level::Info, "export_ready", export = e);
}
