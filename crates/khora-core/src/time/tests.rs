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

use super::*;

#[test]
fn default_uses_60hz_fixed_step() {
    let t = Time::default();
    assert_eq!(t.fixed_delta_seconds, 1.0 / 60.0);
    assert_eq!(t.delta_seconds, 1.0 / 60.0);
    assert_eq!(t.interpolation_alpha, 0.0);
    assert_eq!(t.frame, 0);
}

#[test]
fn with_fixed_delta_overrides_step_only() {
    let t = Time::with_fixed_delta(1.0 / 120.0);
    assert_eq!(t.fixed_delta_seconds, 1.0 / 120.0);
    assert_eq!(t.interpolation_alpha, 0.0);
    assert_eq!(t.frame, 0);
}

mod scale_tests {
    use super::super::*;

    #[test]
    fn a_fresh_clock_runs_at_real_time() {
        assert_eq!(Time::default().scale(), 1.0);
        assert!(Time::default().is_running());
    }

    /// **What stops a body falling in the editor.** Zero is not a special case
    /// in the scheduler — the accumulator simply never fills.
    #[test]
    fn a_scale_of_zero_stops_the_world() {
        let mut time = Time::default();
        time.set_scale(0.0);

        assert!(!time.is_running());
    }

    /// Slow motion and fast forward are the same knob, which is the reason it
    /// is a scale and not a boolean: a game wants these on its own, and the
    /// editor is one caller among them.
    #[test]
    fn a_scale_between_the_two_is_slow_motion() {
        let mut time = Time::default();
        time.set_scale(0.25);

        assert_eq!(time.scale(), 0.25);
        assert!(time.is_running());
    }

    /// Running a solver backwards is not a slower forward. Every integrator
    /// here assumes time moves one way, and a reversed step produces states no
    /// forward run could reach.
    #[test]
    fn time_refuses_to_run_backwards() {
        let mut time = Time::default();
        time.set_scale(-1.0);

        assert_eq!(time.scale(), 0.0);
    }
}
