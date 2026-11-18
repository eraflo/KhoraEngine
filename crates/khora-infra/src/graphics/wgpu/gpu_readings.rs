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

//! What the GPU's timestamp queries read back, frame by frame.

use khora_core::renderer::api::device::GpuFrameTime;
use std::time::Duration;

/// The GPU times read back: the latest frame's raw time, and smoothed times
/// for display.
///
/// The frame's report to the DCC carries the raw time of one frame; a stall
/// is one frame long there, however long the smoothed values take to settle.
#[derive(Debug, Clone)]
pub(crate) struct GpuReadings {
    /// Nanoseconds per timestamp tick.
    period_ns: f32,
    /// The weight of a new reading in the smoothed values.
    alpha: f32,
    /// The latest frame read back, raw.
    last: Option<GpuFrameTime>,
    /// The smoothed time of the work between the two timestamp passes, in ms.
    smooth_main_ms: Option<f32>,
    /// The smoothed time of the whole frame, in ms.
    smooth_total_ms: Option<f32>,
}

impl GpuReadings {
    /// Creates readings of nothing yet, smoothing new readings by `alpha`.
    pub(crate) fn new(alpha: f32) -> Self {
        Self {
            period_ns: 1.0,
            alpha,
            last: None,
            smooth_main_ms: None,
            smooth_total_ms: None,
        }
    }

    /// Sets the queue's timestamp period, in nanoseconds per tick.
    pub(crate) fn set_period(&mut self, period_ns: f32) {
        self.period_ns = period_ns;
    }

    /// Records the four timestamps frame `frame` wrote: the opening pass's
    /// begin and end, the closing pass's begin and end.
    ///
    /// Timestamps out of order measure nothing and are left out. A frame
    /// older than the latest one read updates the smoothed values only.
    pub(crate) fn record(&mut self, frame: u64, timestamps: [u64; 4]) {
        let [start, opened, closing, end] = timestamps;
        if closing <= opened || end <= start {
            return;
        }
        let nanos = |ticks: u64| ticks as f64 * f64::from(self.period_ns);
        let main_ns = nanos(closing - opened);
        let total_ns = nanos(end - start);
        let alpha = self.alpha;
        let smooth = |raw_ns: f64, smooth: Option<f32>| {
            let raw = (raw_ns / 1_000_000.0) as f32;
            Some(smooth.map_or(raw, |smooth| alpha * raw + (1.0 - alpha) * smooth))
        };
        self.smooth_main_ms = smooth(main_ns, self.smooth_main_ms);
        self.smooth_total_ms = smooth(total_ns, self.smooth_total_ms);
        if self.last.is_none_or(|last| frame > last.frame) {
            self.last = Some(GpuFrameTime {
                frame,
                time: Duration::from_nanos(total_ns.round() as u64),
            });
        }
    }

    /// Returns the latest frame read back, raw; `None` before the first.
    pub(crate) fn last(&self) -> Option<GpuFrameTime> {
        self.last
    }

    /// Returns the smoothed time of the work between the timestamp passes, in ms.
    pub(crate) fn smooth_main_ms(&self) -> Option<f32> {
        self.smooth_main_ms
    }

    /// Returns the smoothed time of the whole frame, in ms.
    pub(crate) fn smooth_total_ms(&self) -> Option<f32> {
        self.smooth_total_ms
    }
}

#[cfg(test)]
mod tests {
    use super::GpuReadings;
    use std::time::Duration;

    /// Timestamps of a frame whose whole span lasts `total_ms`, at 1 ns a tick.
    fn frame_of(total_ms: u64) -> [u64; 4] {
        let total = total_ms * 1_000_000;
        [0, 1_000, total - 1_000, total]
    }

    /// **Nothing read back is unknown.**
    #[test]
    fn nothing_read_back_is_unknown() {
        let readings = GpuReadings::new(0.2);

        assert_eq!(readings.last(), None);
        assert_eq!(readings.smooth_total_ms(), None);
        assert_eq!(readings.smooth_main_ms(), None);
    }

    /// **A stall is one frame long in the raw reading.**
    ///
    /// Frames of 13 ms, one of 120 ms, then 13 ms again: the frame after the
    /// stall reads 13 ms raw, while the smoothed value is still about 30 ms.
    #[test]
    fn a_stall_is_one_frame_long_in_the_raw_reading() {
        let mut readings = GpuReadings::new(0.2);
        readings.record(1, frame_of(13));
        readings.record(2, frame_of(120));
        let stall = readings.last().expect("the stall was read");
        readings.record(3, frame_of(13));
        let after = readings.last().expect("the frame after it was read");

        assert_eq!(stall.frame, 2);
        assert_eq!(stall.time, Duration::from_millis(120));
        assert_eq!(after.frame, 3);
        assert_eq!(after.time, Duration::from_millis(13), "raw, not smoothed");
        let smoothed = readings.smooth_total_ms().expect("smoothed for display");
        assert!(smoothed > 25.0, "the display smooths it: {smoothed}");
    }

    /// **Timestamps out of order measure nothing.**
    #[test]
    fn timestamps_out_of_order_measure_nothing() {
        let mut readings = GpuReadings::new(0.2);
        readings.record(1, frame_of(13));
        readings.record(2, [5_000, 4_000, 3_000, 2_000]);

        assert_eq!(readings.last().map(|read| read.frame), Some(1));
    }

    /// **An older frame read late does not replace the latest.**
    #[test]
    fn an_older_frame_read_late_does_not_replace_the_latest() {
        let mut readings = GpuReadings::new(0.2);
        readings.record(5, frame_of(13));
        readings.record(4, frame_of(20));

        assert_eq!(readings.last().map(|read| read.frame), Some(5));
    }

    /// **The period scales the ticks.** At 2 ns a tick, 6.5 million ticks are 13 ms.
    #[test]
    fn the_period_scales_the_ticks() {
        let mut readings = GpuReadings::new(0.2);
        readings.set_period(2.0);
        readings.record(1, [0, 500, 6_000_000, 6_500_000]);

        assert_eq!(
            readings.last().map(|read| read.time),
            Some(Duration::from_millis(13))
        );
    }
}
