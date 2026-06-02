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

//! A discrete-time PID controller for closed-loop budget regulation.
//!
//! The DCC uses this to drive GORNA's global budget multiplier so that the
//! *measured* frame time tracks a *setpoint* (the heuristic-suggested latency),
//! replacing the old static thermal/battery lookup. The controller is a pure
//! scalar algorithm — no engine state, no allocation — so it lives in
//! `khora-core` and is unit-tested in isolation.
//!
//! It implements the practical refinements that matter for a noisy,
//! saturating, discrete-actuator plant:
//!
//! - **Derivative on measurement** (not on error) — no derivative kick when the
//!   setpoint jumps (it jumps every time the thermal state or phase changes).
//! - **Filtered derivative** — a first-order low-pass on the derivative term
//!   tames frame-time noise; the filter time constant is `(Kd/Kp)/N`.
//! - **Back-calculation anti-windup** — the output saturates to
//!   `[output_min, output_max]`; the integrator is unwound by
//!   `Kb·(saturated − unsaturated)` so it never accumulates against a limit
//!   (the steady ceiling at `output_max` when running with headroom is the most
//!   common windup case here).
//! - **Setpoint weighting** — the setpoint is weighted by `b` in the
//!   proportional term to curb overshoot without hurting disturbance rejection.
//! - **Output clamping** — to a configurable `[output_min, output_max]`.

/// Tuning and limits for a [`PidController`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PidConfig {
    /// Proportional gain.
    pub kp: f32,
    /// Integral gain (per second).
    pub ki: f32,
    /// Derivative gain (seconds).
    pub kd: f32,
    /// Setpoint weight `b` on the proportional term (`b·setpoint − measurement`).
    /// `1.0` is classic PID; lower values (≈0.7) reduce overshoot on setpoint
    /// changes while keeping full disturbance rejection.
    pub setpoint_weight_b: f32,
    /// Derivative filter divisor `N` (the filter time constant is `(Kd/Kp)/N`).
    /// Typical range `2..=20`; higher `N` filters less. Must be `> 0`.
    pub derivative_filter_n: f32,
    /// Back-calculation anti-windup gain. `0.0` disables anti-windup. A common
    /// starting point is `Kb ≈ Ki` (anti-windup time constant `1/Kb`).
    pub kb: f32,
    /// Lower saturation bound on the output.
    pub output_min: f32,
    /// Upper saturation bound on the output.
    pub output_max: f32,
}

impl Default for PidConfig {
    /// Defaults tuned for the DCC frame-time loop: output is the budget
    /// multiplier in `[0.3, 1.0]`, the plant is the discrete strategy ladder
    /// sampled at the cold-path rate (~20 Hz). Conservative gains favour
    /// stability over speed on this coarse, non-linear actuator.
    fn default() -> Self {
        Self {
            // Gains are expressed against an error in *milliseconds* of frame
            // time mapped onto a unitless multiplier, so they are small.
            kp: 0.02,
            ki: 0.04,
            kd: 0.002,
            setpoint_weight_b: 0.7,
            derivative_filter_n: 5.0,
            // Back-calculation gain ≫ ki so the integrator stays bounded under
            // sustained saturation and recovers promptly once the error clears.
            kb: 0.4,
            output_min: 0.3,
            output_max: 1.0,
        }
    }
}

/// A discrete-time PID controller with filtered derivative-on-measurement and
/// back-calculation anti-windup. See the [module docs](self) for the rationale.
#[derive(Debug, Clone)]
pub struct PidController {
    cfg: PidConfig,
    /// Integral accumulator. Seeded to `output_max` so the controller rests at
    /// full performance until an error pushes it down.
    integral: f32,
    /// Previous measurement, for the derivative-on-measurement difference.
    prev_measurement: f32,
    /// Low-pass-filtered derivative term.
    derivative_state: f32,
    /// Last saturated output, exposed for the glass-box surface.
    last_output: f32,
    /// `false` until the first [`update`](Self::update) seeds `prev_measurement`
    /// (avoids a spurious derivative spike on the first sample).
    initialized: bool,
}

