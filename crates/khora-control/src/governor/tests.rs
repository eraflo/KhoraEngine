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

//! The frame governor alone: what it keeps out of its estimate, when it
//! decides, how far a decision moves the multiplier, and the ceiling it
//! remembers. The setpoint is a 60 Hz frame (16.67 ms), so the target, 10 %
//! under it, is 15 ms and the band under the target reaches down to 12.75 ms.
//! Every clock here is frame time: the sum of the frames observed.

use super::*;

/// The setpoint of every test: a 60 Hz frame.
fn setpoint() -> Duration {
    Duration::from_secs_f64(1.0 / 60.0)
}

/// `ms` milliseconds.
fn ms(ms: f64) -> Duration {
    Duration::from_secs_f64(ms / 1_000.0)
}

/// A governor of the default tuning.
fn governor() -> FrameGovernor {
    FrameGovernor::new(GovernorConfig::default())
}

/// Observes `count` frames of `frame`, deciding after none of them.
fn observe(governor: &mut FrameGovernor, count: usize, frame: Duration) {
    for _ in 0..count {
        governor.observe(frame, setpoint());
    }
}

/// Observes `count` frames of `frame`, deciding after each; the decisions
/// that changed the multiplier, in order.
fn observe_deciding(governor: &mut FrameGovernor, count: usize, frame: Duration) -> Vec<f32> {
    let mut changes = Vec::new();
    for _ in 0..count {
        governor.observe(frame, setpoint());
        changes.extend(governor.decide(setpoint(), false));
    }
    changes
}

/// A governor that lowered its multiplier once, to 0.75: three frames of
/// 20 ms, the estimate seeded at 20 ms, the target 15 ms.
fn lowered_to_three_quarters() -> FrameGovernor {
    let mut governor = governor();
    observe(&mut governor, 3, ms(20.0));
    let lowered = governor.decide(setpoint(), false);
    assert!(
        lowered.is_some_and(|m| (m - 0.75).abs() < 1e-3),
        "three frames of 20 ms lower the multiplier to 15 / 20 = 0.75, got {lowered:?}"
    );
    governor
}

/// **The multiplier starts at the top of its range.** A new governor's
/// multiplier is its `output_max`: 1.0 by default, 0.8 when tuned so.
#[test]
fn the_multiplier_starts_at_output_max() {
    assert_eq!(governor().multiplier(), 1.0);
    let tuned = FrameGovernor::new(GovernorConfig {
        output_max: 0.8,
        ..GovernorConfig::default()
    });
    assert_eq!(tuned.multiplier(), 0.8);
}

/// **An isolated hitch leaves the multiplier at rest.**
///
/// Among frames of 10 ms, one frame of 100 ms, of a second, of ten seconds — a
/// load, a stall, a debugger break: it follows a frame within the setpoint, so
/// it is a hitch and stays out of the estimate. The governor, deciding after
/// every frame, never changes the multiplier from 1.0.
#[test]
fn an_isolated_hitch_among_fast_frames_leaves_the_multiplier_at_rest() {
    for hitch in [ms(100.0), ms(1_000.0), ms(10_000.0)] {
        let mut governor = governor();
        let mut changes = observe_deciding(&mut governor, 60, ms(10.0));
        changes.extend(observe_deciding(&mut governor, 1, hitch));
        changes.extend(observe_deciding(&mut governor, 60, ms(10.0)));

        assert!(
            changes.is_empty(),
            "a hitch of {hitch:?} among 10 ms frames changes nothing, changed to {changes:?}"
        );
        assert_eq!(governor.multiplier(), 1.0, "after a hitch of {hitch:?}");
    }
}

/// **A hitch leaves a lowered multiplier where it was.**
///
/// The multiplier was lowered to 0.75, and frames of 14 ms — inside the band —
/// hold it there. One frame of 100 ms, a second or ten seconds after them is a
/// hitch: the multiplier stays 0.75.
#[test]
fn a_hitch_after_frames_within_the_setpoint_leaves_a_lowered_multiplier_where_it_was() {
    for hitch in [ms(100.0), ms(1_000.0), ms(10_000.0)] {
        let mut governor = lowered_to_three_quarters();
        observe(&mut governor, 300, ms(14.0));
        let mut changes = observe_deciding(&mut governor, 30, ms(14.0));
        changes.extend(observe_deciding(&mut governor, 1, hitch));
        changes.extend(observe_deciding(&mut governor, 30, ms(14.0)));

        assert!(
            changes.is_empty(),
            "a hitch of {hitch:?} among 14 ms frames changes nothing, changed to {changes:?}"
        );
        assert!(
            (governor.multiplier() - 0.75).abs() < 1e-6,
            "after a hitch of {hitch:?}: {}",
            governor.multiplier()
        );
    }
}

