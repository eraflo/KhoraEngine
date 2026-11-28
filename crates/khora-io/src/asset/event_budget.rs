// Copyright 2025 eraflo
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

//! The change events a watched root lets through each second.

use std::time::{Duration, Instant};

/// How long a burst that dropped events must be quiet before its rescan.
const QUIET: Duration = Duration::from_secs(1);

/// A token bucket of the change events a watched root lets through each second.
///
/// Past it, events are dropped; once the burst has been quiet a second, one
/// rescan is due.
#[derive(Debug)]
pub struct EventBudget {
    per_second: f64,
    /// The events that may pass now.
    tokens: f64,
    /// When the tokens were last counted.
    counted: Option<Instant>,
    /// When an event was last dropped, until the rescan it calls for is taken.
    dropped: Option<Instant>,
}

impl EventBudget {
    /// Returns a budget of `per_second` events a second, full at first.
    pub fn new(per_second: u32) -> Self {
        let per_second = f64::from(per_second.max(1));
        Self {
            per_second,
            tokens: per_second,
            counted: None,
            dropped: None,
        }
    }

    /// Returns whether an event at `now` is let through.
    pub fn admit(&mut self, now: Instant) -> bool {
        if let Some(counted) = self.counted {
            let refilled = now.saturating_duration_since(counted).as_secs_f64() * self.per_second;
            self.tokens = (self.tokens + refilled).min(self.per_second);
        }
        self.counted = Some(now);
        if self.tokens >= 1.0 {
            self.tokens -= 1.0;
            true
        } else {
            self.dropped = Some(now);
            false
        }
    }

    /// Returns whether a rescan is due at `now`: true once, after a burst that
    /// dropped events has been quiet a second.
    pub fn take_rescan(&mut self, now: Instant) -> bool {
        match self.dropped {
            Some(dropped) if now.saturating_duration_since(dropped) >= QUIET => {
                self.dropped = None;
                true
            }
            _ => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, Instant};

    /// Lets events through at `now` until one is dropped; how many were.
    fn exhaust(budget: &mut EventBudget, now: Instant) -> usize {
        let mut admitted = 0;
        while budget.admit(now) {
            admitted += 1;
            assert!(admitted <= 4096, "a budget lets a finite number through");
        }
        admitted
    }

    /// **A flood of watcher events collapses into a rescan.**
    ///
    /// 512 events in a second pass, the 513th is dropped; exactly one rescan
    /// is due once the burst has been quiet a second, then events pass again.
    #[test]
    fn a_flood_of_watcher_events_collapses_into_a_rescan() {
        let start = Instant::now();
        let mut budget = EventBudget::new(512);

        for event in 0..512 {
            assert!(budget.admit(start), "event {event} of the first 512");
        }
        assert!(!budget.admit(start), "the 513th in the same second");

        assert!(!budget.take_rescan(start), "no rescan while dropping");
        assert!(
            !budget.take_rescan(start + Duration::from_millis(999)),
            "none before the burst has been quiet a second"
        );
        assert!(
            budget.take_rescan(start + Duration::from_millis(1001)),
            "one rescan once it has"
        );
        assert!(
            !budget.take_rescan(start + Duration::from_millis(1002)),
            "and only one"
        );
        assert!(!budget.take_rescan(start + Duration::from_secs(5)));
        assert!(
            budget.admit(start + Duration::from_secs(5)),
            "a quiet budget lets events through again"
        );
    }

    /// Events that never exceed the budget ask for no rescan.
    #[test]
    fn events_within_the_budget_ask_for_no_rescan() {
        let start = Instant::now();
        let mut budget = EventBudget::new(512);
        for millis in 0..100 {
            assert!(budget.admit(start + Duration::from_millis(millis)));
        }

        assert!(!budget.take_rescan(start + Duration::from_secs(2)));
    }

    /// An event dropped during the quiet second starts the second again: the
    /// rescan waits a second after the last drop.
    #[test]
    fn a_drop_during_the_quiet_second_delays_the_rescan() {
        let start = Instant::now();
        let mut budget = EventBudget::new(512);
        assert_eq!(exhaust(&mut budget, start), 512);
        let later = start + Duration::from_millis(500);
        exhaust(&mut budget, later);

        assert!(
            !budget.take_rescan(start + Duration::from_millis(1001)),
            "a drop half a second in"
        );
        assert!(budget.take_rescan(later + Duration::from_millis(1001)));
    }
}