impl PidController {
    /// Creates a controller resting at `output_max` (full performance).
    pub fn new(cfg: PidConfig) -> Self {
        Self {
            integral: cfg.output_max,
            prev_measurement: 0.0,
            derivative_state: 0.0,
            last_output: cfg.output_max,
            initialized: false,
            cfg,
        }
    }

    /// Advances the controller one discrete step and returns the saturated
    /// output. `dt` is the elapsed time in seconds since the previous call; a
    /// non-positive `dt` is a no-op that returns the last output.
    pub fn update(&mut self, setpoint: f32, measurement: f32, dt: f32) -> f32 {
        if dt <= 0.0 {
            return self.last_output;
        }

        // First call: seed the measurement history so the derivative starts at
        // zero instead of jumping from the default 0.0.
        if !self.initialized {
            self.prev_measurement = measurement;
            self.initialized = true;
        }

        let error = setpoint - measurement;

        // Proportional term with setpoint weighting.
        let p = self.cfg.kp * (self.cfg.setpoint_weight_b * setpoint - measurement);

        // Derivative on measurement (negated), low-pass filtered. Deriving on
        // the measurement avoids a kick when the setpoint steps.
        let d_measurement = (measurement - self.prev_measurement) / dt;
        let d_raw = -self.cfg.kd * d_measurement;
        // First-order filter: alpha = dt / (Tf + dt), Tf = (Kd/Kp)/N.
        let tf = if self.cfg.kp.abs() > f32::EPSILON {
            (self.cfg.kd / self.cfg.kp) / self.cfg.derivative_filter_n
        } else {
            0.0
        };
        let alpha = if tf + dt > 0.0 { dt / (tf + dt) } else { 1.0 };
        self.derivative_state += alpha * (d_raw - self.derivative_state);

        // Unsaturated command, then clamp.
        let unsat = p + self.integral + self.derivative_state;
        let out = unsat.clamp(self.cfg.output_min, self.cfg.output_max);

        // Integrate with back-calculation anti-windup: the `kb·(out − unsat)`
        // term bleeds the integrator back inside the saturation limits.
        self.integral += (self.cfg.ki * error + self.cfg.kb * (out - unsat)) * dt;

        self.prev_measurement = measurement;
        self.last_output = out;
        out
    }

    /// Resets the controller to its initial resting state (`output_max`).
    pub fn reset(&mut self) {
        self.integral = self.cfg.output_max;
        self.prev_measurement = 0.0;
        self.derivative_state = 0.0;
        self.last_output = self.cfg.output_max;
        self.initialized = false;
    }

    /// The last saturated output (the current control value). Glass-box only.
    pub fn output(&self) -> f32 {
        self.last_output
    }