/// **Sustained long frames are load.**
///
/// After frames of 10 ms, frames of 40 ms that go on: the first follows a frame
/// within the setpoint and is a hitch, the next ones follow a frame over it and
/// are load. They lower the multiplier, never under `output_min`.
#[test]
fn sustained_long_frames_are_load_and_lower_the_multiplier() {
    let cfg = GovernorConfig::default();
    let mut governor = governor();
    observe_deciding(&mut governor, 30, ms(10.0));
    let changes = observe_deciding(&mut governor, 30, ms(40.0));

    assert!(
        !changes.is_empty(),
        "sustained 40 ms frames lower the multiplier"
    );
    let multiplier = governor.multiplier();
    assert!(multiplier < 1.0, "lowered: {multiplier}");
    assert!(
        multiplier >= cfg.output_min,
        "never under the floor {}: {multiplier}",
        cfg.output_min
    );
}

/// **A long frame after a long frame enters the estimate bounded.**
///
/// Two frames of 20 ms, then one of ten seconds: it follows a frame over the
/// setpoint, so it is load, and enters the estimate as a frame of twice the
/// setpoint (33.3 ms), not as ten seconds. With drops allowed up to 90 %, the
/// decision lowers the multiplier to about 15 / 33.3 = 0.45: below the 0.75 the
/// two 20 ms frames alone would give (the long frame entered), far above the
/// floor ten seconds would give (it entered bounded).
#[test]
fn a_long_frame_after_an_over_setpoint_frame_enters_the_estimate_bounded() {
    let mut governor = FrameGovernor::new(GovernorConfig {
        max_drop: 0.9,
        ..GovernorConfig::default()
    });
    observe(&mut governor, 2, ms(20.0));
    governor.observe(ms(10_000.0), setpoint());
    let lowered = governor.decide(setpoint(), false);

    let m = lowered.expect("three frames over the setpoint make a decision");
    assert!(
        m > 0.4,
        "the ten-second frame entered bounded at twice the setpoint, got {m}"
    );
    assert!(
        m < 0.73,
        "the ten-second frame entered the estimate (load, not a hitch), got {m}"
    );
}

/// **Each frame weighs its own duration in the estimate.**
///
/// Seeded by a frame of 30 ms, then 25 frames of 10 ms — a quarter second of
/// frame time, one filter time constant: the estimate is `10 + 20·e⁻¹` ≈ 17.36
/// ms. With drops allowed up to 90 %, the decision lowers the multiplier to 15
/// / 17.36 ≈ 0.864.
#[test]
fn the_estimate_weighs_each_frame_by_its_own_duration() {
    let mut governor = FrameGovernor::new(GovernorConfig {
        max_drop: 0.9,
        ..GovernorConfig::default()
    });
    // Nine frames of 30 ms span the filter's time: the estimate is their
    // mean, 30 ms, before the low-pass takes over.
    observe(&mut governor, 9, ms(30.0));
    observe(&mut governor, 25, ms(10.0));
    let lowered = governor.decide(setpoint(), false);

    let expected = 15.0 / (10.0 + 20.0 * (-1.0_f32).exp());
    let m = lowered.expect("an estimate over the target lowers the multiplier");
    assert!(
        (m - expected).abs() < 5e-3,
        "the multiplier is the target over the estimate, {expected}, got {m}"
    );
}

/// **Lowering scales the multiplier by the target over the estimate, down to
/// the floor.**
///
/// Frames of 30 ms: each decision would scale it by 15 / 30 = 0.5, but takes at
/// most 30 %: 0.7, 0.49, 0.343, then 0.3, the floor (0.24 is under it). At the
/// floor, nothing more changes.
#[test]
fn lowering_scales_the_multiplier_by_the_target_over_the_estimate_down_to_the_floor() {
    let mut governor = governor();
    let mut decisions = Vec::new();
    for _ in 0..5 {
        observe(&mut governor, 3, ms(30.0));
        decisions.push(governor.decide(setpoint(), false));
    }

    let expected = [Some(0.7), Some(0.49), Some(0.343), Some(0.3), None];
    for (decision, expected) in decisions.iter().zip(expected) {
        match (decision, expected) {
            (Some(m), Some(e)) => assert!(
                (m - e).abs() < 1e-3,
                "decisions {decisions:?}, expected {expected:?}"
            ),
            (None, None) => {}
            _ => panic!("decisions {decisions:?}, expected [0.7, 0.49, 0.343, 0.3, None]"),
        }
    }
    assert!((governor.multiplier() - 0.3).abs() < 1e-6);
}

