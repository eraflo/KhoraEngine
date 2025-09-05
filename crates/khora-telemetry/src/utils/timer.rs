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

//! Provides RAII-based timers for automatically recording metrics. (RAII = Resource Acquisition Is Initialization)

use crate::metrics::registry::HistogramHandle;
use khora_core::utils::timer::Stopwatch;

/// A utility for timing the duration of a scope and automatically recording
/// the result in a `Histogram` when it is dropped.
///
/// This leverages the RAII pattern to ensure that the measurement is always
// recorded, even in the case of early returns or panics.
pub struct ScopedMetricTimer<'a> {
    stopwatch: Stopwatch,
    histogram: &'a HistogramHandle,
}

impl<'a> ScopedMetricTimer<'a> {
    /// Creates a new timer for the given histogram and starts it immediately.
    pub fn new(histogram: &'a HistogramHandle) -> Self {
        Self {
            stopwatch: Stopwatch::new(),
            histogram,
        }
    }
}

/// When the timer goes out of scope, it records the elapsed time in milliseconds.
impl<'a> Drop for ScopedMetricTimer<'a> {
    fn drop(&mut self) {
        if let Some(elapsed_secs) = self.stopwatch.elapsed_secs_f64() {
            let elapsed_ms = elapsed_secs * 1000.0;
            if let Err(e) = self.histogram.observe(elapsed_ms) {
                log::warn!("[ScopedMetricTimer] Failed to record metric: {:?}", e);
            }
        }
    }
}
