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

//! The margin the rounds plan with reads the frames the governor keeps: a
//! startup's hitches are left out, a load's frames enter bounded, so neither
//! makes the margin take the target from every agent.

use super::*;

use crate::gorna::planning::{z_of, PlanningConfig};

use StrategyId::{Balanced, HighPerformance, LowPower};

/// The longest an engine's first frames last, one DCC tick or more each.
///
/// Pipelines compiled, assets uploaded: ten of them, varied, so the DCC's
/// first decision falls among them.
const STARTUP_MS: [f64; 10] = [
    900.0, 420.0, 250.0, 120.0, 310.0, 80.0, 200.0, 60.0, 150.0, 95.0,
];

/// Three agents of three rows each, costing what they quote: `render` 2, 4
/// or 7 ms, `physics` 1, 2 or 3 ms, `audio` 0.5, 1 or 1.5 ms.
fn three_agents() -> Vec<SimAgent> {
    vec![
        SimAgent::new(
            "render",
            vec![
                (LowPower, 2_000, 0.3),
                (Balanced, 4_000, 0.7),
                (HighPerformance, 7_000, 1.0),
            ],
        ),
        SimAgent::new(
            "physics",
            vec![
                (LowPower, 1_000, 0.4),
                (Balanced, 2_000, 0.8),
                (HighPerformance, 3_000, 1.0),
            ],
        ),
        SimAgent::new(
            "audio",
            vec![
                (LowPower, 500, 0.5),
                (Balanced, 1_000, 0.9),
                (HighPerformance, 1_500, 1.0),
            ],
        ),
    ]
}

/// The z the planner's default risk asks.
fn z() -> f64 {
    z_of(PlanningConfig::default().risk)
}

/// **A startup's hitches push no agent to its floor.**
///
/// The engine starts on ten frames of 60 to 900 ms, one DCC tick each, then
/// runs steady frames of three agents and a 2 ms host part, their costs varying
/// by a tenth. The DCC's first decision falls among the startup frames. Every
/// round is planned with a margin no wider than the steady frames' spread
/// warrants — z × 1.4826 × their range, a bound on any median absolute
/// deviation the governor's kept samples can show while the startup's are fewer
/// than half of them — and every decision leaves some agent above its floor.
#[test]
fn a_startups_hitches_push_no_agent_to_its_floor() {
    let mut sim = Simulation::new(
        GovernorConfig::default(),
        three_agents(),
        ms(2.0),
        Arbiters::Planned,
        5,
    )
    .with_noise(0.1);
    for frame in STARTUP_MS {
        sim.hitch(ms(frame));
        sim.run_for(0.001);
    }
    sim.run_for(6.0);

    let frames = sim.frames();
    let startup: Vec<Duration> = frames[..STARTUP_MS.len()].iter().map(|f| f.time).collect();
    let expected: Vec<Duration> = STARTUP_MS.iter().map(|&frame| ms(frame)).collect();
    assert_eq!(startup, expected, "each startup frame is one tick");
    let steady = &frames[STARTUP_MS.len()..];
    let longest = steady.iter().map(|f| f.time).max().expect("steady frames");
    let shortest = steady.iter().map(|f| f.time).min().expect("steady frames");
    let warranted = Duration::from_secs_f64(z() * 1.4826 * (longest - shortest).as_secs_f64());
    let first = sim
        .decisions()
        .first()
        .map(|decision| decision.at)
        .expect("the governor decides on the startup's frames");
    assert!(
        first <= frames[STARTUP_MS.len()].at,
        "the first decision is made on the startup's frames, at {first:?}"
    );

    let margins = sim.margins();
    assert_eq!(
        margins.len(),
        sim.decisions().len() + 1,
        "one margin a round: the first epoch's and each decision's"
    );
    let widest = margins.iter().copied().max().unwrap_or_default();
    assert!(
        widest <= warranted,
        "the widest margin {widest:?} exceeds what steady frames of {shortest:?} to \
         {longest:?} warrant ({warranted:?}): {margins:?}"
    );
    let floored: Vec<&SimDecision> = (sim.decisions().iter())
        .filter(|decision| !decision.lever)
        .collect();
    assert!(
        floored.is_empty(),
        "decisions left every agent on its floor: {floored:?} (margins {margins:?})"
    );
}

/// One agent whose rows quote 1, 2 or 4 ms but take forty times that,
/// varying by nine tenths each frame: frames of a load far past the setpoint.
fn underquoting() -> Vec<SimAgent> {
    let mut heavy = SimAgent::new(
        "heavy",
        vec![
            (LowPower, 1_000, 0.3),
            (Balanced, 2_000, 0.7),
            (HighPerformance, 4_000, 1.0),
        ],
    );
    heavy.bias = vec![40.0; 3];
    vec![heavy]
}

