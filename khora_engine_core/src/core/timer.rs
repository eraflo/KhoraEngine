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

use std::time::{Duration, Instant};

#[derive(Debug, Clone)]
pub(crate) struct Stopwatch {
    start_time: Option<Instant>,
}

impl Stopwatch {
    /// Creates a new Stopwatch instance.
    /// ## Returns
    /// A new instance of the Stopwatch struct.
    #[inline]
    pub fn new() -> Self {
        Self {
            start_time: Some(Instant::now()),
        }
    }

    /// Returns the elapsed time since the stopwatch was started.
    /// ## Arguments
    /// * `&self` - A reference to the Stopwatch instance.
    /// ## Returns
    /// An Option containing the elapsed time as a Duration, or None if the stopwatch has not been started.
    #[inline]
    pub fn elapsed(&self) -> Option<Duration> {
        self.start_time.map(|start| start.elapsed())
    }

    /// Returns the elapsed time since the stopwatch was started in milliseconds.
    /// ## Arguments
    /// * `&self` - A reference to the Stopwatch instance.
    /// ## Returns
    /// An Option containing the elapsed time in milliseconds as a u64, or None if the stopwatch has not been started.
    #[inline]
    pub fn elapsed_ms(&self) -> Option<u64> {
        self.elapsed().map(|d| d.as_millis() as u64)
    }

    /// Returns the elapsed time since the stopwatch was started in microseconds.
    /// ## Arguments
    /// * `&self` - A reference to the Stopwatch instance.
    /// ## Returns
    /// An Option containing the elapsed time in microseconds as a u64, or None if the stopwatch has not been started.
    #[inline]
    pub fn elapsed_us(&self) -> Option<u64> {
        self.elapsed().map(|d| d.as_micros() as u64)
    }

    /// Returns the elapsed time since the stopwatch was started in seconds as f64.
    /// ## Arguments
    /// * `&self` - A reference to the Stopwatch instance.
    /// ## Returns
    /// An Option containing the elapsed time in seconds as f64, or None if the stopwatch has not been started.
    #[inline]
    pub fn elapsed_secs_f64(&self) -> Option<f64> {
        self.elapsed().map(|d| d.as_secs_f64())
    }
}

impl Default for Stopwatch {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::thread;

    const SMALL_DURATION_MS: u64 = 15;
    const SLEEP_DURATION_MS: u64 = 100;
    const SLEEP_MARGIN_MS: u64 = 200;

    /// A test to check if the Stopwatch struct is created correctly and starts the timer.
    /// It verifies that the elapsed time is not None after creation and that it is very small.
    #[test]
    fn stopwatch_creation_starts_timer() {
        let watch = Stopwatch::new();
        // Since ::new() guarantees start_time is Some, elapsed() should also be Some.
        assert!(
            watch.elapsed().is_some(),
            "Elapsed should return Some after creation"
        );
        assert!(
            watch.elapsed_ms().is_some(),
            "Elapsed_ms should return Some after creation"
        );
        assert!(
            watch.elapsed_us().is_some(),
            "Elapsed_us should return Some after creation"
        );
        assert!(
            watch.elapsed_secs_f64().is_some(),
            "Elapsed_secs_f64 should return Some after creation"
        );
    }

    /// A test to check if the Stopwatch struct correctly reports elapsed time after a short delay.
    /// It verifies that the elapsed time is greater than or equal to the sleep duration and less than the sleep duration plus a margin.
    #[test]
    fn stopwatch_elapsed_time_near_zero_initially() {
        let watch = Stopwatch::new();

        // Check elapsed Duration
        let elapsed_duration = watch.elapsed().expect("Should have elapsed duration");
        assert!(
            elapsed_duration < Duration::from_millis(SMALL_DURATION_MS),
            "Initial elapsed duration ({elapsed_duration:?}) should be very small"
        );

        // Check elapsed milliseconds
        let elapsed_ms = watch.elapsed_ms().expect("Should have elapsed ms");
        assert!(
            elapsed_ms < SMALL_DURATION_MS,
            "Initial elapsed ms ({elapsed_ms}) should be very small"
        );

        // Check elapsed microseconds
        let elapsed_us = watch.elapsed_us().expect("Should have elapsed us");
        let small_duration_us = SMALL_DURATION_MS * 1000;
        assert!(
            elapsed_us < small_duration_us,
            "Initial elapsed us ({elapsed_us}) should be very small"
        );

        let elapsed_secs_f64 = watch
            .elapsed_secs_f64()
            .expect("Should have elapsed seconds as f64");
        assert!(
            elapsed_secs_f64 < SMALL_DURATION_MS as f64 / 1000.0,
            "Initial elapsed seconds ({elapsed_secs_f64}) should be very small"
        );
    }