/// **A decision never takes more than `max_drop` of the multiplier.**
///
/// Frames of ten seconds, each load after the first: every decision keeps at
/// least 70 % of the multiplier it lowers, and several decisions are made.
#[test]
fn a_decision_never_drops_the_multiplier_by_more_than_max_drop() {
    let cfg = GovernorConfig::default();
    let mut governor = governor();
    governor.observe(ms(30.0), setpoint());
    let mut previous = governor.multiplier();
    let mut made = 0;
    for _ in 0..40 {
        governor.observe(ms(10_000.0), setpoint());
        if let Some(m) = governor.decide(setpoint(), false) {
            assert!(
                m >= previous * (1.0 - cfg.max_drop) - 1e-5,
                "a decision took {previous} to {m}, more than {} of it",
                cfg.max_drop
            );
            assert!(m >= cfg.output_min, "never under the floor: {m}");
            previous = m;
            made += 1;
        }
    }
    assert!(
        made >= 2,
        "the long frames lowered the multiplier {made} times"
    );
}

/// **The governor waits `settle_frames` frames after a change.**
///
/// Tuned to settle over 10 frames and panic after 3, frames of 16 ms — over the
/// 15 ms target, within the setpoint, so never a panic — lower the multiplier
/// on the tenth frame only, and again ten frames after.
#[test]
fn no_decision_before_settle_frames_frames() {
    let mut governor = FrameGovernor::new(GovernorConfig {
        settle_frames: 10,
        panic_frames: 3,
        // Short enough that the frames, not the estimate's time, are what
        // the governor waits for here.
        filter_seconds: 0.1,
        ..GovernorConfig::default()
    });
    for round in 0..2 {
        for frame in 1..10 {
            governor.observe(ms(16.0), setpoint());
            assert_eq!(
                governor.decide(setpoint(), false),
                None,
                "round {round}: frame {frame} of 16 ms is too soon"
            );
        }
        governor.observe(ms(16.0), setpoint());
        assert!(
            governor.decide(setpoint(), false).is_some(),
            "round {round}: the tenth frame of 16 ms lowers the multiplier"
        );
    }
}

/// **A panic does not wait for the frames to settle.**
///
/// Tuned to settle over 10 frames and panic after 3, frames of 20 ms — over the
/// setpoint — lower the multiplier on the third. A frame within the setpoint
/// breaks the run: 20, 20, 10, 20, 20 ms decide nothing, the next 20 ms frame
/// does.
#[test]
fn panic_frames_over_the_setpoint_decide_without_waiting_to_settle() {
    let tuned = GovernorConfig {
        settle_frames: 10,
        panic_frames: 3,
        ..GovernorConfig::default()
    };
    let mut governor = FrameGovernor::new(tuned);
    let mut decisions = Vec::new();
    for _ in 0..3 {
        governor.observe(ms(20.0), setpoint());
        decisions.push(governor.decide(setpoint(), false));
    }
    assert!(
        decisions[..2].iter().all(Option::is_none) && decisions[2].is_some(),
        "the third frame over the setpoint decides: {decisions:?}"
    );

    let mut governor = FrameGovernor::new(tuned);
    let mut decisions = Vec::new();
    for frame in [20.0, 20.0, 10.0, 20.0, 20.0, 20.0] {
        governor.observe(ms(frame), setpoint());
        decisions.push(governor.decide(setpoint(), false));
    }
    assert!(
        decisions[..5].iter().all(Option::is_none) && decisions[5].is_some(),
        "only three frames over the setpoint in a row decide: {decisions:?}"
    );
}

