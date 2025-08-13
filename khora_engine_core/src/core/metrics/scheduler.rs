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

use crate::core::timer::Stopwatch;

/// Scheduler for periodic metrics operations like logging summaries
#[derive(Debug)]
pub struct MetricsScheduler {
    last_summary_time: Stopwatch,
    summary_interval_secs: f64,
}

impl MetricsScheduler {
    /// Creates a new metrics scheduler with the specified summary interval
    pub fn new(summary_interval_secs: f64) -> Self {
        Self {
            last_summary_time: Stopwatch::new(),
            summary_interval_secs,
        }
    }

    /// Creates a new metrics scheduler with default 10-second summary interval
    pub fn with_default_interval() -> Self {
        Self::new(10.0)
    }

    /// Checks if it's time to log a metrics summary
    /// Returns true if the interval has elapsed
    pub fn should_log_summary(&self) -> bool {
        let time_since_last = self.last_summary_time.elapsed_secs_f64().unwrap_or(0.0);
        time_since_last >= self.summary_interval_secs
    }

    /// Marks that a summary has been logged, resetting the timer
    pub fn mark_summary_logged(&mut self) {
        self.last_summary_time = Stopwatch::new();
    }

    /// Gets the current interval in seconds
    pub fn interval_secs(&self) -> f64 {
        self.summary_interval_secs
    }

    /// Sets a new interval in seconds
    pub fn set_interval_secs(&mut self, interval_secs: f64) {
        self.summary_interval_secs = interval_secs;
    }

    /// Resets the scheduler timer (useful for initialization)
    pub fn reset(&mut self) {
        self.last_summary_time = Stopwatch::new();
    }
}

impl Default for MetricsScheduler {
    fn default() -> Self {
        Self::with_default_interval()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::thread;
    use std::time::Duration;

    #[test]
    fn scheduler_creation() {
        let scheduler = MetricsScheduler::new(5.0);
        assert_eq!(scheduler.interval_secs(), 5.0);
        assert!(!scheduler.should_log_summary()); // Should not be ready immediately
    }

    #[test]
    fn scheduler_default() {
        let scheduler = MetricsScheduler::default();
        assert_eq!(scheduler.interval_secs(), 10.0);
    }

    #[test]
    fn scheduler_interval_change() {
        let mut scheduler = MetricsScheduler::new(5.0);
        scheduler.set_interval_secs(15.0);
        assert_eq!(scheduler.interval_secs(), 15.0);
    }

    #[test]
    fn scheduler_timing_logic() {
        let mut scheduler = MetricsScheduler::new(0.1); // 100ms for testing

        // Initially should not be ready
        assert!(!scheduler.should_log_summary());

        // Wait a bit and check again
        thread::sleep(Duration::from_millis(150));
        assert!(scheduler.should_log_summary());

        // Mark as logged and should not be ready again
        scheduler.mark_summary_logged();
        assert!(!scheduler.should_log_summary());
    }
}