    /// The active configuration.
    pub fn config(&self) -> &PidConfig {
        &self.cfg
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Drives the controller toward a setpoint with a simple first-order plant:
    /// `measurement` moves toward `k / output` — i.e. a lower multiplier yields
    /// a lower (faster) frame time, mirroring the real budget→strategy→time
    /// relationship. Returns the settled measurement and output.
    fn simulate(cfg: PidConfig, setpoint: f32, plant_gain: f32, steps: usize) -> (f32, f32) {
        let mut pid = PidController::new(cfg);
        let dt = 1.0 / 20.0; // 20 Hz cold path
        let mut measurement = setpoint; // start on target
        let mut output = cfg.output_max;
        for _ in 0..steps {
            output = pid.update(setpoint, measurement, dt);
            // Plant: frame time scales with the granted budget multiplier.
            let target_measurement = plant_gain * output;
            // First-order lag toward the plant's response (gentle, stable).
            measurement += 0.3 * (target_measurement - measurement);
        }
        (measurement, output)
    }

    #[test]
    fn rests_at_output_max_on_construction() {
        let pid = PidController::new(PidConfig::default());
        assert_eq!(pid.output(), 1.0);
    }

    #[test]
    fn non_positive_dt_is_noop() {
        let mut pid = PidController::new(PidConfig::default());
        let before = pid.output();
        assert_eq!(pid.update(16.0, 20.0, 0.0), before);
        assert_eq!(pid.update(16.0, 20.0, -1.0), before);
    }

    #[test]
    fn output_stays_within_clamp() {
        let cfg = PidConfig::default();
        let mut pid = PidController::new(cfg);
        let dt = 0.05;
        // Hammer with a huge persistent overrun; output must never leave bounds.
        for _ in 0..500 {
            let out = pid.update(16.0, 200.0, dt);
            assert!(out >= cfg.output_min - 1e-6 && out <= cfg.output_max + 1e-6);
        }
    }

    #[test]
    fn persistent_overrun_drives_output_down() {
        let mut pid = PidController::new(PidConfig::default());
        let dt = 0.05;
        let first = pid.update(16.0, 40.0, dt);
        let mut last = first;
        for _ in 0..50 {
            last = pid.update(16.0, 40.0, dt);
        }
        // Measurement well above setpoint → multiplier must shrink.
        assert!(
            last < first,
            "expected output to decrease, {first} -> {last}"
        );
        assert!(last < 1.0);
    }

    #[test]
    fn converges_toward_setpoint() {
        // Plant gain 32 means measurement = 32*output; setpoint 16 → output≈0.5.
        let (measurement, output) = simulate(PidConfig::default(), 16.0, 32.0, 5000);
        assert!(
            (measurement - 16.0).abs() < 1.5,
            "measurement {measurement} should settle near setpoint 16"
        );
        assert!(
            (0.3..=1.0).contains(&output),
            "output {output} should be within clamp"
        );
    }

    #[test]
    fn anti_windup_keeps_integrator_bounded() {
        // Under a long, heavy (but realistic) overrun the output pins at the
        // floor; back-calculation must keep the integrator bounded so the output
        // recovers once the frame becomes cheap again — not stay stuck at floor.
        let cfg = PidConfig::default();
        let mut pid = PidController::new(cfg);
        let dt = 0.05;
        for _ in 0..200 {
            pid.update(16.0, 50.0, dt); // sustained 50ms frames → pinned at floor
        }
        assert!((pid.output() - cfg.output_min).abs() < 1e-3);
        // Frames are now well under budget: the output must climb back off floor.
        let mut out = pid.output();
        for _ in 0..400 {
            out = pid.update(16.0, 6.0, dt);
        }
        assert!(
            out > cfg.output_min + 0.15,
            "anti-windup should let output recover off the floor, got {out}"
        );
    }

    #[test]
    fn filtered_derivative_tames_noise() {
        // Alternating noisy measurements around the setpoint should not blow the
        // output around violently; with a filtered derivative it stays bounded.
        let mut pid = PidController::new(PidConfig::default());
        let dt = 0.05;
        let mut min_out = f32::MAX;
        let mut max_out = f32::MIN;
        for i in 0..200 {
            let noisy = if i % 2 == 0 { 14.0 } else { 18.0 };
            let out = pid.update(16.0, noisy, dt);
            min_out = min_out.min(out);
            max_out = max_out.max(out);
        }
        assert!(
            max_out - min_out < 0.3,
            "filtered derivative should keep output swing small, got {}",
            max_out - min_out
        );
    }

    #[test]
    fn reset_restores_resting_state() {
        let mut pid = PidController::new(PidConfig::default());
        let dt = 0.05;
        for _ in 0..50 {
            pid.update(16.0, 60.0, dt);
        }
        assert!(pid.output() < 1.0);
        pid.reset();
        assert_eq!(pid.output(), 1.0);
    }
}
