// Copyright 2025 eraflo
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0

//! Hub-local presentation helpers.
//!
//! The widget vocabulary itself lives in `khora_tool_ui::widgets`, shared with
//! the editor. What stays here is the handful of things that are specific to
//! the hub's data rather than to the look.

/// Formats a unix timestamp as a short relative string ("2 h ago").
///
/// Relative time is what the reader actually wants on a project card — the
/// exact date is noise until they need it.
pub fn format_ts(ts: u64) -> String {
    if ts == 0 {
        return "never".to_owned();
    }
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let diff = now.saturating_sub(ts);

    if diff < 60 {
        "just now".to_owned()
    } else if diff < 3_600 {
        format!("{} min ago", diff / 60)
    } else if diff < 86_400 {
        format!("{} h ago", diff / 3_600)
    } else if diff < 7 * 86_400 {
        format!("{} d ago", diff / 86_400)
    } else {
        format!("{} w ago", diff / (7 * 86_400))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ago(secs: u64) -> String {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();
        format_ts(now - secs)
    }

    #[test]
    fn zero_means_never_not_the_epoch() {
        assert_eq!(format_ts(0), "never");
    }

    #[test]
    fn scales_the_unit_to_the_distance() {
        assert_eq!(ago(30), "just now");
        assert_eq!(ago(5 * 60), "5 min ago");
        assert_eq!(ago(3 * 3_600), "3 h ago");
        assert_eq!(ago(2 * 86_400), "2 d ago");
        assert_eq!(ago(3 * 7 * 86_400), "3 w ago");
    }

    /// A timestamp from the future (clock skew, a restored backup) must not
    /// underflow into a nonsense duration.
    #[test]
    fn future_timestamps_do_not_underflow() {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();
        assert_eq!(format_ts(now + 10_000), "just now");
    }
}
