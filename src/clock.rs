//! Wall-clock helpers shared by the CLI, daemon, and caches.

use std::time::{SystemTime, UNIX_EPOCH};

/// Milliseconds since the Unix epoch, saturating at `i64::MAX` and returning
/// zero for clocks set before the epoch.
pub fn now_ms() -> i64 {
    i64::try_from(since_epoch().as_millis()).unwrap_or(i64::MAX)
}

/// Whole seconds since the Unix epoch, or zero for clocks set before it.
pub fn now_secs() -> u64 {
    since_epoch().as_secs()
}

fn since_epoch() -> std::time::Duration {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
}