/// **A silence decides nothing.**
///
/// Once the frames over the setpoint have lowered the multiplier, ticks that
/// observed no frame since leave it where it is: no frame, no new measurement.
#[test]
fn a_silence_decides_nothing() {
    let mut governor = lowered_to_three_quarters();
    for tick in 0..10 {
        assert_eq!(
            governor.decide(setpoint(), false),
            None,
            "tick {tick} of silence"
        );
    }
    assert!((governor.multiplier() - 0.75).abs() < 1e-6);
}

/// **The ceiling memory blocks raising for `memory_seconds` of frame time, then
/// lets it.**
///
/// Frames of 30 ms lower the multiplier from 1.0 to 0.7, then from 0.7 to 0.49:
/// the governor remembers 0.7. Frames of 5 ms follow, far under the target: for
/// five seconds of frame time the multiplier rises but never past 0.7 − 0.06 =
/// 0.64; past them, it rises above it.
#[test]
fn the_ceiling_memory_blocks_raising_for_memory_seconds_of_frame_time_then_lets_it() {
    let memory = Duration::from_secs_f32(GovernorConfig::default().memory_seconds);
    let mut governor = governor();
    let mut lowered = Vec::new();
    for _ in 0..2 {
        observe(&mut governor, 3, ms(30.0));
        lowered.push(governor.decide(setpoint(), false));
    }
    assert!(
        matches!(lowered[..], [Some(a), Some(b)] if (a - 0.7).abs() < 1e-3 && (b - 0.49).abs() < 1e-3),
        "30 ms frames lower the multiplier to 0.7 then 0.49: {lowered:?}"
    );
    // The frame time since the second cut, which left 0.7.
    let mut since_cut = Duration::ZERO;
    let mut fast = |governor: &mut FrameGovernor, decide: bool| {
        governor.observe(ms(5.0), setpoint());
        since_cut += ms(5.0);
        if decide {
            governor.decide(setpoint(), false);
        }
        since_cut
    };

    // The estimate catches up with the fast frames before any decision.
    for _ in 0..100 {
        fast(&mut governor, false);
    }
    let cap = 0.7 - 0.06;
    let mut highest = governor.multiplier();
    let mut frames = 0_u32;
    loop {
        let at = fast(&mut governor, frames % 3 == 2);
        frames += 1;
        if at + ms(100.0) >= memory {
            break;
        }
        highest = highest.max(governor.multiplier());
        assert!(
            governor.multiplier() <= cap + 1e-4,
            "{at:?} of frame time after the cut, the multiplier {} passed {cap}",
            governor.multiplier()
        );
    }
    assert!(
        highest > 0.49 + 0.02,
        "under the ceiling, the fast frames still raise the multiplier: {highest}"
    );

    while fast(&mut governor, frames % 3 == 2) < memory + ms(1_000.0) {
        frames += 1;
    }
    assert!(
        governor.multiplier() > cap + 0.01,
        "five seconds of frame time after the cut, the multiplier rises past {cap}: {}",
        governor.multiplier()
    );
}

/// **With no lever, the governor does not lower.**
///
/// Frames of 20 ms would lower the multiplier, but every agent is on its floor
/// (`no_lever`): it stays at 1.0. Once a lever is back, the same frames lower
/// it.
#[test]
fn no_lever_stops_lowering() {
    let mut governor = governor();
    for _ in 0..30 {
        governor.observe(ms(20.0), setpoint());
        assert_eq!(governor.decide(setpoint(), true), None);
    }
    assert_eq!(governor.multiplier(), 1.0);

    governor.observe(ms(20.0), setpoint());
    let lowered = governor.decide(setpoint(), false);
    assert!(
        lowered.is_some_and(|m| m < 1.0),
        "with a lever, 20 ms frames lower the multiplier: {lowered:?}"
    );
}

/// **With no lever, the governor still raises.**
///
/// The multiplier was lowered to 0.75; frames of 5 ms run far under the target:
/// with every agent on its floor, the multiplier rises all the same.
#[test]
fn no_lever_does_not_stop_raising() {
    let mut governor = lowered_to_three_quarters();
    observe(&mut governor, 200, ms(5.0));
    let raised = governor.decide(setpoint(), true);

    assert!(
        raised.is_some_and(|m| m > 0.75),
        "fast frames raise the multiplier without a lever: {raised:?}"
    );
}

