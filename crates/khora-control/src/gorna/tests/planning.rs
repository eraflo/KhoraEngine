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

//! A round planned beyond its frame: a switch of row is charged in weighed
//! quality, the plans keep a margin free of the target that never takes it
//! below the floors, and a row above its floor is planned at the cost its
//! agent's workload trend expects over the horizon. The planner learns, from
//! the reports and the frames, each switch's preparation, the frames' spread
//! and each agent's trend. Every round figure is in whole microseconds.

use super::decide::{bid, round};
use super::*;
use crate::cost_model::ComplexityClass;
use crate::gorna::arbiter::{Arbiter, Proposal, Refusal, Shield};
use crate::gorna::decide::{admit, Admission, Plan};
use crate::gorna::planning::{growth, margin, z_of, Planner, PlanningConfig};
use khora_core::agent::gorna::protocol::{BindingConstraint, Trigger};
use khora_core::agent::gorna::Planning;
use khora_core::agent::KEEP_EPOCHS;
use khora_core::lane::cost::Device;

use AgentImportance::{Critical, Important};
use StrategyId::{Balanced, Custom, HighPerformance, LowPower};

/// Two values closer than this are equal: qualities are `f32`, read as
/// `f64`.
const EPS: f64 = 1e-6;

// ── Building a planned round ──────────────────────────────────────

/// A planning holding its plans 5 s, a switch's transient 250 ms, and each
/// of `switch` costing its preparation; no margin, no growth.
fn holding(switch: Vec<(AgentId, StrategyId, Duration)>) -> Planning {
    Planning {
        horizon: Duration::from_secs(5),
        transient: ms(250),
        switch,
        ..Planning::default()
    }
}

/// A planning keeping `margin_us` free of the target, and nothing else.
fn keeping(margin_us: u64) -> Planning {
    Planning {
        margin: us(margin_us),
        ..Planning::default()
    }
}

/// A planning expecting `agent`'s costs to grow by `by` over the horizon,
/// and nothing else.
fn growing(agent: &'static str, by: f64) -> Planning {
    Planning {
        horizon: Duration::from_secs(5),
        growth: vec![(AgentId::new(agent), by)],
        ..Planning::default()
    }
}

/// The bids and the plans admission gives `round`; panics when its floors do
/// not fit.
fn admitted<'r>(round: &'r Round<'_>) -> (Vec<&'r Bid>, Vec<Plan>) {
    match admit(round) {
        Admission::Admitted { bids, plans } => (bids, plans),
        Admission::Safety(allocation, _) => panic!("the floors fit: {allocation:?}"),
    }
}

