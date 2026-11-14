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

//! The frame governor: drives GORNA's global budget multiplier from the frames'
//! critical paths. Every time it counts is frame time — the sum of the frames
//! it observed — never the wall clock.

use std::time::Duration;

/// Tuning of a [`FrameGovernor`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GovernorConfig {
    /// The share of the setpoint kept free: the governor aims at
    /// `setpoint × (1 − headroom)`.
    pub headroom: f32,
    /// The width of the band under the target, as a share of the target, in
    /// which the governor holds.
    pub band: f32,
    /// The largest share of the multiplier one decision takes away.
    pub max_drop: f32,
    /// How far one decision moves the multiplier toward a higher target.
    pub rise: f32,
    /// How long, in seconds of frame time, the multiplier a decision lowered
    /// from stays a ceiling on raising it again.
    pub memory_seconds: f32,
    /// Consecutive frames over the setpoint after which the governor acts
    /// without waiting for `settle_frames`.
    pub panic_frames: u32,
    /// A frame longer than `hitch_ratio × setpoint` is a hitch, or load bounded
    /// at that length.
    pub hitch_ratio: f32,

    /// Time constant, in seconds of frame time, of the estimate's low-pass.
    pub filter_seconds: f32,
    /// Frames observed after a change before the governor changes again.
    pub settle_frames: u32,
    /// The lowest multiplier.
    pub output_min: f32,
    /// The highest multiplier, where the governor starts.
    pub output_max: f32,
}

impl Default for GovernorConfig {
    fn default() -> Self {
        Self {
            headroom: 0.10,
            band: 0.15,
            max_drop: 0.30,
            rise: 0.25,
            memory_seconds: 5.0,
            panic_frames: 3,
            hitch_ratio: 2.0,
            filter_seconds: 0.25,
            settle_frames: 3,
            output_min: 0.3,
            output_max: 1.0,
        }
    }
}

/// The smallest change a decision makes: below it, the multiplier holds.
const MIN_STEP: f32 = 0.02;

/// What the step comparisons forgive: a step built to be exactly `MIN_STEP`
/// may come out a hair under it in `f32`.
const STEP_TOLERANCE: f32 = 1e-4;

/// How far under the multiplier it lowered from a raise stops while that
/// multiplier is remembered: the plan just above it is the one that ran over.
const CEILING_MARGIN: f32 = 0.06;

/// Drives the budget multiplier so the frames' critical path settles under the
/// setpoint, with headroom.
///
/// A ratio law, not a PID: the multiplier is scaled by `target / estimate`,
/// which inverts the frame's cost in the budget in one step when the agents'
/// work follows their envelopes. Around it, what makes it hold on a plant that
/// moves by whole rows and a frame or two late: a band it does not act in, a
/// memory of the multiplier that ran over, decisions only on frames measured
/// after the last one took effect, hitches left out, and no lowering when no
/// agent has a row left to give.
#[derive(Debug, Clone)]
pub struct FrameGovernor {
    cfg: GovernorConfig,
    /// The multiplier in force.
    multiplier: f32,
    /// The estimate of the frame, in seconds; `None` until a frame since the
    /// last change was observed.
    estimate: Option<f64>,
    /// Frame time observed so far, in seconds: the governor's clock.
    clock: f64,
    /// Frames observed since the last change was lent.
    since_change: u32,
    /// Frame time, in seconds, the estimate has measured since it started
    /// again: until it spans `filter_seconds`, the estimate is a plain
    /// duration-weighted mean, so no one frame weighs more than its share.
    measured: f64,
    /// Consecutive frames over the setpoint since the last change.
    over: u32,
    /// Whether the last frame was over the setpoint.
    last_over: bool,

    /// The multiplier the last lowering left, and when, on the clock.
    ceiling: Option<(f32, f64)>,
}

impl FrameGovernor {
    /// A governor of `cfg`, its multiplier at `cfg.output_max`.
    pub fn new(cfg: GovernorConfig) -> Self {
        Self {
            cfg,
            multiplier: cfg.output_max,
            estimate: None,
            clock: 0.0,
            since_change: 0,
            measured: 0.0,
            over: 0,
            last_over: false,
            ceiling: None,
        }
    }