/// **Inside the band, nothing changes.**
///
/// The multiplier was lowered to 0.75; frames of 13 ms or of 14.5 ms — between
/// 12.75 ms and the 15 ms target — hold it there, decision after decision.
#[test]
fn inside_the_band_nothing_changes() {
    for frame in [13.0, 14.5] {
        let mut governor = lowered_to_three_quarters();
        observe(&mut governor, 400, ms(frame));
        let changes = observe_deciding(&mut governor, 60, ms(frame));

        assert!(
            changes.is_empty(),
            "frames of {frame} ms are inside the band, changed to {changes:?}"
        );
        assert!((governor.multiplier() - 0.75).abs() < 1e-6);
    }
}

/// **A change smaller than 0.02 is not made.**
///
/// Frames of 15.2 ms are just over the 15 ms target: lowering would take the
/// multiplier from 1.0 to 15 / 15.2 ≈ 0.987, a change of 0.013. Decision after
/// decision, it stays 1.0; frames of 15.5 ms (0.968) do lower it.
#[test]
fn a_change_smaller_than_two_hundredths_is_not_made() {
    let mut governor = governor();
    let changes = observe_deciding(&mut governor, 90, ms(15.2));
    assert!(
        changes.is_empty(),
        "0.013 is too small a change, changed to {changes:?}"
    );
    assert_eq!(governor.multiplier(), 1.0);

    let mut governor = self::governor();
    let changes = observe_deciding(&mut governor, 20, ms(15.5));
    assert!(
        changes
            .first()
            .is_some_and(|m| (m - 15.0 / 15.5).abs() < 1e-3),
        "0.032 is a change: {changes:?}"
    );
}

/// **Raising moves the multiplier a quarter of the way to its target.**
///
/// The multiplier was lowered to 0.75 a long time ago (in frame time); frames
/// of 12 ms run under the band: the target is 0.75 × 15 / 12 = 0.9375, and one
/// decision moves the multiplier to 0.75 + 0.25 × 0.1875 ≈ 0.797.
#[test]
fn raising_moves_the_multiplier_a_quarter_of_the_way_to_its_target() {
    let mut governor = lowered_to_three_quarters();
    observe(&mut governor, 1_000, ms(12.0));
    let raised = governor.decide(setpoint(), false);

    let expected = 0.75 + 0.25 * (0.75 * 15.0 / 12.0 - 0.75);
    assert!(
        raised.is_some_and(|m| (m - expected).abs() < 1e-3),
        "raised to {expected}, got {raised:?}"
    );
}

/// **A hitch as the very first frame leaves the multiplier at rest.**
///
/// The engine's first frame is its longest — pipelines built, assets uploaded:
/// 100 ms, a second, ten seconds. Frames of 10 ms follow, far under the target.
/// The first frame is as much a hitch as one among fast frames, and the
/// governor, deciding after every frame, never changes the multiplier from 1.0.
#[test]
fn a_hitch_as_the_very_first_frame_leaves_the_multiplier_at_rest() {
    for hitch in [ms(100.0), ms(1_000.0), ms(10_000.0)] {
        let mut governor = governor();
        let mut changes = observe_deciding(&mut governor, 1, hitch);
        changes.extend(observe_deciding(&mut governor, 60, ms(10.0)));

        assert!(
            changes.is_empty(),
            "a first frame of {hitch:?} before 10 ms frames changes nothing, changed to {changes:?}"
        );
        assert_eq!(
            governor.multiplier(),
            1.0,
            "after a first frame of {hitch:?}"
        );
    }
}

/// **One long frame right after a change does not lower the multiplier.**
///
/// The multiplier was lowered to 0.75 long ago, and frames of 10 ms raised it
/// to 0.8125. The frame right after the raise lasts 30 ms — over the setpoint,
/// under twice it, so not a hitch — and the two after it 10 ms: in a low-pass
/// of a quarter second, the 30 ms frame weighs its 30 ms, about a ninth, and
/// the estimate stays near 12 ms, under the band. The same frames in another
/// order (10, 30, 10 ms) raise the multiplier; in this one, the governor must
/// not cut it by 30 %.
#[test]
fn a_single_long_frame_right_after_a_change_does_not_lower_the_multiplier() {
    let mut governor = lowered_to_three_quarters();
    observe(&mut governor, 600, ms(10.0));
    let raised = governor.decide(setpoint(), false);
    assert!(
        raised.is_some_and(|m| (m - 0.8125).abs() < 1e-3),
        "six seconds of 10 ms frames raise the multiplier a quarter of the way to 1.0: {raised:?}"
    );

    for frame in [30.0, 10.0, 10.0] {
        governor.observe(ms(frame), setpoint());
    }
    let decision = governor.decide(setpoint(), false);

    assert!(
        decision.is_none_or(|m| m >= 0.8125),
        "one 30 ms frame among 10 ms frames lowered the multiplier to {decision:?}"
    );
}

