//! Autosave and preview timing.

use std::time::Duration;

pub const AUTOSAVE_DELAY: Duration = Duration::from_secs(1);
pub const PREVIEW_DELAY: Duration = Duration::from_millis(150);
const MAX_RETRY_DELAY: Duration = Duration::from_secs(30);

/// Delay before retry `attempt` (0-based): 1s, 2s, 4s ... capped at 30s.
pub fn retry_delay(attempt: u32) -> Duration {
    Duration::from_secs(1 << attempt.min(5)).min(MAX_RETRY_DELAY)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn backoff_doubles_then_caps() {
        let secs: Vec<u64> = [0, 1, 2, 4, 5, 40]
            .iter()
            .map(|&a| retry_delay(a).as_secs())
            .collect();
        assert_eq!(secs, [1, 2, 4, 16, 30, 30]);
    }
}