/// The proposal planning, for each of `bids` in order, the row `rows` names
/// for it, or its floor.
fn proposing(bids: &[&Bid], rows: &[(&'static str, StrategyId)]) -> Proposal {
    Proposal {
        rows: bids
            .iter()
            .map(|bid| {
                let wanted = rows
                    .iter()
                    .find(|(name, _)| AgentId::new(name) == bid.offer.agent)
                    .map_or(bid.offer.floor, |(_, row)| *row);
                bid.offer.rows.iter().position(|row| row.id == wanted)
            })
            .collect(),
        price: None,
        rejected: Vec::new(),
    }
}

/// `oak` (Critical) offering 2 ms at 0.4 or `upgrade_us` at 1.0, and `pine`
/// (Important) offering its 2 ms floor only.
fn oak_and_pine(upgrade_us: u64) -> [Bid; 2] {
    [
        bid(
            offer(
                "oak",
                vec![
                    row(LowPower, 2_000, 0.4),
                    row(HighPerformance, upgrade_us, 1.0),
                ],
                LowPower,
            ),
            Critical,
        ),
        bid(
            offer("pine", vec![row(LowPower, 2_000, 0.4)], LowPower),
            Important,
        ),
    ]
}

/// The row the decision plans for `name`.
fn planned_for(rows: &[(AgentId, StrategyId)], name: &'static str) -> Option<StrategyId> {
    rows.iter()
        .find(|(agent, _)| *agent == AgentId::new(name))
        .map(|(_, row)| *row)
}

// ── The switch charge ─────────────────────────────────────────────

/// **A gain below the switch charge keeps the running row.** `oak` weighs 1
/// and runs its floor, 1 ms at 0.5; a switch to its 2 ms row costs 5 ms to
/// prepare. Planned over 5 s with a 250 ms transient, the switch is charged
/// 1 × (5 + 250) ms / 5 s = 0.051 of weighed quality. A row of 0.54 gains
/// 0.04: it is not taken, and the plan is worth what the floor lends. A row
/// of 0.56 gains 0.06: it is taken, and the plan is worth 0.56 less the
/// charge, though it lends 0.56 of weighed quality. Unplanned, both are
/// taken; with no horizon, nothing is charged.
#[test]
fn a_gain_below_the_switch_charge_keeps_the_running_row() {
    let oak = AgentId::new("oak");
    let planning = holding(vec![(oak, HighPerformance, ms(5))]);
    for (quality, taken) in [(0.54_f32, false), (0.56_f32, true)] {
        let bids = [bid(
            offer(
                "oak",
                vec![
                    row(LowPower, 1_000, 0.5),
                    row(HighPerformance, 2_000, quality),
                ],
                LowPower,
            ),
            Critical,
        )];
        let planned = Round {
            planning: planning.clone(),
            ..round(Trigger::Refresh, 10_000, &bids)
        };

        let decision = Arbiter::default().decide(&planned);

        let (row, lends, worth) = if taken {
            (
                HighPerformance,
                f64::from(quality),
                f64::from(quality) - 0.051,
            )
        } else {
            (LowPower, 0.5, 0.5)
        };
        assert_eq!(
            decision.rows,
            vec![(oak, row)],
            "a gain of {:.2} against a charge of 0.051",
            quality - 0.5
        );
        assert!(
            (decision.quality - lends).abs() < EPS,
            "the rows lend {lends}: {}",
            decision.quality
        );
        assert!(
            (decision.value - worth).abs() < EPS,
            "the plan is worth {worth}: {}",
            decision.value
        );

        let unplanned = Arbiter::default().decide(&round(Trigger::Refresh, 10_000, &bids));
        assert_eq!(
            unplanned.rows,
            vec![(oak, HighPerformance)],
            "nothing charged"
        );
        let no_horizon = Round {
            planning: Planning {
                horizon: Duration::ZERO,
                ..planning.clone()
            },
            ..round(Trigger::Refresh, 10_000, &bids)
        };
        let decision = Arbiter::default().decide(&no_horizon);
        assert_eq!(
            decision.rows,
            vec![(oak, HighPerformance)],
            "a planning of no horizon charges no switch"
        );
    }
}

/// **A gain below the switch charge lends no envelope the upgrade fits.**
/// The agent runs the best row its envelope fits, whatever row the plan
/// names: `oak`, kept on its 1 ms floor of 0.5 because a switch to its 2 ms
/// row of 0.54 gains less than the 0.051 it is charged, must not be lent an
/// envelope that 2 ms row fits — otherwise it switches all the same, and the
/// charge damps nothing.
#[test]
fn a_gain_below_the_switch_charge_lends_no_envelope_the_upgrade_fits() {
    let oak = AgentId::new("oak");
    let upgrade = row(HighPerformance, 2_000, 0.54);
    let bids = [bid(
        offer("oak", vec![row(LowPower, 1_000, 0.5), upgrade], LowPower),
        Critical,
    )];
    let planned = Round {
        planning: holding(vec![(oak, HighPerformance, ms(5))]),
        ..round(Trigger::Refresh, 10_000, &bids)
    };

    let decision = Arbiter::default().decide(&planned);

    assert_eq!(decision.rows, vec![(oak, LowPower)], "the charge keeps it");
    let lent = lent_to(&decision.allocation, "oak");
    assert!(
        !fits(&upgrade.cost, &lent),
        "the plan keeps `oak` on its floor, but the envelope lent ({:?}) fits \
         the 2 ms upgrade: the agent runs it, and switches anyway",
        lent.time.cpu
    );
}

/// **An envelope held to its floor leaves no lever.** `oak`, held on its
/// 1 ms floor by the switch charge, is lent an envelope its 2 ms row no
/// longer fits: no row above its floor fits, so a lower target takes
/// nothing back, and the allocation must say so — the governor reads
/// `lever` to know whether lowering the target would change anything.
#[test]
fn an_envelope_held_to_its_floor_leaves_no_lever() {
    let oak = AgentId::new("oak");
    let upgrade = row(HighPerformance, 2_000, 0.54);
    let bids = [bid(
        offer("oak", vec![row(LowPower, 1_000, 0.5), upgrade], LowPower),
        Critical,
    )];
    let planned = Round {
        planning: holding(vec![(oak, HighPerformance, ms(5))]),
        ..round(Trigger::Refresh, 10_000, &bids)
    };

    let decision = Arbiter::default().decide(&planned);

    let lent = lent_to(&decision.allocation, "oak");
    assert!(!fits(&upgrade.cost, &lent), "held to the floor: {lent:?}");
    assert!(
        !decision.allocation.lever,
        "no row above the floor fits {:?}, yet the allocation names a lever",
        lent.time.cpu
    );
}

/// **An envelope held to the plan still fits the planned row.** `oak` runs
/// its floor (1 ms on the host, 1 ms on the graphics processor, 0.5); its
/// upgrade (0.54) costs 2 ms on the graphics processor and half a
/// microsecond more on the host — a corrected quote is not whole
/// microseconds. Held off it by the charge, the envelope must be cut where
/// the upgrade costs clearly more, never below the floor it plans: an
/// envelope its planned row does not fit runs the agent over budget.
#[test]
fn an_envelope_held_to_the_plan_still_fits_the_planned_row() {
    let oak = AgentId::new("oak");
    let mut floor = row(LowPower, 1_000, 0.5);
    floor.cost.time.gpu = us(1_000);
    let mut upgrade = row(HighPerformance, 1_000, 0.54);
    upgrade.cost.time.cpu += Duration::from_nanos(500);
    upgrade.cost.time.gpu = us(2_000);
    let bids = [bid(offer("oak", vec![floor, upgrade], LowPower), Critical)];
    let planned = Round {
        planning: holding(vec![(oak, HighPerformance, ms(5))]),
        ..round(Trigger::Refresh, 10_000, &bids)
    };

    let decision = Arbiter::default().decide(&planned);

    assert_eq!(decision.rows, vec![(oak, LowPower)], "the charge keeps it");
    let lent = lent_to(&decision.allocation, "oak");
    assert!(
        !fits(&upgrade.cost, &lent),
        "held off the upgrade: {lent:?}"
    );
    assert!(
        fits(&floor.cost, &lent),
        "the planned row no longer fits the envelope held to it: {:?}",
        lent.time
    );
}

/// **An envelope is held to the plan at the costs the agent quoted.** The
/// agent runs the best row whose own quote fits its envelope. `oak`
/// (Important) quotes 1 ms at 0.5 (running) and 2 ms at 0.54; the planner's
/// charge (0.75 × 255 ms / 5 s ≈ 0.038) is above the gain (0.75 × 0.04 =
/// 0.03), so the plan keeps the 1 ms row. Its runs of that row took 1.5 ms,
/// so the cost book corrects its rows ×1.5: the envelope lent must still not
/// fit the 2 ms the agent quotes for its upgrade, or it switches anyway.
#[test]
fn an_envelope_is_held_to_the_plan_at_the_costs_the_agent_quoted() {
    let upgrade = row(HighPerformance, 2_000, 0.54);
    for taught in [None, Some(1.5)] {
        let (_, agent) = shared(Bidder::new(
            "oak",
            vec![row(LowPower, 1_000, 0.5), upgrade],
            LowPower,
        ));
        let mut arbitrator = arbitrator();
        if let Some(factor) = taught {
            teach(&mut arbitrator, "oak", LowPower, 1_000, factor);
        }

        let allocation = arbitrator.allocate(
            &playing(),
            &frame_of(10.0),
            Trigger::Refresh,
            &[agent],
            &nothing(),
        );

        let lent = lent_to(&allocation, "oak");
        assert!(
            !fits(&upgrade.cost, &lent),
            "corrected by {taught:?}: the envelope lent ({:?}) fits the 2 ms the \
             agent quotes for its upgrade, so it runs it",
            lent.time.cpu
        );
    }
}

/// **The running row switches free, and a kept row pays only the
/// transient.**
/// - In a round, `oak` runs its 2 ms row of 0.54 and its floor lends 0.5: a
///   preparation the planning names for the running row is not charged, so
///   `oak` stays on it, and the plan is worth the 0.54 it lends.
/// - A row whose layer is kept prepares nothing (named at zero) but its swap
///   still pays the 250 ms transient: 1 × 250 ms / 5 s = 0.05. From a floor
///   of 0.5, a kept row of 0.54 is not taken; one of 0.56 is, and the plan
///   is worth 0.56 less 0.05.
/// - The planner names no switch for the row an offer runs; it names a row
///   its agent ran within the last `KEEP_EPOCHS` epochs (its layer is kept)
///   at zero, and a row run before them at what it measured.
#[test]
fn the_running_row_switches_free_and_a_kept_row_pays_only_the_transient() {
    let oak = AgentId::new("oak");
    let mut running = offer(
        "oak",
        vec![row(LowPower, 1_000, 0.5), row(HighPerformance, 2_000, 0.54)],
        LowPower,
    );
    running.current = HighPerformance;
    let bids = [bid(running, Critical)];
    let planned = Round {
        planning: holding(vec![(oak, HighPerformance, ms(5))]),
        ..round(Trigger::Refresh, 10_000, &bids)
    };

    let decision = Arbiter::default().decide(&planned);

    assert_eq!(decision.rows, vec![(oak, HighPerformance)]);
    assert!(
        (decision.value - 0.54).abs() < EPS,
        "the running row is not charged: {}",
        decision.value
    );

    for (quality, taken) in [(0.54_f32, false), (0.56_f32, true)] {
        let bids = [bid(
            offer(
                "oak",
                vec![
                    row(LowPower, 1_000, 0.5),
                    row(HighPerformance, 2_000, quality),
                ],
                LowPower,
            ),
            Critical,
        )];
        let kept = Round {
            planning: holding(vec![(oak, HighPerformance, Duration::ZERO)]),
            ..round(Trigger::Refresh, 10_000, &bids)
        };

        let decision = Arbiter::default().decide(&kept);

        let (row, worth) = if taken {
            (HighPerformance, f64::from(quality) - 0.05)
        } else {
            (LowPower, 0.5)
        };
        assert_eq!(
            decision.rows,
            vec![(oak, row)],
            "a gain of {:.2} against the transient's 0.05",
            quality - 0.5
        );
        assert!(
            (decision.value - worth).abs() < EPS,
            "the plan is worth {worth}: {}",
            decision.value
        );
    }

    // The planner: the round opens epoch 10; `oak` runs `HighPerformance`.
    let epoch = 10;
    let keep = u64::from(KEEP_EPOCHS);
    let mut planner = Planner::new(PlanningConfig::default());
    planner.observe_report(
        &ran("oak", epoch - keep - 1, Balanced, Some(ms(2)), None),
        Duration::from_secs(1),
    );
    planner.observe_report(
        &ran("oak", epoch - keep, Custom(1), Some(ms(3)), None),
        Duration::from_secs(2),
    );
    planner.observe_report(
        &ran("oak", epoch - 1, HighPerformance, Some(ms(4)), None),
        Duration::from_secs(3),
    );
    let offers = [offer_running(
        "oak",
        &[LowPower, Balanced, Custom(1), HighPerformance],
        HighPerformance,
    )];

    let planning = planner.planning(&offers, Epoch(epoch));

    assert_eq!(
        planning.switch_of(oak, HighPerformance),
        None,
        "the running row"
    );
    assert_eq!(
        planning.switch_of(oak, Custom(1)),
        Some(Duration::ZERO),
        "run {KEEP_EPOCHS} epochs before: its layer is kept, it prepares nothing"
    );
    assert_eq!(
        planning.switch_of(oak, Balanced),
        Some(ms(2)),
        "run {} epochs before: its layer is gone",
        KEEP_EPOCHS + 1
    );
}

/// **A row never prepared costs the agent's mean switch, else the prior.**
/// `oak` measured a switch to `Balanced` of 2 ms and to `HighPerformance` of
/// 4 ms, long before the round: those rows cost what they measured, and
/// `Custom(1)`, never prepared, their mean, 3 ms. `pine` measured none: its
/// upgrade costs the configured prior, 7 ms. Neither running row is named.
/// The planning holds the configured horizon and transient.
#[test]
fn a_row_never_prepared_costs_the_agents_mean_switch_else_the_prior() {
    let (oak, pine) = (AgentId::new("oak"), AgentId::new("pine"));
    let cfg = PlanningConfig {
        horizon: Duration::from_secs(4),
        transient: ms(200),
        switch_prior: ms(7),
        ..PlanningConfig::default()
    };
    let mut planner = Planner::new(cfg);
    planner.observe_report(
        &ran("oak", 1, Balanced, Some(ms(2)), None),
        Duration::from_secs(1),
    );
    planner.observe_report(
        &ran("oak", 2, HighPerformance, Some(ms(4)), None),
        Duration::from_secs(2),
    );
    planner.observe_report(&ran("oak", 3, LowPower, None, None), Duration::from_secs(3));
    let offers = [
        offer_running(
            "oak",
            &[LowPower, Balanced, HighPerformance, Custom(1)],
            LowPower,
        ),
        offer_running("pine", &[LowPower, HighPerformance], LowPower),
    ];

    let planning = planner.planning(&offers, Epoch(20));

    assert_eq!(planning.horizon, Duration::from_secs(4));
    assert_eq!(planning.transient, ms(200));
    assert_eq!(planning.switch_of(oak, Balanced), Some(ms(2)));
    assert_eq!(planning.switch_of(oak, HighPerformance), Some(ms(4)));
    assert_eq!(
        planning.switch_of(oak, Custom(1)),
        Some(ms(3)),
        "never prepared: the mean of oak's"
    );
    assert_eq!(
        planning.switch_of(pine, HighPerformance),
        Some(ms(7)),
        "pine measured none: the prior"
    );
    assert_eq!(planning.switch_of(oak, LowPower), None, "oak runs it");
    assert_eq!(planning.switch_of(pine, LowPower), None, "pine runs it");
    assert_eq!(planner.config(), cfg);
}

/// **The DCC keeps each measured switch per agent and row.** `oak` measured
/// its switch to `Balanced` at 2 ms, then at 6 ms; to `HighPerformance` at
/// 4 ms; `pine` its own to `Balanced` at 1 ms. The round reads the last
/// measured of each (agent, row): 6 ms, 4 ms and 1 ms — `pine`'s is not
/// `oak`'s — and a later report that carries no switch forgets nothing.
#[test]
fn the_dcc_keeps_each_measured_switch_per_agent_and_row() {
    let (oak, pine) = (AgentId::new("oak"), AgentId::new("pine"));
    let mut planner = Planner::new(PlanningConfig::default());
    for (report, at) in [
        (ran("oak", 1, Balanced, Some(ms(2)), None), 1),
        (ran("oak", 2, HighPerformance, Some(ms(4)), None), 2),
        (ran("pine", 2, Balanced, Some(ms(1)), None), 2),
        (ran("oak", 3, Balanced, Some(ms(6)), None), 3),
        (ran("oak", 4, LowPower, Some(us(500)), None), 4),
        (ran("oak", 5, LowPower, None, None), 5),
        (ran("pine", 5, LowPower, None, None), 5),
    ] {
        planner.observe_report(&report, Duration::from_secs(at));
    }
    let rows = [LowPower, Balanced, HighPerformance];
    let offers = [
        offer_running("oak", &rows, LowPower),
        offer_running("pine", &rows, LowPower),
    ];

    let planning = planner.planning(&offers, Epoch(30));

    assert_eq!(
        planning.switch_of(oak, Balanced),
        Some(ms(6)),
        "the last measured"
    );
    assert_eq!(planning.switch_of(oak, HighPerformance), Some(ms(4)));
    assert_eq!(
        planning.switch_of(pine, Balanced),
        Some(ms(1)),
        "pine's own, not oak's"
    );
}

// ── The margin ────────────────────────────────────────────────────

/// **The risk reads as a normal quantile.** A 5 % risk asks z ≈ 1.645, a 1 %
/// risk z ≈ 2.326; a risk of one half or more asks none. Fewer than two
/// frames, or frames that do not vary, ask no margin.
#[test]
fn the_risk_reads_as_a_normal_quantile() {
    assert!((z_of(0.05) - 1.6449).abs() < 1e-3, "{}", z_of(0.05));
    assert!((z_of(0.01) - 2.3263).abs() < 1e-3, "{}", z_of(0.01));
    assert_eq!(z_of(0.5), 0.0);
    assert_eq!(z_of(0.8), 0.0);

    assert_eq!(margin(&[], 0.05), Duration::ZERO);
    assert_eq!(margin(&[ms(16)], 0.05), Duration::ZERO);
    assert_eq!(margin(&[ms(16); 50], 0.05), Duration::ZERO);
}

/// **A risk of zero keeps the widest margin, never none.** A risk of zero
/// or less reads as the smallest positive one: its z is the largest, so the
/// margin over a spread of frames is wider than a 5 % risk's — not zero, as
/// a z that is not a number would make it. The same holds for a risk too
/// small for 1 − risk to differ from 1.
#[test]
fn a_risk_of_zero_keeps_the_widest_margin() {
    let frames = steady(30);
    let usual = margin(&frames, 0.05);
    assert!(usual > Duration::ZERO);

    for risk in [0.0, -1.0, 1e-18] {
        let z = z_of(risk);
        assert!(
            z.is_finite() && z > z_of(1e-6),
            "a risk of {risk} asks the largest z: {z}"
        );
        assert!(
            margin(&frames, risk) > usual,
            "a risk of {risk} keeps {:?}, a 5 % risk {usual:?}",
            margin(&frames, risk)
        );
    }
}

/// `count` frames cycling through 9, 10 and 11 ms: their median is 10 ms,
/// their median absolute deviation 1 ms.
fn steady(count: usize) -> Vec<Duration> {
    (0..count).map(|frame| ms(9 + (frame % 3) as u64)).collect()
}

/// **The margin follows the frames' spread, not their hitches.** 120 frames
/// cycling through 9, 10 and 11 ms spread by a median absolute deviation of
/// 1 ms: at a 5 % risk, the margin is 1.645 × 1.4826 × 1 ms ≈ 2.439 ms. Four
/// of them turned into 100 ms hitches leave the margin as it was, where a
/// standard deviation would grow tenfold. The planner, fed those frames
/// after older ones beyond the 120 it reads, plans the same margin.
#[test]
fn the_margin_follows_the_frames_spread_not_their_hitches() {
    let expected = 1.644_853_6 * 1.4826 * 1e-3;
    let calm = steady(120);
    let mut hitched = calm.clone();
    for frame in [10, 40, 71, 101] {
        hitched[frame] = ms(100);
    }

    let calm_margin = margin(&calm, 0.05);
    let hitched_margin = margin(&hitched, 0.05);

    assert!(
        (calm_margin.as_secs_f64() - expected).abs() < 2e-5,
        "1.645 × 1.4826 × 1 ms: {calm_margin:?}"
    );
    assert!(
        (hitched_margin.as_secs_f64() - calm_margin.as_secs_f64()).abs() < 1e-6,
        "four hitches move nothing: {hitched_margin:?} against {calm_margin:?}"
    );

    let mut planner = Planner::new(PlanningConfig::default());
    for _ in 0..80 {
        planner.observe_frame(ms(60));
        planner.observe_frame(ms(2));
    }
    for frame in &hitched {
        planner.observe_frame(*frame);
    }
    let planning = planner.planning(&[], Epoch(1));
    assert!(
        (planning.margin.as_secs_f64() - calm_margin.as_secs_f64()).abs() < 1e-6,
        "the last 120 frames' margin: {:?}",
        planning.margin
    );
}

/// **The margin never takes the target below the floors.** `oak` (4 ms at
/// 0.4, or 5 ms at 1.0) and `pine` (4 ms) in a 10 ms frame:
/// - unplanned, `oak`'s upgrade fits (9 ms) and is taken;
/// - keeping 5 ms free, the plans share 8 ms — the floors' critical path,
///   not 5 ms: the upgrade does not fit, the round keeps its trigger, and
///   the envelopes lend the floors exactly;
/// - a margin larger than the target does no more, and is never `Safety`;
/// - with `pine` `Stable` on a 5 ms envelope it keeps, a 3 ms margin shares
///   9 ms, what the floor and the kept envelope ask: `pine` keeps it, and
///   `oak` runs its floor.
#[test]
fn the_margin_never_takes_the_target_below_the_floors() {
    let bids = [
        bid(
            offer(
                "oak",
                vec![row(LowPower, 4_000, 0.4), row(HighPerformance, 5_000, 1.0)],
                LowPower,
            ),
            Critical,
        ),
        bid(
            offer("pine", vec![row(LowPower, 4_000, 0.4)], LowPower),
            Important,
        ),
    ];
    let unplanned = Arbiter::default().decide(&round(Trigger::Refresh, 10_000, &bids));
    assert_eq!(
        planned_for(&unplanned.rows, "oak"),
        Some(HighPerformance),
        "4 + 5 ms fit 10 ms"
    );

    for margin_us in [5_000, 50_000] {
        let planned = Round {
            planning: keeping(margin_us),
            ..round(Trigger::Refresh, 10_000, &bids)
        };

        let decision = Arbiter::default().decide(&planned);

        assert_eq!(
            decision.allocation.trigger,
            Trigger::Refresh,
            "a margin of {margin_us} µs never makes the round Safety"
        );
        assert_eq!(
            planned_for(&decision.rows, "oak"),
            Some(LowPower),
            "a margin of {margin_us} µs leaves 8 ms to share"
        );
        assert_eq!(
            lent_to(&decision.allocation, "oak").time.cpu,
            us(4_000),
            "the floors' critical path, shared"
        );
        assert_eq!(lent_to(&decision.allocation, "pine").time.cpu, us(4_000));
    }

    let mut stable = bids.clone();
    stable[1].mode = AdaptationMode::Stable;
    stable[1].kept = Some(cpu_envelope(5_000));
    let planned = Round {
        planning: keeping(3_000),
        ..round(Trigger::Refresh, 10_000, &stable)
    };

    let decision = Arbiter::default().decide(&planned);

    assert_eq!(decision.allocation.trigger, Trigger::Refresh);
    assert_eq!(
        lent_to(&decision.allocation, "pine"),
        cpu_envelope(5_000),
        "the kept envelope stays"
    );
    assert_eq!(planned_for(&decision.rows, "oak"), Some(LowPower));
    assert_eq!(lent_to(&decision.allocation, "oak").time.cpu, us(4_000));
}

/// **The shield checks the planned target.** In a 10 ms frame keeping 2 ms
/// free, `oak`'s 7 ms row beside `pine`'s 2 ms (9 ms) is refused, over the
/// host's time; its 6 ms row (8 ms, at the planned target) and the floors
/// are accepted. Unplanned, the 7 ms row is accepted. Planned at a growth of
/// 1.5, `oak`'s 7 ms row asks 10.5 ms and is refused in an 11 ms frame, where
/// it fits at its quote; the floors, planned at their quote, are accepted.
#[test]
fn the_shield_checks_the_planned_target() {
    let mut bids = oak_and_pine(7_000);
    bids[0].offer.rows.push(row(Balanced, 6_000, 0.7));
    let planned = Round {
        planning: keeping(2_000),
        ..round(Trigger::Refresh, 10_000, &bids)
    };
    let (admitted_bids, base) = admitted(&planned);

    assert_eq!(
        Shield::check(
            &planned,
            &admitted_bids,
            &base,
            &proposing(&admitted_bids, &[("oak", HighPerformance)])
        ),
        Err(Refusal::Breach(BindingConstraint::Time(Device::Cpu))),
        "7 + 2 ms are over 10 − 2 ms"
    );
    assert_eq!(
        Shield::check(
            &planned,
            &admitted_bids,
            &base,
            &proposing(&admitted_bids, &[("oak", Balanced)])
        ),
        Ok(()),
        "6 + 2 ms are at 10 − 2 ms"
    );
    assert_eq!(
        Shield::check(
            &planned,
            &admitted_bids,
            &base,
            &proposing(&admitted_bids, &[])
        ),
        Ok(())
    );

    let unplanned = round(Trigger::Refresh, 10_000, &bids);
    let (unplanned_bids, unplanned_base) = admitted(&unplanned);
    assert_eq!(
        Shield::check(
            &unplanned,
            &unplanned_bids,
            &unplanned_base,
            &proposing(&unplanned_bids, &[("oak", HighPerformance)])
        ),
        Ok(()),
        "7 + 2 ms fit 10 ms"
    );

    let grown = Round {
        planning: growing("oak", 1.5),
        ..round(Trigger::Refresh, 11_000, &bids)
    };
    let (grown_bids, grown_base) = admitted(&grown);
    assert_eq!(
        Shield::check(
            &grown,
            &grown_bids,
            &grown_base,
            &proposing(&grown_bids, &[("oak", HighPerformance)])
        ),
        Err(Refusal::Breach(BindingConstraint::Time(Device::Cpu))),
        "7 × 1.5 + 2 ms are over 11 ms"
    );
    assert_eq!(
        Shield::check(
            &grown,
            &grown_bids,
            &grown_base,
            &proposing(&grown_bids, &[])
        ),
        Ok(()),
        "the floors are planned at their quote"
    );
}

// ── The growth ────────────────────────────────────────────────────

/// **A rising workload holds back an upgrade that would not last.** `oak`
/// (2 ms, or 6 ms at 1.0) beside `pine` (2 ms) in a 10 ms frame:
/// - with no trend, the upgrade (8 ms) is taken;
/// - expected to grow by 1.5 over the horizon, it would ask 11 ms: it is not;
/// - expected to grow by 1.2, it asks 7.2 ms and is taken, and the envelope
///   lent for it holds the growth: 7.2 ms plus its part of 80 % of the
///   0.8 ms left, 7.7 ms;
/// - expected to grow by 3, the floors are still planned at their quote: the
///   round, of 5 ms, is not `Safety`.
#[test]
fn a_rising_workload_holds_back_an_upgrade_that_would_not_last() {
    let bids = oak_and_pine(6_000);

    let flat = Arbiter::default().decide(&round(Trigger::Refresh, 10_000, &bids));
    assert_eq!(planned_for(&flat.rows, "oak"), Some(HighPerformance));

    let rising = Arbiter::default().decide(&Round {
        planning: growing("oak", 1.5),
        ..round(Trigger::Refresh, 10_000, &bids)
    });
    assert_eq!(
        planned_for(&rising.rows, "oak"),
        Some(LowPower),
        "6 × 1.5 + 2 ms are over 10 ms"
    );

    let lasting = Arbiter::default().decide(&Round {
        planning: growing("oak", 1.2),
        ..round(Trigger::Refresh, 10_000, &bids)
    });
    assert_eq!(planned_for(&lasting.rows, "oak"), Some(HighPerformance));
    let lent = lent_to(&lasting.allocation, "oak").time.cpu;
    assert!(
        lent.abs_diff(us(7_700)) <= us(1),
        "7.2 ms + 0.8 × 0.8 ms × 7.2 / 9.2: {lent:?}"
    );

    let floors = Arbiter::default().decide(&Round {
        planning: growing("oak", 3.0),
        ..round(Trigger::Refresh, 5_000, &bids)
    });
    assert_eq!(floors.allocation.trigger, Trigger::Refresh);
    assert_eq!(planned_for(&floors.rows, "oak"), Some(LowPower));
}

/// **A falling workload does not cheapen a row.** A workload falling from
/// 100 to 80 over 2 s is expected, linear, at 30 in 5 s: a growth of 0.375.
/// Planned at it, `oak`'s 9 ms row still asks 9 ms beside `pine`'s 2 ms, and
/// is not taken in a 10 ms frame; its 6 ms row is taken, and lent what its
/// quote asks: 6 ms plus its part of 80 % of the 2 ms left, 7.2 ms.
#[test]
fn a_falling_workload_does_not_cheapen_a_row() {
    let falling = [
        (Duration::from_secs(0), 100.0),
        (Duration::from_secs(1), 90.0),
        (Duration::from_secs(2), 80.0),
    ];
    let g = growth(&falling, ComplexityClass::Linear, Duration::from_secs(5));
    assert!((g - 0.375).abs() < 1e-9, "30 / 80: {g}");

    let dear = oak_and_pine(9_000);
    let decision = Arbiter::default().decide(&Round {
        planning: growing("oak", g),
        ..round(Trigger::Refresh, 10_000, &dear)
    });
    assert_eq!(
        planned_for(&decision.rows, "oak"),
        Some(LowPower),
        "9 + 2 ms are over 10 ms, whatever the trend"
    );

    let fitting = oak_and_pine(6_000);
    let decision = Arbiter::default().decide(&Round {
        planning: growing("oak", g),
        ..round(Trigger::Refresh, 10_000, &fitting)
    });
    assert_eq!(planned_for(&decision.rows, "oak"), Some(HighPerformance));
    assert_eq!(
        lent_to(&decision.allocation, "oak").time.cpu,
        us(7_200),
        "6 ms + 0.8 × 2 ms × 6 / 8"
    );
}

/// **A growing row never lets another agent past the target.** `oak` is
/// pinned to its 8 ms row (floor 1 ms) and expected to grow by 2; `pine`, in
/// `oak`'s wave, offers 1 ms or 12 ms. In a 10 ms frame the wave asks
/// max(8, 12) = 12 ms with `pine`'s upgrade: over the target, so the round
/// without a trend refuses it. Planning for `oak`'s growth only plans rows
/// dearer: it must refuse at least as much, never plan `pine` past the
/// frame because `oak`'s grown row lifted the target the plans share.
#[test]
fn a_growing_row_never_lets_another_agent_past_the_target() {
    let mut bids = [
        bid(
            offer(
                "oak",
                vec![row(LowPower, 1_000, 0.4), row(HighPerformance, 8_000, 1.0)],
                LowPower,
            ),
            Critical,
        ),
        bid(
            offer(
                "pine",
                vec![row(LowPower, 1_000, 0.4), row(HighPerformance, 12_000, 1.0)],
                LowPower,
            ),
            Important,
        ),
    ];
    bids[0].mode = AdaptationMode::Manual(HighPerformance);
    let waves = [vec![AgentId::new("oak"), AgentId::new("pine")]];
    let flat = Round {
        waves: &waves,
        ..round(Trigger::Refresh, 10_000, &bids)
    };
    let unplanned = Arbiter::default().decide(&flat);
    assert_eq!(
        planned_for(&unplanned.rows, "pine"),
        Some(LowPower),
        "max(8, 12) ms are over 10 ms"
    );

    let rising = Arbiter::default().decide(&Round {
        planning: growing("oak", 2.0),
        ..flat.clone()
    });

    assert_eq!(
        planned_for(&rising.rows, "pine"),
        Some(LowPower),
        "`oak`'s growth plans `pine` at 12 ms in a 10 ms frame"
    );
    assert!(
        lent_to(&rising.allocation, "pine").time.cpu <= us(10_000),
        "`pine` is lent more than the frame: {:?}",
        lent_to(&rising.allocation, "pine").time.cpu
    );
}

/// **The growth reads the trend through the cost class.** A workload rising
/// from 100 by 10 a second, last at 120, is expected at 170 in 5 s: a linear
/// cost grows by 170 / 120, a quadratic one by its square, an `n log n` one
/// by f(170) / f(120), a constant one not at all. With fewer than two
/// samples, a flat trend, or a cost of nothing now, nothing grows; a
/// workload expected below zero grows to nothing, never less.
#[test]
fn the_growth_reads_the_trend_through_the_cost_class() {
    let horizon = Duration::from_secs(5);
    let rising = [
        (Duration::from_secs(0), 100.0),
        (Duration::from_secs(1), 110.0),
        (Duration::from_secs(2), 120.0),
    ];
    let close = |got: f64, want: f64| (got - want).abs() < 1e-9;

    let linear = growth(&rising, ComplexityClass::Linear, horizon);
    assert!(close(linear, 170.0 / 120.0), "{linear}");
    let quadratic = growth(&rising, ComplexityClass::Quadratic, horizon);
    assert!(close(quadratic, (170.0_f64 / 120.0).powi(2)), "{quadratic}");
    let linearithmic = growth(&rising, ComplexityClass::Linearithmic, horizon);
    let want = ComplexityClass::Linearithmic.f(170.0) / ComplexityClass::Linearithmic.f(120.0);
    assert!(close(linearithmic, want), "{linearithmic}");
    assert_eq!(growth(&rising, ComplexityClass::Constant, horizon), 1.0);

    assert_eq!(growth(&[], ComplexityClass::Linear, horizon), 1.0);
    assert_eq!(
        growth(&rising[..1], ComplexityClass::Linear, horizon),
        1.0,
        "one sample"
    );
    let flat = [
        (Duration::from_secs(0), 50.0),
        (Duration::from_secs(1), 50.0),
    ];
    assert_eq!(growth(&flat, ComplexityClass::Quadratic, horizon), 1.0);
    let from_nothing = [
        (Duration::from_secs(0), -10.0),
        (Duration::from_secs(1), 0.0),
    ];
    assert_eq!(
        growth(&from_nothing, ComplexityClass::Linear, horizon),
        1.0,
        "f(n(t)) = 0"
    );
    let collapsing = [
        (Duration::from_secs(0), 100.0),
        (Duration::from_secs(1), 50.0),
    ];
    assert_eq!(
        growth(&collapsing, ComplexityClass::Linear, horizon),
        0.0,
        "expected at −200: never negative"
    );
}

/// A planner of `cfg` that has read `oak`'s and `pine`'s reports: `oak`'s
/// workload falls for 14 reports, then rises linearly for 16, from 100 to
/// 250 a second apart, its cost quadratic; `pine`'s holds at 50, its cost
/// linear.
fn planner_of_two_trends(cfg: PlanningConfig) -> Planner {
    let (oak, pine) = (AgentId::new("oak"), AgentId::new("pine"));
    let mut planner = Planner::new(cfg);
    planner.observe_class(oak, ComplexityClass::Quadratic);
    planner.observe_class(pine, ComplexityClass::Linear);
    for second in 0..30_u64 {
        let n = if second < 14 {
            5_000.0 - 100.0 * second as f64
        } else {
            100.0 + 10.0 * (second - 14) as f64
        };
        let at = Duration::from_secs(second);
        planner.observe_report(&ran("oak", second, LowPower, None, Some(n)), at);
        planner.observe_report(&ran("pine", second, LowPower, None, Some(50.0)), at);
    }
    planner
}

/// `oak`'s and `pine`'s offers, each of two rows, running its floor.
fn two_offers() -> [Offer; 2] {
    [
        offer_running("oak", &[LowPower, HighPerformance], LowPower),
        offer_running("pine", &[LowPower, HighPerformance], LowPower),
    ]
}

/// **The planner reads each agent's growth from its trend and class**, when
/// it follows trends. `oak`'s trend is read over the 16 reports the config
/// names, from 100 to 250 a second apart: expected at 300 in 5 s, a
/// quadratic cost grows by (300 / 250)² = 1.44. `pine`'s holds at 50: it
/// grows by 1.
#[test]
fn the_planner_reads_each_agents_growth_from_its_trend_and_class() {
    let (oak, pine) = (AgentId::new("oak"), AgentId::new("pine"));
    let planner = planner_of_two_trends(PlanningConfig {
        follow_trends: true,
        ..PlanningConfig::default()
    });

    let planning = planner.planning(&two_offers(), Epoch(40));

    assert!(
        (planning.growth_of(oak) - 1.44).abs() < 1e-9,
        "{}",
        planning.growth_of(oak)
    );
    assert_eq!(planning.growth_of(pine), 1.0);
    assert_eq!(planning.horizon, Duration::from_secs(5));
}

/// **The planner follows no trend by default.** Fed the same rising
/// workload, a planner of the default settings names no growth: every agent
/// is planned at its quote.
#[test]
fn the_planner_follows_no_trend_by_default() {
    assert!(!PlanningConfig::default().follow_trends);
    let planner = planner_of_two_trends(PlanningConfig::default());

    let planning = planner.planning(&two_offers(), Epoch(40));

    assert!(planning.growth.is_empty(), "{:?}", planning.growth);
    assert_eq!(planning.growth_of(AgentId::new("oak")), 1.0);
}

// ── Reports and offers for the planner ────────────────────────────

/// `name`'s report of a run of `row` under `epoch`, carrying `switch` — the
/// preparation of `row`, on its first run — and `workload`.
fn ran(
    name: &'static str,
    epoch: u64,
    row: StrategyId,
    switch: Option<Duration>,
    workload: Option<f64>,
) -> Report {
    Report {
        agent: AgentId::new(name),
        epoch: Epoch(epoch),
        row,
        measured: PerDevice {
            cpu: ms(1),
            ..PerDevice::default()
        },
        done: 1.0,
        health: 1.0,
        over_budget: false,
        stalled: false,
        workload,
        switch,
        message: None,
    }
}

/// `name`'s offer of `rows`, 1 ms each, the first its floor, running
/// `current`.
fn offer_running(name: &'static str, rows: &[StrategyId], current: StrategyId) -> Offer {
    let mut offered = offer(
        name,
        rows.iter()
            .enumerate()
            .map(|(rank, id)| row(*id, 1_000, 0.25 * (rank + 1) as f32))
            .collect(),
        rows[0],
    );
    offered.current = current;
    offered
}