/// **A sustained load bounds the margin to its bounded frames.**
///
/// Ten seconds of an agent taking forty times its quotes — frames of tens to
/// hundreds of ms — under a governor taking a tenth at most from the multiplier
/// at each decision, so it decides again and again under the load, long after
/// thirty frames. The governor keeps a load frame bounded at `hitch_ratio ×
/// setpoint`, so no round is planned with a margin wider than z × 1.4826 × that
/// bound: never the tens of ms past the target the raw frames' spread would
/// ask.
#[test]
fn a_sustained_load_bounds_the_margin_to_its_bounded_frames() {
    let cfg = GovernorConfig {
        max_drop: 0.1,
        ..GovernorConfig::default()
    };
    let mut sim =
        Simulation::new(cfg, underquoting(), ms(2.0), Arbiters::PlannedOnQuotes, 9).with_noise(0.9);
    sim.run_for(10.0);

    let bound_frame = setpoint().mul_f64(f64::from(cfg.hitch_ratio));
    let loaded = (sim.frames().iter())
        .filter(|frame| frame.time > bound_frame)
        .count();
    assert!(
        loaded > 30,
        "the load runs more than thirty frames past the bound: {loaded}"
    );
    let bound = Duration::from_secs_f64(z() * 1.4826 * bound_frame.as_secs_f64());
    let margins = sim.margins();
    let widest = margins.iter().copied().max().unwrap_or_default();
    assert!(
        widest <= bound,
        "the widest margin {widest:?} exceeds what bounded load frames allow ({bound:?}): \
         {margins:?}"
    );
    let late = (sim.decisions().iter())
        .filter(|decision| {
            let before = (sim.frames().iter()).filter(|frame| frame.at < decision.at);
            before.count() > 30
        })
        .count();
    assert!(
        late > 0,
        "the governor decides under the load after thirty frames: {:?}",
        sim.decisions()
    );
}

/// **A round opened just after a startup plans no margin past the target.**
///
/// The engine starts on the ten long frames of [`STARTUP_MS`]; the governor
/// leaves the first out as a hitch and keeps the nine after it, each following
/// an overrun, as load bounded at `hitch_ratio × setpoint`. Then it runs steady
/// 10 ms frames. A round can open on any tick — a pressure the heuristics read,
/// a forecast breach, a family switched on — not only when the governor
/// decides. Whatever frame it opens on, the margin it is planned with stays
/// under the setpoint: a margin as long as the target leaves no agent anything
/// above its floor.
#[test]
fn a_round_opened_just_after_a_startup_plans_no_margin_past_the_target() {
    use crate::gorna::planning::Planner;
    use crate::governor::FrameGovernor;
    use khora_core::agent::gorna::protocol::Epoch;

    let mut governor = FrameGovernor::new(GovernorConfig::default());
    let mut planner = Planner::new(PlanningConfig::default());
    let steady = (0..60).map(|i| ms(9.5 + f64::from(i % 3) * 0.5));
    let frames: Vec<Duration> = STARTUP_MS.iter().map(|&f| ms(f)).chain(steady).collect();

    let mut widest = (Duration::ZERO, 0);
    for (at, frame) in frames.into_iter().enumerate() {
        if let Some(kept) = governor.observe(frame, setpoint()) {
            planner.observe_frame(kept, setpoint());
        }
        let margin = planner.planning(&[], Epoch(1)).margin;
        if margin > widest.0 {
            widest = (margin, at);
        }
    }

    assert!(
        widest.0 < setpoint(),
        "a round opened on frame {} is planned with a margin of {:?}, past the {:?} target",
        widest.1,
        widest.0,
        setpoint()
    );
}

/// `count` frames of `mean_ms`, normally spread by 1 ms, drawn by a fixed
/// generator from `seed`.
fn normally_around(mean_ms: f64, count: usize, seed: u64) -> Vec<Duration> {
    let quantile = |p: f64| if p < 0.5 { -z_of(p) } else { z_of(1.0 - p) };
    let mut state = seed;
    (0..count)
        .map(|_| {
            state = (state.wrapping_mul(6_364_136_223_846_793_005))
                .wrapping_add(1_442_695_040_888_963_407);
            let uniform = ((state >> 11) as f64 + 0.5) / (1_u64 << 53) as f64;
            ms(mean_ms + quantile(uniform))
        })
        .collect()
}

