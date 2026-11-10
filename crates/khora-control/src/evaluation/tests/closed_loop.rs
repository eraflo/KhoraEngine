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

//! The frame governor closing the loop through GORNA, simulated: no thread,
//! no wall clock. Agents offer priced rows that cost what they quote; the
//! engine's arbiter shares the target among them; each agent runs the best
//! row its envelope fits. A frame lasts a fixed host part plus the rows the
//! agents run, unless a hitch is injected in its place. The DCC ticks every
//! 50 ms of frame time: the governor observes each frame and decides once a
//! tick; a decision opens an epoch, whose allocation is lent at the next
//! frame start and run from the frame after it.

use super::*;

use StrategyId::{Balanced, HighPerformance, LowPower};

/// The decisions `sim` made from `since` on.
fn decisions_since(sim: &Simulation, since: Duration) -> Vec<SimDecision> {
    (sim.decisions().iter())
        .filter(|decision| decision.at >= since)
        .copied()
        .collect()
}

/// Three agents of three priced rows each: the floors cost 3.5 ms, the full
/// quality rows 11.5 ms.
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

/// The largest distance of the multiplier from `rest` over the decisions
/// made from `since` on.
fn largest_move(sim: &Simulation, since: Duration, rest: f32) -> f32 {
    decisions_since(sim, since)
        .iter()
        .map(|decision| (decision.multiplier - rest).abs())
        .fold(0.0, f32::max)
}

/// **A hitch moves the multiplier less than 0.05.** The engine settled
/// twice: with frame time to spare (a 2 ms host part, every agent on its
/// best row) and loaded (a 9 ms host part, the multiplier lowered to fit).
/// Once settled — no decision for five seconds — one frame of 100 ms, of a
/// second or of ten seconds: over the ten seconds of frames that follow, no
/// decision takes the multiplier 0.05 from where it had settled.
#[test]
fn a_hitch_moves_the_multiplier_less_than_five_hundredths() {
    for host_ms in [2.0, 9.0] {
        for hitch in [ms(100.0), ms(1_000.0), ms(10_000.0)] {
            let mut sim = engine(GovernorConfig::default(), three_agents(), host_ms);
            sim.run_for(30.0);
            let settled = sim.clock();
            assert!(
                decisions_since(&sim, settled - Duration::from_secs(5)).is_empty(),
                "host {host_ms} ms: the engine settles within 25 s, decisions {:?}",
                sim.decisions()
            );
            let rest = sim.multiplier();

            sim.hitch(hitch);
            sim.run_for(10.0 + hitch.as_secs_f64());
            let moved = largest_move(&sim, settled, rest);
            assert!(
                moved < 0.05,
                "host {host_ms} ms, a hitch of {hitch:?}: the multiplier moved {moved} from {rest}, decisions {:?}",
                decisions_since(&sim, settled)
            );
        }
    }
}

/// **A sustained overload returns the frame under the setpoint within a few
/// epochs.** The engine runs with time to spare (a 2 ms host part, 13.5 ms
/// frames); then the host part grows to 9 ms for good — the frame would
/// last 20.5 ms. Within six epochs, the frames are back under the
/// setpoint, and stay there for the ten seconds that follow.
#[test]
fn a_sustained_overload_returns_the_frame_under_the_setpoint_within_a_few_epochs() {
    let mut sim = engine(GovernorConfig::default(), three_agents(), 2.0);
    sim.run_for(5.0);
    assert!(
        (sim.multiplier() - 1.0).abs() < 1e-6,
        "with time to spare, the multiplier rests at 1.0"
    );

    let overloaded = sim.clock();
    sim.set_host(ms(9.0));
    sim.run_for(15.0);

    let last_over = sim
        .frames()
        .iter()
        .rev()
        .find(|frame| frame.time > setpoint())
        .map(|frame| frame.at)
        .expect("the overload runs frames over the setpoint");
    assert!(
        sim.clock() - last_over > Duration::from_secs(10),
        "the frames stay under the setpoint for the last ten seconds; the last over it at {last_over:?} of {:?}",
        sim.clock()
    );
    let epochs = decisions_since(&sim, overloaded)
        .iter()
        .filter(|decision| decision.at <= last_over)
        .count();
    assert!(
        epochs <= 6,
        "the frames are back under the setpoint after {epochs} epochs: {:?}",
        decisions_since(&sim, overloaded)
    );
}

