// Raw (non-'static) `&str` must be rejected. Callers must wrap with
// `SafeString` or pass a string literal. (SEC-050)
use orbit_log::{event, Level};

fn main() {
    let owned: String = "untrusted".to_string();
    let s: &str = owned.as_str();
    event!(Level::Info, "event", field = s);
}