/// **A small cut is raised back once its memory has passed.**
///
/// Frames of 15.5 ms lower the multiplier from 1.0 to 15 / 15.5 ≈ 0.968, a cut
/// of 0.032. Frames of 5 ms follow, a third of the target, for ten seconds of
/// frame time: the ceiling 1.0 left keeps the multiplier under 0.94 for five of
/// them, then lets it go. The way back, 0.032, is more than the smallest change
/// made, and a raise is at least that change and never past its goal: the
/// multiplier returns to 1.0.
#[test]
fn a_small_cut_is_raised_back_once_its_memory_has_passed() {
    let mut governor = governor();
    // A quarter second of them: what the governor measures before deciding.
    observe(&mut governor, 17, ms(15.5));
    let lowered = governor.decide(setpoint(), false);
    assert!(
        lowered.is_some_and(|m| (m - 15.0 / 15.5).abs() < 1e-3),
        "15.5 ms frames lower the multiplier to about 0.968: {lowered:?}"
    );

    let raised = observe_deciding(&mut governor, 2_000, ms(5.0));

    assert!(
        (governor.multiplier() - 1.0).abs() < 1e-6,
        "ten seconds of 5 ms frames bring the multiplier back to 1.0, it stayed at {} (raised to {raised:?})",
        governor.multiplier()
    );
}

/// Whether `kept` is `expected`, to the nanosecond a conversion through
/// seconds may lose.
fn keeps(kept: Option<Duration>, expected: Duration) -> bool {
    kept.is_some_and(|kept| kept.abs_diff(expected) <= Duration::from_nanos(1))
}

/// **A frame under the hitch bound is kept as it is.**
///
/// Its sample is the frame itself — 12 ms within the setpoint, 20 ms over it
/// but under `hitch_ratio × setpoint` — what GORNA's planning margin reads of
/// it.
#[test]
fn observe_keeps_a_frame_under_the_hitch_bound_as_it_is() {
    let mut governor = governor();
    let kept = governor.observe(ms(12.0), setpoint());
    assert!(
        keeps(kept, ms(12.0)),
        "a 12 ms frame is kept at 12 ms: {kept:?}"
    );
    let kept = governor.observe(ms(20.0), setpoint());
    assert!(
        keeps(kept, ms(20.0)),
        "a 20 ms frame, over the setpoint but under the hitch bound, is kept at 20 ms: {kept:?}"
    );
}

/// **A hitch is kept nowhere.**
///
/// A 100 ms frame right after one within the setpoint, and the engine's first
/// frame when it is that long, are hitches: `observe` returns no sample, so
/// GORNA's margin never reads them. A frame that took no time measured nothing,
/// and returns none either.
#[test]
fn observe_keeps_no_sample_of_a_hitch() {
    let mut first = governor();
    assert_eq!(
        first.observe(ms(100.0), setpoint()),
        None,
        "the engine's first frame, 100 ms long, is a hitch"
    );

    let mut governor = governor();
    observe(&mut governor, 5, ms(10.0));
    assert_eq!(
        governor.observe(ms(100.0), setpoint()),
        None,
        "100 ms after frames within the setpoint is a hitch"
    );
    assert_eq!(
        governor.observe(Duration::ZERO, setpoint()),
        None,
        "a frame that took no time measured nothing"
    );
}

/// **A load frame is kept bounded.**
///
/// A 100 ms frame right after a 20 ms one, over the setpoint, is load: its
/// sample is bounded at `hitch_ratio × setpoint` — 33.3 ms — so a load's frames
/// reach GORNA's margin no longer than that.
#[test]
fn observe_keeps_a_load_frame_bounded_at_the_hitch_ratio() {
    let mut governor = governor();
    observe(&mut governor, 5, ms(10.0));
    let _ = governor.observe(ms(20.0), setpoint());
    let bound = setpoint().mul_f64(f64::from(GovernorConfig::default().hitch_ratio));

    let kept = governor.observe(ms(100.0), setpoint());

    assert!(
        keeps(kept, bound),
        "100 ms after an overrun is load, kept at {bound:?}: {kept:?}"
    );
}