/// Two agents whose plans straddle the governor's band: with a 7 ms host
/// part, `scene` on its floor makes 9 ms frames — far under the 15 ms
/// target, so the governor raises — and on its best row 16 ms — over it,
/// so the governor lowers. No plan holds still.
fn straddling_agents() -> Vec<SimAgent> {
    vec![
        SimAgent::new(
            "scene",
            vec![(LowPower, 1_500, 0.3), (HighPerformance, 8_500, 1.0)],
        ),
        SimAgent::new("audio", vec![(LowPower, 500, 0.5)]),
    ]
}

/// How many times the agents' rows went somewhere and back from `since` on:
/// every second change of one agent's row.
fn round_trips(sim: &Simulation, agents: usize, since: Duration) -> usize {
    (0..agents)
        .map(|agent| {
            sim.switches()
                .iter()
                .filter(|(at, switched)| *at >= since && *switched == agent)
                .count()
                / 2
        })
        .sum()
}

/// **In steady state, a row round-trips at most once per five seconds.** No
/// plan of the straddling agents holds still: the full row overruns the
/// target, the floor leaves it far under. Over sixty seconds of frames once
/// the engine has settled, the rows go up and back at most once per five
/// seconds — the governor remembers the multiplier that overran. The
/// scenario is one that cycles: without that memory, the same engine
/// round-trips more often.
#[test]
fn in_steady_state_a_row_round_trips_at_most_once_per_five_seconds() {
    let window = 60.0;
    let agents = straddling_agents().len();
    let mut sim = engine(GovernorConfig::default(), straddling_agents(), 7.0);
    sim.run_for(10.0);
    let settled = sim.clock();
    sim.run_for(window);
    let trips = round_trips(&sim, agents, settled);

    let allowed = (window / 5.0).ceil() as usize;
    assert!(
        trips <= allowed,
        "{trips} round trips in {window} s, at most {allowed}: decisions {:?}",
        decisions_since(&sim, settled)
    );

    let mut forgetful = engine(
        GovernorConfig {
            memory_seconds: 0.0,
            ..GovernorConfig::default()
        },
        straddling_agents(),
        7.0,
    );
    forgetful.run_for(10.0);
    let settled = forgetful.clock();
    forgetful.run_for(window);
    assert!(
        round_trips(&forgetful, agents, settled) > allowed,
        "without the memory the scenario cycles faster: {} round trips",
        round_trips(&forgetful, agents, settled)
    );
}

/// Three agents whose floors cost 6 ms, every upgrade at least 2 ms more.
fn heavy_floors() -> Vec<SimAgent> {
    vec![
        SimAgent::new(
            "render",
            vec![
                (LowPower, 3_000, 0.3),
                (Balanced, 5_000, 0.7),
                (HighPerformance, 8_000, 1.0),
            ],
        ),
        SimAgent::new(
            "physics",
            vec![
                (LowPower, 2_000, 0.4),
                (Balanced, 4_000, 0.8),
                (HighPerformance, 5_000, 1.0),
            ],
        ),
        SimAgent::new(
            "audio",
            vec![
                (LowPower, 1_000, 0.5),
                (Balanced, 3_000, 0.9),
                (HighPerformance, 4_000, 1.0),
            ],
        ),
    ]
}

/// **With a host part above the target, the multiplier stops at the first
/// all-floors plan.** The host part alone (16 ms) is over the 15 ms target:
/// no plan brings the frame under it. The governor lowers the multiplier
/// until GORNA's plan puts every agent on its floor (no lever left),
/// then lowers it no more — lowering further would change nothing — and
/// stops above its own floor.
#[test]
fn with_a_host_part_above_the_target_the_multiplier_stops_at_the_first_all_floors_plan() {
    let cfg = GovernorConfig::default();
    let mut sim = engine(cfg, heavy_floors(), 16.0);
    sim.run_for(30.0);

    let first_floors = sim
        .decisions()
        .iter()
        .position(|decision| !decision.lever)
        .unwrap_or_else(|| {
            panic!(
                "the multiplier reaches an all-floors plan: {:?}",
                sim.decisions()
            )
        });
    let stopped = sim.decisions()[first_floors].multiplier;
    let later = &sim.decisions()[first_floors + 1..];
    assert!(
        later.iter().all(|decision| decision.multiplier >= stopped),
        "after the first all-floors plan at {stopped}, the multiplier is lowered no more: {:?}",
        sim.decisions()
    );
    assert!(
        (sim.multiplier() - stopped).abs() < 1e-6,
        "it stays at {stopped}: {}",
        sim.multiplier()
    );
    assert!(
        stopped > cfg.output_min + 0.02,
        "it stops above the governor's floor {}: {stopped}",
        cfg.output_min
    );
}
