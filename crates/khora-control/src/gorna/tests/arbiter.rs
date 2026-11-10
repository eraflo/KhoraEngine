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

//! The arbiter: its proposers run behind the shield, the best plan accepted
//! is kept, and the decision says why — what decided it, the trigger, the
//! constraint that bound it and the upgrades it refused. Every figure is in
//! whole microseconds.

use super::decide::{bid, round};
use super::*;
use crate::gorna::arbiter::{Arbiter, Greedy, Proposal, Proposer, Refusal, Shield};
use crate::gorna::decide::Plan;
use khora_core::agent::gorna::protocol::{Binding, DecidedBy, Rejected, Trigger};
use khora_core::lane::cost::Device;

use AgentImportance::{Critical, Important, Optional};
use StrategyId::{Balanced, HighPerformance, LowPower};

/// What the shield said of each plan a [`Fixed`] proposed, in order.
type Verdicts = Arc<Mutex<Vec<Result<(), Refusal>>>>;

/// A proposer that plans fixed rows: the row it names for an agent, its
/// floor for every other. It asks the shield of its own plan, and records the
/// answer.
struct Fixed {
    by: DecidedBy,
    rows: Vec<(AgentId, StrategyId)>,
    verdicts: Verdicts,
}

/// A [`Fixed`] proposer named `by`, planning `rows`, and what the shield said
/// of its plans.
fn fixed(by: DecidedBy, rows: &[(&'static str, StrategyId)]) -> (Box<dyn Proposer>, Verdicts) {
    let verdicts = Verdicts::default();
    let proposer = Fixed {
        by,
        rows: rows
            .iter()
            .map(|(name, row)| (AgentId::new(name), *row))
            .collect(),
        verdicts: Arc::clone(&verdicts),
    };
    (Box::new(proposer), verdicts)
}

impl Proposer for Fixed {
    fn name(&self) -> DecidedBy {
        self.by
    }

    fn propose(&self, round: &Round<'_>, bids: &[&Bid], base: &[Plan]) -> Proposal {
        let proposal = Proposal {
            rows: bids
                .iter()
                .map(|bid| {
                    let wanted = self
                        .rows
                        .iter()
                        .find(|(agent, _)| *agent == bid.offer.agent)
                        .map_or(bid.offer.floor, |(_, row)| *row);
                    bid.offer.rows.iter().position(|row| row.id == wanted)
                })
                .collect(),
            price: None,
            rejected: Vec::new(),
        };
        let verdict = Shield::check(round, bids, base, &proposal);
        self.verdicts
            .lock()
            .expect("the verdicts lock")
            .push(verdict);
        proposal
    }
}

/// What the shield said of the plans one [`Fixed`] proposed.
fn said(verdicts: &Verdicts) -> Vec<Result<(), Refusal>> {
    verdicts.lock().expect("the verdicts lock").clone()
}

/// A row of `gpu_us` on the graphics processor and 500 µs on the host, of
/// `quality`.
fn on_gpu(id: StrategyId, gpu_us: u64, quality: f32) -> OfferRow {
    let mut row = row(id, 500, quality);
    row.cost.time.gpu = us(gpu_us);
    row
}

// ── What decided, and why ─────────────────────────────────────────

/// **A decision names its proposer and its trigger.** An ordinary round,
/// opened for the periodic refresh, whose floors fit, decided by the Q-RAM
/// pass alone: the decision says so and keeps the round's trigger. The
/// default arbiter lends what `decide` lends.
#[test]
fn a_decision_names_its_proposer_and_trigger() {
    let bids = [
        bid(
            offer(
                "oak",
                vec![row(LowPower, 1_000, 0.4), row(HighPerformance, 4_000, 1.0)],
                LowPower,
            ),
            Critical,
        ),
        bid(
            offer(
                "pine",
                vec![row(LowPower, 1_000, 0.4), row(HighPerformance, 1_500, 1.0)],
                LowPower,
            ),
            Optional,
        ),
    ];
    let round = round(Trigger::Refresh, 5_000, &bids);

    let decision = Arbiter::with(vec![Box::new(Greedy)]).decide(&round);

    assert_eq!(decision.why.by, DecidedBy::Greedy, "{:?}", decision.why);
    assert_eq!(decision.why.trigger, Trigger::Refresh, "{:?}", decision.why);
    assert_eq!(decision.allocation.trigger, Trigger::Refresh);
    assert_eq!(
        Arbiter::default().decide(&round).allocation,
        decide(&round),
        "the default arbiter lends what `decide` lends"
    );
}

/// **Floors that do not fit are decided by the floors.** Floors of 6 + 5 ms
/// exceed a 10 ms frame on the host: the round becomes `Safety`, every agent
/// is lent its floor, and the decision names the floors and the host's time
/// as what they broke. Floors that fit the time but hold 12 MB of a graphics
/// processor's 10 MB break its memory instead. A round opened as `Safety`
/// broke nothing: the trigger, not a constraint, gave every agent its floor.
#[test]
fn floors_that_do_not_fit_are_decided_by_the_floors() {
    const MB: u64 = 1_000_000;
    let over_time = [
        bid(
            offer(
                "ash",
                vec![row(LowPower, 6_000, 0.4), row(HighPerformance, 9_000, 1.0)],
                LowPower,
            ),
            Important,
        ),
        bid(
            offer("birch", vec![row(LowPower, 5_000, 0.4)], LowPower),
            Important,
        ),
    ];
    let decision = Arbiter::default().decide(&round(Trigger::Pressure, 10_000, &over_time));
    assert_eq!(decision.allocation.trigger, Trigger::Safety);
    assert_eq!(decision.why.by, DecidedBy::Floors, "{:?}", decision.why);
    assert_eq!(decision.why.trigger, Trigger::Safety, "{:?}", decision.why);
    assert_eq!(
        decision.why.binding,
        Some(Binding::Time(Device::Cpu)),
        "6 + 5 ms exceed 10 ms on the host"
    );
    assert!(decision.why.rejected.is_empty(), "{:?}", decision.why);

    let over_memory = [
        bid(
            offer(
                "ash",
                vec![row_holding(LowPower, 1_000, 7 * MB, 0.4)],
                LowPower,
            ),
            Important,
        ),
        bid(
            offer(
                "birch",
                vec![row_holding(LowPower, 1_000, 5 * MB, 0.4)],
                LowPower,
            ),
            Important,
        ),
    ];
    let decision = Arbiter::default().decide(&Round {
        memory: PerDevice {
            gpu: Some(10 * MB),
            ..PerDevice::default()
        },
        ..round(Trigger::Pressure, 16_000, &over_memory)
    });
    assert_eq!(decision.why.by, DecidedBy::Floors, "{:?}", decision.why);
    assert_eq!(decision.why.trigger, Trigger::Safety, "{:?}", decision.why);
    assert_eq!(
        decision.why.binding,
        Some(Binding::Memory(Device::Gpu)),
        "7 + 5 MB exceed the 10 MB the graphics processor has"
    );

    let fitting = [bid(
        offer(
            "ash",
            vec![row(LowPower, 1_000, 0.4), row(HighPerformance, 3_000, 1.0)],
            LowPower,
        ),
        Important,
    )];
    let decision = Arbiter::default().decide(&round(Trigger::Safety, 100_000, &fitting));
    assert_eq!(decision.why.by, DecidedBy::Floors, "{:?}", decision.why);
    assert_eq!(decision.why.trigger, Trigger::Safety, "{:?}", decision.why);
    assert_eq!(
        decision.why.binding, None,
        "the trigger gave every agent its floor, no constraint did"
    );
    assert!(decision.why.rejected.is_empty(), "{:?}", decision.why);
}

/// **The why names the constraint that stopped an upgrade.** `ash` runs on
/// the graphics processor: its floor takes 2 ms there, `Balanced` 6 ms and
/// `HighPerformance` 12 ms. In a 10 ms frame the Q-RAM pass takes `Balanced`
/// and refuses `HighPerformance` for the graphics processor's time; the
/// decision names that constraint and that upgrade. In a 20 ms frame every
/// upgrade fits: nothing binds, nothing is refused.
#[test]
fn the_why_names_the_constraint_that_stopped_an_upgrade() {
    let bids = [bid(
        offer(
            "ash",
            vec![
                on_gpu(LowPower, 2_000, 0.4),
                on_gpu(Balanced, 6_000, 0.7),
                on_gpu(HighPerformance, 12_000, 1.0),
            ],
            LowPower,
        ),
        Important,
    )];

    let decision = Arbiter::default().decide(&round(Trigger::Refresh, 10_000, &bids));
    assert_eq!(decision.why.by, DecidedBy::Greedy, "{:?}", decision.why);
    assert_eq!(
        decision.why.binding,
        Some(Binding::Time(Device::Gpu)),
        "12 ms exceed 10 ms on the graphics processor"
    );
    assert_eq!(
        decision.why.rejected,
        vec![Rejected {
            agent: AgentId::new("ash"),
            row: HighPerformance,
            over: Binding::Time(Device::Gpu),
        }]
    );
    let ash = lent_to(&decision.allocation, "ash");
    assert!(fits(&bids[0].offer.rows[1].cost, &ash), "{ash:?}");
    assert!(!fits(&bids[0].offer.rows[2].cost, &ash), "{ash:?}");

    let decision = Arbiter::default().decide(&round(Trigger::Refresh, 20_000, &bids));
    assert_eq!(decision.why.by, DecidedBy::Greedy, "{:?}", decision.why);
    assert_eq!(
        decision.why.binding, None,
        "ash reached its best row: {:?}",
        decision.why
    );
    assert!(decision.why.rejected.is_empty(), "{:?}", decision.why);
}

// ── The shield ────────────────────────────────────────────────────

/// **The shield refuses a proposal over the target.** A proposer plans
/// `ash`'s 9 ms row beside `birch`'s 2 ms floor in a 10 ms frame: the shield
/// refuses it for the host's time. Alone, it leaves the round to the floors,
/// and `ash` is not lent room for its 9 ms row; followed by the Q-RAM pass,
/// that pass decides, as `decide` would.
#[test]
fn the_shield_refuses_a_proposal_over_the_target() {
    let bids = [
        bid(
            offer(
                "ash",
                vec![row(LowPower, 2_000, 0.4), row(HighPerformance, 9_000, 1.0)],
                LowPower,
            ),
            Important,
        ),
        bid(
            offer("birch", vec![row(LowPower, 2_000, 0.4)], LowPower),
            Important,
        ),
    ];
    let round = round(Trigger::Refresh, 10_000, &bids);

    let (over, verdicts) = fixed(DecidedBy::Exact, &[("ash", HighPerformance)]);
    let decision = Arbiter::with(vec![over]).decide(&round);
    assert_eq!(
        said(&verdicts),
        vec![Err(Refusal::Breach(Binding::Time(Device::Cpu)))],
        "9 + 2 ms exceed 10 ms on the host"
    );
    assert_eq!(decision.why.by, DecidedBy::Floors, "{:?}", decision.why);
    let ash = lent_to(&decision.allocation, "ash");
    assert!(!fits(&bids[0].offer.rows[1].cost, &ash), "{ash:?}");

    let (over, verdicts) = fixed(DecidedBy::Exact, &[("ash", HighPerformance)]);
    let decision = Arbiter::with(vec![over, Box::new(Greedy)]).decide(&round);
    assert_eq!(
        said(&verdicts),
        vec![Err(Refusal::Breach(Binding::Time(Device::Cpu)))]
    );
    assert_eq!(decision.why.by, DecidedBy::Greedy, "{:?}", decision.why);
    assert_eq!(decision.allocation, decide(&round));
}

/// **The shield refuses a ruled-out row.** `ash` is pinned (`Manual`) to its
/// floor: a proposal moving it to `HighPerformance` is refused, one leaving
/// it on its pinned row is accepted and decides. `birch` may run `LowPower`
/// to `Balanced` (`Bounded`): a proposal moving it to `HighPerformance` is
/// refused, and the round falls to the floors; one moving it to `Balanced` is
/// accepted.
#[test]
fn the_shield_refuses_a_ruled_out_row() {
    let mut pinned = bid(
        offer(
            "ash",
            vec![row(LowPower, 1_000, 0.4), row(HighPerformance, 3_000, 1.0)],
            LowPower,
        ),
        Important,
    );
    pinned.mode = AdaptationMode::Manual(LowPower);
    let bids = [
        pinned,
        bid(
            offer("birch", vec![row(LowPower, 1_000, 0.4)], LowPower),
            Important,
        ),
    ];
    let round_pinned = round(Trigger::Refresh, 16_000, &bids);
    let (moving, moved) = fixed(DecidedBy::Exact, &[("ash", HighPerformance)]);
    let (keeping, kept) = fixed(DecidedBy::Planner, &[("ash", LowPower)]);
    let decision = Arbiter::with(vec![moving, keeping]).decide(&round_pinned);
    assert_eq!(
        said(&moved),
        vec![Err(Refusal::RuledOut {
            agent: AgentId::new("ash"),
            row: HighPerformance,
        })],
        "ash is pinned to LowPower"
    );
    assert_eq!(said(&kept), vec![Ok(())], "its pinned row is allowed");
    assert_eq!(decision.why.by, DecidedBy::Planner, "{:?}", decision.why);
    assert_eq!(
        decision.allocation.pins,
        vec![(AgentId::new("ash"), LowPower)]
    );

    let mut bounded = bid(
        offer(
            "birch",
            vec![
                row(LowPower, 1_000, 0.4),
                row(Balanced, 2_000, 0.7),
                row(HighPerformance, 3_000, 1.0),
            ],
            LowPower,
        ),
        Important,
    );
    bounded.mode = AdaptationMode::Bounded {
        min: LowPower,
        max: Balanced,
    };
    let bids = [bounded];
    let round_bounded = round(Trigger::Refresh, 16_000, &bids);

    let (outside, verdicts) = fixed(DecidedBy::Exact, &[("birch", HighPerformance)]);
    let decision = Arbiter::with(vec![outside]).decide(&round_bounded);
    assert_eq!(
        said(&verdicts),
        vec![Err(Refusal::RuledOut {
            agent: AgentId::new("birch"),
            row: HighPerformance,
        })],
        "birch may run LowPower to Balanced"
    );
    assert_eq!(decision.why.by, DecidedBy::Floors, "{:?}", decision.why);

    let (inside, verdicts) = fixed(DecidedBy::Exact, &[("birch", Balanced)]);
    let decision = Arbiter::with(vec![inside]).decide(&round_bounded);
    assert_eq!(said(&verdicts), vec![Ok(())], "Balanced is in range");
    assert_eq!(decision.why.by, DecidedBy::Exact, "{:?}", decision.why);
}

// ── The best plan accepted ────────────────────────────────────────

/// **The arbiter keeps the best feasible proposal.** Both plans fit the
/// 16 ms frame. One plans `ash` at `Balanced` and `birch` at its floor
/// (0.75 × (0.7 + 0.4) weighed quality), the other both at `HighPerformance`
/// (0.75 × 2.0): the second is kept, and named, whichever order the arbiter
/// asks them in.
#[test]
fn the_arbiter_keeps_the_best_feasible_proposal() {
    let bids = [
        bid(
            offer(
                "ash",
                vec![
                    row(LowPower, 1_000, 0.4),
                    row(Balanced, 3_000, 0.7),
                    row(HighPerformance, 6_000, 1.0),
                ],
                LowPower,
            ),
            Important,
        ),
        bid(
            offer(
                "birch",
                vec![row(LowPower, 1_000, 0.4), row(HighPerformance, 3_000, 1.0)],
                LowPower,
            ),
            Important,
        ),
    ];
    let round = round(Trigger::Refresh, 16_000, &bids);
    let lesser = || fixed(DecidedBy::Exact, &[("ash", Balanced)]);
    let better = || {
        fixed(
            DecidedBy::Planner,
            &[("ash", HighPerformance), ("birch", HighPerformance)],
        )
    };

    for (first, second) in [(lesser(), better()), (better(), lesser())] {
        let (first, first_said) = first;
        let (second, second_said) = second;
        let decision = Arbiter::with(vec![first, second]).decide(&round);

        assert_eq!(said(&first_said), vec![Ok(())], "both plans fit");
        assert_eq!(said(&second_said), vec![Ok(())], "both plans fit");
        assert_eq!(decision.why.by, DecidedBy::Planner, "{:?}", decision.why);
        let ash = lent_to(&decision.allocation, "ash");
        let birch = lent_to(&decision.allocation, "birch");
        assert!(fits(&bids[0].offer.rows[2].cost, &ash), "{ash:?}");
        assert!(fits(&bids[1].offer.rows[1].cost, &birch), "{birch:?}");
    }
}

/// A proposer that plans the rows it names by agent; an agent it names no
/// row for is given `None`, which keeps its base row.
struct Naming {
    by: DecidedBy,
    rows: Vec<(AgentId, StrategyId)>,
}

impl Proposer for Naming {
    fn name(&self) -> DecidedBy {
        self.by
    }

    fn propose(&self, _: &Round<'_>, bids: &[&Bid], _: &[Plan]) -> Proposal {
        Proposal {
            rows: bids
                .iter()
                .map(|bid| {
                    let named = self
                        .rows
                        .iter()
                        .find(|(agent, _)| *agent == bid.offer.agent);
                    named.and_then(|(_, id)| bid.offer.rows.iter().position(|row| row.id == *id))
                })
                .collect(),
            price: None,
            rejected: Vec::new(),
        }
    }
}

/// **A proposal is weighed by the rows it lends.** A proposal that names no
/// row for `birch` keeps it on its base row, as the shield and the
/// allocation read it: it lends `ash` `HighPerformance` and `birch` its
/// floor, 0.75 × (1.0 + 0.4) weighed quality. Another lends `ash` `Balanced`
/// and `birch` its floor, 0.75 × (0.7 + 0.4). Both fit; the first lends
/// more weighed quality and is kept, whichever order they are asked in.
#[test]
fn a_proposal_is_weighed_by_the_rows_it_lends() {
    let bids = [
        bid(
            offer(
                "ash",
                vec![
                    row(LowPower, 1_000, 0.4),
                    row(Balanced, 2_000, 0.7),
                    row(HighPerformance, 3_000, 1.0),
                ],
                LowPower,
            ),
            Important,
        ),
        bid(
            offer(
                "birch",
                vec![row(LowPower, 1_000, 0.4), row(HighPerformance, 3_000, 1.0)],
                LowPower,
            ),
            Important,
        ),
    ];
    let round = round(Trigger::Refresh, 16_000, &bids);
    let better = || -> Box<dyn Proposer> {
        Box::new(Naming {
            by: DecidedBy::Exact,
            rows: vec![(AgentId::new("ash"), HighPerformance)],
        })
    };
    let lesser = || -> Box<dyn Proposer> {
        Box::new(Naming {
            by: DecidedBy::Planner,
            rows: vec![
                (AgentId::new("ash"), Balanced),
                (AgentId::new("birch"), LowPower),
            ],
        })
    };

    for proposers in [vec![better(), lesser()], vec![lesser(), better()]] {
        let decision = Arbiter::with(proposers).decide(&round);
        assert_eq!(
            decision.why.by,
            DecidedBy::Exact,
            "ash at HighPerformance and birch at its floor weigh more: {:?}",
            decision.why
        );
        let ash = lent_to(&decision.allocation, "ash");
        assert!(fits(&bids[0].offer.rows[2].cost, &ash), "{ash:?}");
    }
}

/// **A `Stable` ceiling is not named as the frame's time.** `ash` keeps a
/// 0.5 ms envelope, smaller than its 1 ms floor: it may not grow past it, so
/// its 3 ms `HighPerformance` is refused. The frame is 100 ms and the plan
/// asks 1 ms of it: the host's time on the critical path did not refuse the
/// upgrade, and the decision must not say it did.
#[test]
fn a_stable_ceiling_is_not_named_as_the_frames_time() {
    let mut stable = bid(
        offer(
            "ash",
            vec![row(LowPower, 1_000, 0.4), row(HighPerformance, 3_000, 1.0)],
            LowPower,
        ),
        Important,
    );
    stable.mode = AdaptationMode::Stable;
    stable.kept = Some(cpu_envelope(500));
    let bids = [stable];

    let decision = Arbiter::default().decide(&round(Trigger::Refresh, 100_000, &bids));

    assert_eq!(decision.why.by, DecidedBy::Greedy, "{:?}", decision.why);
    assert!(
        decision
            .why
            .rejected
            .iter()
            .all(|rejected| rejected.over != Binding::Time(Device::Cpu)),
        "1 ms of a 100 ms frame breaks no time on the host: {:?}",
        decision.why
    );
    assert_ne!(
        decision.why.binding,
        Some(Binding::Time(Device::Cpu)),
        "1 ms of a 100 ms frame breaks no time on the host: {:?}",
        decision.why
    );
}