/// **Frames spread around the target keep a margin of their spread.**
///
/// Frames that run at the 60 Hz target, normally spread by 1 ms, are no load:
/// half of them run past it, and three in a row past it followed by one within
/// come by chance about one frame in sixteen. Each frame goes through the
/// governor to the planner, as the DCC feeds it. Once 120 frames ran, whatever
/// frame a round opens on, it is planned with at least half the z × 1 ms their
/// spread asks — a margin of zero plans every frame at the target, half of them
/// past it, ten times the risk.
#[test]
fn frames_spread_around_the_target_keep_a_margin_of_their_spread() {
    use crate::gorna::planning::Planner;
    use crate::governor::FrameGovernor;
    use khora_core::agent::gorna::protocol::Epoch;

    let mut governor = FrameGovernor::new(GovernorConfig::default());
    let mut planner = Planner::new(PlanningConfig::default());
    let target_ms = setpoint().as_secs_f64() * 1_000.0;
    // z × the 1 ms the frames are spread by.
    let asked = ms(z());

    let (mut short, mut zero, mut read) = (0_usize, 0_usize, 0_usize);
    for (at, frame) in normally_around(target_ms, 1_200, 7).into_iter().enumerate() {
        if let Some(kept) = governor.observe(frame, setpoint()) {
            planner.observe_frame(kept, setpoint());
        }
        if at < 120 {
            continue;
        }
        let margin = planner.planning(&[], Epoch(1)).margin;
        read += 1;
        if margin < asked / 2 {
            short += 1;
        }
        if margin.is_zero() {
            zero += 1;
        }
    }

    assert_eq!(
        short, 0,
        "of {read} rounds opened on frames spread by 1 ms around the target, {short} are \
         planned with less than half the {asked:?} the spread asks, {zero} with none"
    );
}

/// The first frames of an engine whose startup runs under the hitch bound.
///
/// Shaders compiled and textures streamed over ten frames of 17 to 32 ms,
/// between one and two setpoints, so the governor keeps each as it ran.
const STREAMING_MS: [f64; 10] = [30.0, 18.0, 26.0, 32.0, 20.0, 28.0, 17.0, 31.0, 22.0, 27.0];

/// **A round opened after a startup under the hitch bound plans no margin past
/// the target.**
///
/// The engine starts on the ten frames of [`STREAMING_MS`], each kept as it
/// ran, then runs steady 9.5 to 10.5 ms frames. Once the steady frames have
/// begun, whatever frame a round opens on, the margin it is planned with stays
/// under the setpoint: the frames it plans for are the steady ones.
#[test]
fn a_round_opened_after_a_startup_under_the_hitch_bound_plans_no_margin_past_the_target() {
    use crate::gorna::planning::Planner;
    use crate::governor::FrameGovernor;
    use khora_core::agent::gorna::protocol::Epoch;

    let mut governor = FrameGovernor::new(GovernorConfig::default());
    let mut planner = Planner::new(PlanningConfig::default());
    for frame in STREAMING_MS {
        let kept = governor.observe(ms(frame), setpoint());
        assert_eq!(
            kept,
            Some(ms(frame)),
            "a frame under the hitch bound is kept"
        );
        planner.observe_frame(ms(frame), setpoint());
    }

    let mut widest = (Duration::ZERO, 0);
    for at in 0..60 {
        let frame = ms(9.5 + f64::from(at % 3) * 0.5);
        if let Some(kept) = governor.observe(frame, setpoint()) {
            planner.observe_frame(kept, setpoint());
        }
        let margin = planner.planning(&[], Epoch(1)).margin;
        if margin > widest.0 {
            widest = (margin, at);
        }
    }

    assert!(
        widest.0 < setpoint(),
        "a round opened on steady frame {} is planned with a margin of {:?}, past the {:?} \
         target",
        widest.1,
        widest.0,
        setpoint()
    );
}

/// **A frame past the target ends no load, even within the margin.**
///
/// A load ends at the first frame within the target after `load_frames` past
/// the target plus the margin; only then is the spread read afresh. 120 steady
/// frames of 9.5 to 10.5 ms keep a margin of about 1.2 ms. Three frames of 30
/// ms run past the target plus it, then one past the 60 Hz target by half the
/// margin: the load runs on, so the margin is still read from the steady frames
/// — not from a window started afresh on that frame, which keeps none.
#[test]
fn a_frame_past_the_target_ends_no_load_even_within_the_margin() {
    use crate::gorna::planning::Planner;
    use khora_core::agent::gorna::protocol::Epoch;

    let mut planner = Planner::new(PlanningConfig::default());
    for at in 0..120 {
        planner.observe_frame(ms(9.5 + f64::from(at % 3) * 0.5), setpoint());
    }
    let steady = planner.planning(&[], Epoch(1)).margin;
    assert!(steady > Duration::ZERO, "steady frames keep a margin");
    for _ in 0..3 {
        planner.observe_frame(ms(30.0), setpoint());
    }

    planner.observe_frame(setpoint() + steady / 2, setpoint());

    let margin = planner.planning(&[], Epoch(1)).margin;
    assert!(
        margin > Duration::ZERO,
        "a frame {:?} past the target ended the load and started the spread afresh: \
         the margin went from {steady:?} to {margin:?}",
        steady / 2
    );
}