    /// One frame report: the frame's critical path, against `setpoint`.
    ///
    /// The estimate is a low-pass weighted by the frame's own length, so it
    /// spans the same time at any frame rate. A frame longer than
    /// `hitch_ratio × setpoint` right after one within the setpoint is a
    /// hitch — a load, a stall, nothing a row could fix — and is left out;
    /// one that follows a long frame is load, and enters bounded.
    ///
    /// Returns the sample the estimate took: `None` for a hitch or a frame
    /// that took no time. The planner reads its margin from these samples, so
    /// both read the same frames.
    pub fn observe(&mut self, frame: Duration, setpoint: Duration) -> Option<Duration> {
        let seconds = frame.as_secs_f64();
        // A frame that took no time measured nothing.
        if seconds <= 0.0 {
            return None;
        }
        self.clock += seconds;
        self.since_change = self.since_change.saturating_add(1);
        let setpoint = setpoint.as_secs_f64();
        let over = seconds > setpoint;
        self.over = if over { self.over.saturating_add(1) } else { 0 };

        let bound = setpoint * f64::from(self.cfg.hitch_ratio);
        // The engine's first frame too: it is usually its longest — the
        // pipelines, the uploads — and no load.
        let hitch = seconds > bound && !self.last_over;
        self.last_over = over;
        if hitch {
            return None;
        }
        let sample = seconds.min(bound.max(f64::MIN_POSITIVE));
        let filter = f64::from(self.cfg.filter_seconds).max(f64::MIN_POSITIVE);
        // A mean weighted by each frame's length until the estimate spans
        // the filter's time, then the low-pass over that time.
        let alpha = if self.measured < filter {
            seconds / (self.measured + seconds)
        } else {
            1.0 - (-seconds / filter).exp()
        };
        self.measured += seconds;
        self.estimate = Some(match self.estimate {
            Some(estimate) => estimate + alpha * (sample - estimate),
            None => sample,
        });
        Some(Duration::from_secs_f64(sample))
    }

    /// One DCC tick: the new multiplier when the governor changes it.
    /// `no_lever` says lowering the multiplier would change nothing (every
    /// agent is on its floor).
    ///
    /// It acts once `settle_frames` frames, and `filter_seconds` of frame
    /// time, were measured since its last change was lent ([`lent`]) — the
    /// change has reached the frames, and the estimate spans its time — or at
    /// once when `panic_frames` frames in a row ran over the setpoint.
    ///
    /// [`lent`]: Self::lent
    pub fn decide(&mut self, setpoint: Duration, no_lever: bool) -> Option<f32> {
        let cfg = self.cfg;
        let estimate = self.estimate?;
        let settled = self.since_change >= cfg.settle_frames
            && self.measured >= f64::from(cfg.filter_seconds);
        let panic = self.over >= cfg.panic_frames;
        if !(settled || panic) {
            return None;
        }
        let target = setpoint.as_secs_f64() * f64::from(1.0 - cfg.headroom);
        if target <= 0.0 || estimate <= 0.0 {
            return None;
        }
        let ratio = (target / estimate) as f32;
        let m = self.multiplier;

        let next = if estimate > target {
            if no_lever {
                return None;
            }
            (m * ratio.max(1.0 - cfg.max_drop)).max(cfg.output_min)
        } else if estimate < target * f64::from(1.0 - cfg.band) {
            let mut goal = (m * ratio).min(cfg.output_max);
            if let Some((ceiling, at)) = self.ceiling {
                if self.clock - at < f64::from(cfg.memory_seconds) {
                    goal = goal.min(ceiling - CEILING_MARGIN);
                } else {
                    self.ceiling = None;
                }
            }
            let gap = goal - m;
            if gap < MIN_STEP - STEP_TOLERANCE {
                return None;
            }
            // A quarter of the way, but never a step too small to make, and
            // the whole way when what would be left is too small to make:
            // the multiplier reaches its goal rather than creeping at it.
            let step = (cfg.rise * gap).max(MIN_STEP);
            m + if gap - step < MIN_STEP { gap } else { step }
        } else {
            return None;
        };

        if (next - m).abs() < MIN_STEP - STEP_TOLERANCE {
            return None;
        }
        if next < m {
            self.ceiling = Some((m, self.clock));
        }
        self.multiplier = next;
        self.restart();
        Some(next)
    }

    /// The last change was lent to the agents: what was measured before says
    /// nothing of the frames under it, so the estimate starts again from them.
    /// The DCC calls it when an epoch carries the multiplier out; until then a
    /// governor driving a real allocation would be deciding on frames that
    /// never saw its last decision.
    pub fn lent(&mut self) {
        self.restart();
    }

    /// Forgets what was measured: the estimate, and the counts toward
    /// settling and panic.
    fn restart(&mut self) {
        self.estimate = None;
        self.measured = 0.0;
        self.since_change = 0;
        self.over = 0;
    }

    /// The multiplier in force.
    pub fn multiplier(&self) -> f32 {
        self.multiplier
    }
}

#[cfg(test)]
mod tests;