    /// A test to check if the Stopwatch struct correctly reports elapsed time after a sleep duration.
    /// It verifies that the elapsed time is greater than or equal to the sleep duration and less than the sleep duration plus a margin.
    #[test]
    fn stopwatch_elapsed_time_after_delay() {
        let watch = Stopwatch::new();
        let sleep_duration = Duration::from_millis(SLEEP_DURATION_MS);
        let margin_duration = Duration::from_millis(SLEEP_MARGIN_MS);
        let min_expected_duration = sleep_duration;
        let max_expected_duration = sleep_duration + margin_duration;

        thread::sleep(sleep_duration);

        // Check elapsed Duration
        let elapsed_duration = watch
            .elapsed()
            .expect("Should have elapsed duration after sleep");
        assert!(
            elapsed_duration >= min_expected_duration,
            "Elapsed duration ({elapsed_duration:?}) should be >= sleep duration ({min_expected_duration:?})"
        );
        assert!(
            elapsed_duration < max_expected_duration,
            "Elapsed duration ({elapsed_duration:?}) should be < sleep duration + margin ({max_expected_duration:?})"
        );

        // Check elapsed milliseconds
        let elapsed_ms = watch
            .elapsed_ms()
            .expect("Should have elapsed ms after sleep");
        let min_expected_ms = SLEEP_DURATION_MS;
        let max_expected_ms = SLEEP_DURATION_MS + SLEEP_MARGIN_MS;
        assert!(
            elapsed_ms >= min_expected_ms,
            "Elapsed ms ({elapsed_ms}) should be >= sleep duration ms ({min_expected_ms})"
        );
        assert!(
            elapsed_ms < max_expected_ms,
            "Elapsed ms ({elapsed_ms}) should be < sleep duration ms + margin ({max_expected_ms})"
        );

        // Check elapsed microseconds
        let elapsed_us = watch
            .elapsed_us()
            .expect("Should have elapsed us after sleep");
        let min_expected_us = SLEEP_DURATION_MS * 1000;
        let max_expected_us = (SLEEP_DURATION_MS + SLEEP_MARGIN_MS) * 1000;
        assert!(
            elapsed_us >= min_expected_us,
            "Elapsed us ({elapsed_us}) should be >= sleep duration us ({min_expected_us})"
        );
        assert!(
            elapsed_us < max_expected_us,
            "Elapsed us ({elapsed_us}) should be < sleep duration us + margin ({max_expected_us})"
        );

        // Check elapsed seconds as f64
        let elapsed_secs_f64 = watch
            .elapsed_secs_f64()
            .expect("Should have elapsed seconds as f64 after sleep");
        let min_expected_secs_f64 = SLEEP_DURATION_MS as f64 / 1000.0;
        let max_expected_secs_f64 = (SLEEP_DURATION_MS + SLEEP_MARGIN_MS) as f64 / 1000.0;
        assert!(
            elapsed_secs_f64 >= min_expected_secs_f64,
            "Elapsed seconds ({elapsed_secs_f64}) should be >= sleep duration seconds ({min_expected_secs_f64})"
        );
        assert!(
            elapsed_secs_f64 < max_expected_secs_f64,
            "Elapsed seconds ({elapsed_secs_f64}) should be < sleep duration seconds + margin ({max_expected_secs_f64})"
        );
    }

    /// A test to check if the Stopwatch struct implements the Default trait.
    /// It verifies that the default stopwatch has a valid elapsed time.
    #[test]
    fn stopwatch_implements_default() {
        let watch = Stopwatch::default();
        assert!(watch.elapsed().is_some());
    }

    /// A test to check if the Stopwatch struct implements the Clone trait.
    /// It verifies that the elapsed time of the original and cloned stopwatch are roughly equal.
    #[test]
    fn stopwatch_clone() {
        let watch1 = Stopwatch::new();
        thread::sleep(Duration::from_millis(10));
        let watch2 = watch1.clone(); // Clone the stopwatch

        // Both clones should report roughly the same elapsed time,
        // relative to the *original* start time.
        let elapsed1 = watch1.elapsed_us().unwrap();
        let elapsed2 = watch2.elapsed_us().unwrap();

        // They should be very close, allow a small difference for the clone operation itself
        let difference = if elapsed1 > elapsed2 {
            elapsed1.abs_diff(elapsed2)
        } else {
            elapsed2.abs_diff(elapsed1)
        };
        assert!(
            difference < 1000,
            "Elapsed time of clones should be very close (diff: {difference} us)"
        ); // Allow 1ms diff
    }
}
