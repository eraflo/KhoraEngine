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

//! The exact proposer: a branch-and-bound over each agent's rows, started
//! from the Q-RAM pass's plan and bounded by a node budget. It finds the plan
//! of most weighed quality the shield accepts, never lends less than the
//! Q-RAM pass, keeps every policy, says what stops each agent growing, and
//! leaves a tie to the Q-RAM pass. Its plans are checked against a
//! brute-force enumeration of every row of every agent, over rounds drawn by
//! a seeded generator. Every figure is in whole microseconds.

use super::decide::{bid, round};
use super::*;
use crate::gorna::arbiter::{planned, Arbiter, Exact, Greedy, Proposal, Proposer, Shield};
use crate::gorna::decide::{admit, weight, Admission, Plan};
use khora_core::agent::gorna::protocol::{Binding, DecidedBy, Rejected, Trigger};
use khora_core::lane::cost::{Device, PerLink};

use AgentImportance::{Critical, Important, Optional};
use StrategyId::{Balanced, Custom, HighPerformance, LowPower};

/// A node budget no round of these tests exhausts.
const AMPLE: u32 = 1_000_000;

/// Two worths closer than this are equal: qualities are `f32`, read as `f64`.
const EPS: f64 = 1e-6;

/// The agents a generated round names, in order.
const NAMES: [&str; 10] = [
    "ash", "birch", "cedar", "elm", "fir", "hazel", "larch", "maple", "oak", "pine",
];

/// The rows a generated offer names, in the order a `Bounded` range ranks
/// them.
const IDS: [StrategyId; 4] = [LowPower, Balanced, HighPerformance, Custom(1)];

// ── Reading a plan ────────────────────────────────────────────────

/// The bids and the plans admission gives `round`; panics when its floors do
/// not fit.
fn admitted<'r>(round: &'r Round<'_>) -> (Vec<&'r Bid>, Vec<Plan>) {
    match admit(round) {
        Admission::Admitted { bids, plans } => (bids, plans),
        Admission::Safety(allocation, _) => panic!("the floors fit: {allocation:?}"),
    }
}

/// The weighed quality `proposal` lends: each agent's weight times the
/// quality of the row it plans, its base row where the proposal names none.
fn worth(bids: &[&Bid], base: &[Plan], proposal: &Proposal) -> f64 {
    bids.iter()
        .zip(planned(base, proposal))
        .filter_map(|(bid, plan)| {
            plan.row
                .map(|row| weight(bid) * f64::from(bid.offer.rows[row].quality))
        })
        .sum()
}

/// The plan of most weighed quality the shield accepts, and its worth: every
/// row of every agent, and its base row, enumerated. The first found is kept
/// on a tie.
fn optimum(round: &Round<'_>, bids: &[&Bid], base: &[Plan]) -> (Proposal, f64) {
    let choices: Vec<Vec<Option<usize>>> = bids
        .iter()
        .map(|bid| {
            std::iter::once(None)
                .chain((0..bid.offer.rows.len()).map(Some))
                .collect()
        })
        .collect();
    let mut index = vec![0_usize; bids.len()];
    let mut best: Option<(Proposal, f64)> = None;
    loop {
        let proposal = Proposal {
            rows: index
                .iter()
                .zip(&choices)
                .map(|(chosen, choices)| choices[*chosen])
                .collect(),
            price: None,
            rejected: Vec::new(),
        };
        if Shield::check(round, bids, base, &proposal).is_ok() {
            let value = worth(bids, base, &proposal);
            if best.as_ref().is_none_or(|(_, kept)| value > *kept + EPS) {
                best = Some((proposal, value));
            }
        }
        // The next combination, the first agent's row turning fastest.
        let mut agent = 0;
        loop {
            if agent == index.len() {
                return best.expect("admission's own plan is accepted");
            }
            index[agent] += 1;
            if index[agent] < choices[agent].len() {
                break;
            }
            index[agent] = 0;
            agent += 1;
        }
    }
}

/// The row the plans `proposal` lends hold for each agent, by id.
fn rows_of(
    bids: &[&Bid],
    base: &[Plan],
    proposal: &Proposal,
) -> Vec<(AgentId, Option<StrategyId>)> {
    bids.iter()
        .zip(planned(base, proposal))
        .map(|(bid, plan)| (bid.offer.agent, plan.row.map(|row| bid.offer.rows[row].id)))
        .collect()
}

/// The row `rows` hold for `name`.
fn row_for(rows: &[(AgentId, Option<StrategyId>)], name: &'static str) -> Option<StrategyId> {
    rows.iter()
        .find(|(agent, _)| *agent == AgentId::new(name))
        .and_then(|(_, row)| *row)
}

/// What the exact search with an ample budget plans for `round`, by agent,
/// once the shield has accepted it.
fn exact_rows(round: &Round<'_>) -> Vec<(AgentId, Option<StrategyId>)> {
    let (bids, base) = admitted(round);
    let proposal = Exact { node_budget: AMPLE }.propose(round, &bids, &base);
    assert_eq!(
        Shield::check(round, &bids, &base, &proposal),
        Ok(()),
        "the shield accepts the exact plan {proposal:?}"
    );
    rows_of(&bids, &base, &proposal)
}

// ── A seeded generator of rounds ──────────────────────────────────

/// A xorshift generator: the same seed draws the same rounds.
struct Rng(u64);

impl Rng {
    /// A number in `0..below`.
    fn below(&mut self, below: u64) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0 % below
    }

    /// Whether a draw of `percent` in a hundred comes up.
    fn chance(&mut self, percent: u64) -> bool {
        self.below(100) < percent
    }

    /// A number in `0..below`, as an index.
    fn index(&mut self, below: usize) -> usize {
        self.below(below as u64) as usize
    }
}

/// A generated round: its bids, the scheduler's waves, the agents at the
/// background, the frame's target and the graphics processor's memory.
struct Case {
    bids: Vec<Bid>,
    waves: Vec<Vec<AgentId>>,
    background: Vec<AgentId>,
    target_us: u64,
    gpu_memory: Option<u64>,
}

impl Case {
    fn round(&self) -> Round<'_> {
        Round {
            waves: &self.waves,
            background: &self.background,
            memory: PerDevice {
                gpu: self.gpu_memory,
                ..PerDevice::default()
            },
            ..round(Trigger::Refresh, self.target_us, &self.bids)
        }
    }
}

/// A round of 1 to `max_agents` agents, each offering 1 to `max_rows` rows
/// of host time, sometimes graphics time, graphics memory and a quality
/// unrelated to the cost; a floor that is usually the first row; a policy
/// (`Manual`, `Bounded`, a cap, `Stable` with an envelope kept, late, or
/// none) and sometimes a priority; waves, agents at the background and a
/// memory limit.
fn generate(rng: &mut Rng, max_agents: usize, max_rows: usize) -> Case {
    let agents = 1 + rng.index(max_agents);
    let importances = [Critical, Important, Optional];
    let mut bids = Vec::with_capacity(agents);
    for name in NAMES.iter().take(agents) {
        let count = 1 + rng.index(max_rows);
        let mut rows = Vec::with_capacity(count);
        for (index, id) in IDS.iter().take(count).enumerate() {
            let quality = (1 + rng.below(10)) as f32 / 10.0;
            let mut offered = row(*id, 200 + rng.below(4_000), quality);
            if rng.chance(30) {
                offered.cost.time.gpu = us(rng.below(3_000));
            }
            offered.cost.memory.gpu = rng.below(8) * MIB;
            if index > 0 && rng.chance(8) {
                offered.confidence = Confidence::Assumed;
            }
            rows.push(offered);
        }
        let floor = if rng.chance(80) {
            rows[0].id
        } else {
            rows[rng.index(count)].id
        };
        let mut bid = bid(offer(name, rows, floor), importances[rng.index(3)]);
        if rng.chance(15) {
            bid.hints.priority = Some((1 + rng.below(20)) as f32 / 10.0);
        }
        match rng.below(10) {
            0 => bid.mode = AdaptationMode::Manual(bid.offer.rows[rng.index(count)].id),
            1 => {
                let (a, b) = (rng.index(IDS.len()), rng.index(IDS.len()));
                bid.mode = AdaptationMode::Bounded {
                    min: IDS[a.min(b)],
                    max: IDS[a.max(b)],
                };
            }
            2 => bid.hints.cap_ms = Some(0.5 + rng.below(5_000) as f32 / 1_000.0),
            3 => {
                bid.mode = AdaptationMode::Stable;
                let mut kept = cpu_envelope(300 + rng.below(6_000));
                kept.memory.gpu = rng.below(10) * MIB;
                bid.kept = Some(kept);
            }
            4 => {
                bid.late = true;
                bid.kept = Some(cpu_envelope(300 + rng.below(4_000)));
            }
            _ => {}
        }
        bids.push(bid);
    }
    let mut waves = vec![Vec::new(), Vec::new()];
    let mut background = Vec::new();
    for bid in &bids {
        if rng.chance(10) {
            background.push(bid.offer.agent);
        } else if rng.chance(40) {
            waves[rng.index(2)].push(bid.offer.agent);
        }
    }
    waves.retain(|wave| !wave.is_empty());
    let gpu_memory = rng.chance(50).then(|| (4 + rng.below(20)) * MIB);
    Case {
        bids,
        waves,
        background,
        target_us: 2_000 + rng.below(14_000),
        gpu_memory,
    }
}

// ── The optimum ───────────────────────────────────────────────────

/// The knapsack the Q-RAM pass loses. In a 5 ms frame, `oak` (Critical)
/// offers 1 ms at quality 0.4 or 3.9 ms at 1.0; `pine` (Optional) 1 ms at
/// 0.4 or 1.5 ms at 1.0. `pine`'s upgrade gains 0.5 × 0.6 over 0.5 ms
/// (0.0006 per µs), `oak`'s 1.0 × 0.6 over 2.9 ms (0.0002 per µs): the Q-RAM
/// pass takes `pine`'s first, and then `oak`'s no longer fits (3.9 + 1.5 ms).
/// It lends 1.0 × 0.4 + 0.5 × 1.0 = 0.9. `oak` upgraded alone fits
/// (3.9 + 1 ms) and lends 1.0 × 1.0 + 0.5 × 0.4 = 1.2, the optimum.
#[test]
fn exact_beats_greedy_on_a_knapsack_counterexample() {
    let bids = [
        bid(
            offer(
                "oak",
                vec![row(LowPower, 1_000, 0.4), row(HighPerformance, 3_900, 1.0)],
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

    // The claim, checked by enumeration and against the Q-RAM pass.
    let (admitted_bids, base) = admitted(&round);
    let (best, best_worth) = optimum(&round, &admitted_bids, &base);
    assert!(
        (best_worth - 1.2).abs() < EPS,
        "the optimum lends 1.2: {best_worth}"
    );
    assert_eq!(
        rows_of(&admitted_bids, &base, &best),
        vec![
            (AgentId::new("oak"), Some(HighPerformance)),
            (AgentId::new("pine"), Some(LowPower)),
        ]
    );
    let greedy_plan = Greedy.propose(&round, &admitted_bids, &base);
    let greedy_worth = worth(&admitted_bids, &base, &greedy_plan);
    assert!(
        (greedy_worth - 0.9).abs() < EPS,
        "the Q-RAM pass lends 0.9: {greedy_worth}"
    );

    // The Q-RAM pass alone upgrades `pine`.
    let greedy = Arbiter::with(vec![Box::new(Greedy)]).decide(&round);
    assert_eq!(greedy.why.by, DecidedBy::Greedy, "{:?}", greedy.why);
    assert!(fits(
        &bids[1].offer.rows[1].cost,
        &lent_to(&greedy.allocation, "pine")
    ));
    assert!(!fits(
        &bids[0].offer.rows[1].cost,
        &lent_to(&greedy.allocation, "oak")
    ));

    // The default arbiter keeps the exact plan: `oak` upgraded, `pine` not.
    let decision = Arbiter::default().decide(&round);
    assert_eq!(
        decision.why.by,
        DecidedBy::Exact,
        "the exact plan lends more weighed quality: {:?}",
        decision.why
    );
    let oak = lent_to(&decision.allocation, "oak");
    let pine = lent_to(&decision.allocation, "pine");
    assert!(
        fits(&bids[0].offer.rows[1].cost, &oak),
        "oak upgraded: {oak:?}"
    );
    assert!(
        !fits(&bids[1].offer.rows[1].cost, &pine),
        "pine stays: {pine:?}"
    );
}

/// **The exact search finds the optimum of small rounds.** Over 400 rounds
/// of at most 4 agents × 4 rows, drawn with policies, waves, agents at the
/// background and memory limits: with an ample budget, the exact plan is
/// accepted by the shield and lends the weighed quality of the brute-force
/// optimum. The generator is first checked to draw rounds the Q-RAM pass
/// does not solve.
#[test]
fn exact_finds_the_optimum_of_small_rounds() {
    let mut rng = Rng(0x5eed_0e8a_c7b1_0001);
    let cases: Vec<Case> = (0..400).map(|_| generate(&mut rng, 4, 4)).collect();

    let (mut admitted_rounds, mut greedy_short) = (0, 0);
    for case in &cases {
        let round = case.round();
        let Admission::Admitted { bids, plans } = admit(&round) else {
            continue;
        };
        admitted_rounds += 1;
        let (_, best) = optimum(&round, &bids, &plans);
        let greedy = Greedy.propose(&round, &bids, &plans);
        if worth(&bids, &plans, &greedy) + EPS < best {
            greedy_short += 1;
        }
    }
    assert!(
        admitted_rounds >= 150,
        "the generator draws rounds whose floors fit: {admitted_rounds} of 400"
    );
    assert!(
        greedy_short > 0,
        "the generator draws rounds the Q-RAM pass does not solve"
    );

    for (number, case) in cases.iter().enumerate() {
        let round = case.round();
        let Admission::Admitted { bids, plans } = admit(&round) else {
            continue;
        };
        let (best, best_worth) = optimum(&round, &bids, &plans);
        let found = Exact { node_budget: AMPLE }.propose(&round, &bids, &plans);
        assert_eq!(
            Shield::check(&round, &bids, &plans, &found),
            Ok(()),
            "round {number}: the exact plan {found:?} is refused"
        );
        let found_worth = worth(&bids, &plans, &found);
        assert!(
            (found_worth - best_worth).abs() <= EPS,
            "round {number}: the exact plan lends {found_worth}, the optimum {best_worth} \
             ({:?} against {:?})",
            rows_of(&bids, &plans, &found),
            rows_of(&bids, &plans, &best)
        );
    }
}

// ── The optimum of the planned value ──────────────────────────────

/// A generated round planned beyond its frame.
struct PlannedCase {
    case: Case,
    planning: Planning,
}

impl PlannedCase {
    fn round(&self) -> Round<'_> {
        Round {
            planning: self.planning.clone(),
            ..self.case.round()
        }
    }
}

/// A round of [`generate`]'s, planned: each agent free to choose its row
/// (no pin, no kept envelope) runs one of its rows, and a switch to each of
/// its other rows costs, more often than not, a preparation of up to 30 ms;
/// half the rounds keep a margin of up to 3 ms free of the target; some
/// agents are expected to grow by 0.5 to 2. Plans hold 5 s, with a 250 ms
/// transient.
fn generate_planned(rng: &mut Rng, max_agents: usize, max_rows: usize) -> PlannedCase {
    let mut case = generate(rng, max_agents, max_rows);
    let mut planning = Planning {
        horizon: Duration::from_secs(5),
        transient: ms(250),
        ..Planning::default()
    };
    for bid in &mut case.bids {
        let agent = bid.offer.agent;
        let chooses =
            !bid.late && !matches!(bid.mode, AdaptationMode::Manual(_) | AdaptationMode::Stable);
        if chooses {
            let count = bid.offer.rows.len();
            bid.offer.current = bid.offer.rows[rng.index(count)].id;
            for offered in &bid.offer.rows {
                if offered.id != bid.offer.current && rng.chance(60) {
                    planning
                        .switch
                        .push((agent, offered.id, us(rng.below(30_000))));
                }
            }
        }
        if rng.chance(30) {
            planning
                .growth
                .push((agent, (50 + rng.below(151)) as f64 / 100.0));
        }
    }
    if rng.chance(50) {
        planning.margin = us(rng.below(3_000));
    }
    PlannedCase { case, planning }
}

/// The value `proposal` plans for `round`: each agent's weight times the
/// quality of the row it plans, less, for a row other than the one its
/// offer runs whose switch the planning prices, its weight times the
/// switch's preparation and transient over the horizon.
fn planned_value(round: &Round<'_>, bids: &[&Bid], base: &[Plan], proposal: &Proposal) -> f64 {
    let planning = &round.planning;
    bids.iter()
        .zip(planned(base, proposal))
        .filter_map(|(bid, plan)| {
            plan.row.map(|row| {
                let offered = &bid.offer.rows[row];
                let quality = weight(bid) * f64::from(offered.quality);
                let priced = (planning.switch.iter())
                    .find(|(agent, id, _)| *agent == bid.offer.agent && *id == offered.id)
                    .map(|(_, _, prepare)| *prepare);
                let charge = match priced {
                    Some(prepare)
                        if offered.id != bid.offer.current && !planning.horizon.is_zero() =>
                    {
                        weight(bid) * (prepare + planning.transient).as_secs_f64()
                            / planning.horizon.as_secs_f64()
                    }
                    _ => 0.0,
                };
                quality - charge
            })
        })
        .sum()
}

/// The plan of most planned value the shield accepts, and its value: every
/// row of every agent, and its base row, enumerated. The first found is kept
/// on a tie.
fn planned_optimum(round: &Round<'_>, bids: &[&Bid], base: &[Plan]) -> (Proposal, f64) {
    let choices: Vec<Vec<Option<usize>>> = bids
        .iter()
        .map(|bid| {
            std::iter::once(None)
                .chain((0..bid.offer.rows.len()).map(Some))
                .collect()
        })
        .collect();
    let mut index = vec![0_usize; bids.len()];
    let mut best: Option<(Proposal, f64)> = None;
    loop {
        let proposal = Proposal {
            rows: index
                .iter()
                .zip(&choices)
                .map(|(chosen, choices)| choices[*chosen])
                .collect(),
            price: None,
            rejected: Vec::new(),
        };
        if Shield::check(round, bids, base, &proposal).is_ok() {
            let value = planned_value(round, bids, base, &proposal);
            if best.as_ref().is_none_or(|(_, kept)| value > *kept + EPS) {
                best = Some((proposal, value));
            }
        }
        let mut agent = 0;
        loop {
            if agent == index.len() {
                return best.expect("admission's own plan is accepted");
            }
            index[agent] += 1;
            if index[agent] < choices[agent].len() {
                break;
            }
            index[agent] = 0;
            agent += 1;
        }
    }
}

/// **The exact search finds the optimum of the planned value.** Over 400
/// rounds of at most 4 agents × 4 rows, drawn as [`generate`] draws them and
/// planned — switch charges, margins and growth — the exact plan with an
/// ample budget is accepted by the shield and plans the value of the
/// brute-force optimum: weighed quality less the switch charges, within the
/// planned target and at the planned costs. The generator is first checked
/// to draw rounds whose planned optimum is not their unplanned one.
#[test]
fn exact_finds_the_optimum_of_the_planned_value() {
    let mut rng = Rng(0x5eed_91a7_0ed0_0005);
    let cases: Vec<PlannedCase> = (0..400).map(|_| generate_planned(&mut rng, 4, 4)).collect();

    let (mut admitted_rounds, mut planning_moves) = (0, 0);
    for case in &cases {
        let round = case.round();
        let Admission::Admitted { bids, plans } = admit(&round) else {
            continue;
        };
        admitted_rounds += 1;
        let (planned_best, _) = planned_optimum(&round, &bids, &plans);
        let unplanned_round = case.case.round();
        let (unplanned_bids, unplanned_plans) = admitted(&unplanned_round);
        let (unplanned_best, _) = optimum(&unplanned_round, &unplanned_bids, &unplanned_plans);
        if rows_of(&bids, &plans, &planned_best)
            != rows_of(&unplanned_bids, &unplanned_plans, &unplanned_best)
        {
            planning_moves += 1;
        }
    }
    assert!(
        admitted_rounds >= 150,
        "the generator draws rounds whose floors fit: {admitted_rounds} of 400"
    );
    assert!(
        planning_moves > 0,
        "the generator draws rounds whose planned optimum differs"
    );

    for (number, case) in cases.iter().enumerate() {
        let round = case.round();
        let Admission::Admitted { bids, plans } = admit(&round) else {
            continue;
        };
        let (best, best_value) = planned_optimum(&round, &bids, &plans);
        let found = Exact { node_budget: AMPLE }.propose(&round, &bids, &plans);
        assert_eq!(
            Shield::check(&round, &bids, &plans, &found),
            Ok(()),
            "round {number}: the exact plan {found:?} is refused"
        );
        let found_value = planned_value(&round, &bids, &plans, &found);
        assert!(
            (found_value - best_value).abs() <= EPS,
            "round {number}: the exact plan is worth {found_value}, the optimum {best_value} \
             ({:?} against {:?})",
            rows_of(&bids, &plans, &found),
            rows_of(&bids, &plans, &best)
        );
    }
}

/// A round drawn wider than [`generate`]'s: rows timed on every device, held
/// in both memories and moving bytes over both links, which may be timed;
/// floors anywhere; negative, zero and large priorities; `Stable` and late
/// envelopes kept on several devices, some lent while a memory was not known
/// (`u64::MAX`); agents in several waves or at the background; a limit on
/// both memories.
fn generate_wide(rng: &mut Rng, max_agents: usize, max_rows: usize) -> WideCase {
    let agents = 1 + rng.index(max_agents);
    let importances = [Critical, Important, Optional];
    let mut bids = Vec::with_capacity(agents);
    for name in NAMES.iter().take(agents) {
        let count = 1 + rng.index(max_rows);
        let mut rows = Vec::with_capacity(count);
        for (index, id) in IDS.iter().take(count).enumerate() {
            let quality = rng.below(11) as f32 / 10.0;
            let mut offered = row(*id, rng.below(4_000), quality);
            if rng.chance(40) {
                offered.cost.time.gpu = us(rng.below(3_000));
            }
            if rng.chance(15) {
                offered.cost.time.npu = us(rng.below(3_000));
            }
            if rng.chance(15) {
                offered.cost.time.other = us(rng.below(3_000));
            }
            offered.cost.memory.gpu = rng.below(8) * MIB;
            offered.cost.memory.cpu = rng.below(8) * MIB;
            if rng.chance(30) {
                offered.cost.transfer.cpu_to_gpu = rng.below(4) * MIB;
            }
            if rng.chance(20) {
                offered.cost.transfer.gpu_to_cpu = rng.below(4) * MIB;
            }
            if index > 0 && rng.chance(8) {
                offered.confidence = Confidence::Assumed;
            }
            rows.push(offered);
        }
        let floor = rows[rng.index(count)].id;
        let mut bid = bid(offer(name, rows, floor), importances[rng.index(3)]);
        match rng.below(6) {
            0 => bid.hints.priority = Some(-((1 + rng.below(20)) as f32) / 10.0),
            1 => bid.hints.priority = Some(0.0),
            2 => bid.hints.priority = Some((1 + rng.below(30)) as f32 / 10.0),
            _ => {}
        }
        // Lent while the device's memory was known, or not (`u64::MAX`).
        let kept_memory = |rng: &mut Rng| {
            if rng.chance(30) {
                u64::MAX
            } else {
                rng.below(10) * MIB
            }
        };
        match rng.below(10) {
            0 => bid.mode = AdaptationMode::Manual(bid.offer.rows[rng.index(count)].id),
            1 => {
                let (a, b) = (rng.index(IDS.len()), rng.index(IDS.len()));
                bid.mode = AdaptationMode::Bounded {
                    min: IDS[a.min(b)],
                    max: IDS[a.max(b)],
                };
            }
            2 => bid.hints.cap_ms = Some(rng.below(5_000) as f32 / 1_000.0),
            3 | 4 => {
                bid.mode = AdaptationMode::Stable;
                let mut kept = cpu_envelope(rng.below(6_000));
                kept.time.gpu = us(rng.below(4_000));
                kept.memory.gpu = kept_memory(rng);
                kept.memory.cpu = kept_memory(rng);
                bid.kept = Some(kept);
            }
            5 => {
                bid.late = true;
                if rng.chance(70) {
                    let mut kept = cpu_envelope(rng.below(4_000));
                    kept.memory.gpu = kept_memory(rng);
                    bid.kept = Some(kept);
                }
            }
            _ => {}
        }
        bids.push(bid);
    }
    let mut waves = vec![Vec::new(), Vec::new(), Vec::new()];
    let mut background = Vec::new();
    for bid in &bids {
        if rng.chance(10) {
            background.push(bid.offer.agent);
        }
        if rng.chance(50) {
            waves[rng.index(3)].push(bid.offer.agent);
        }
    }
    waves.retain(|wave| !wave.is_empty());
    let memory = PerDevice {
        gpu: rng.chance(50).then(|| (4 + rng.below(20)) * MIB),
        cpu: rng.chance(50).then(|| (4 + rng.below(20)) * MIB),
        ..PerDevice::default()
    };
    let rate = |rng: &mut Rng| {
        if rng.chance(50) {
            (1 + rng.below(8)) * 1024 * MIB
        } else {
            0
        }
    };
    let link_rate = PerLink {
        cpu_to_gpu: rate(rng),
        gpu_to_cpu: rate(rng),
    };
    WideCase {
        bids,
        waves,
        background,
        target_us: 1_000 + rng.below(14_000),
        memory,
        link_rate,
    }
}

/// A round [`generate_wide`] drew: as a [`Case`], with a limit on every
/// memory and the links' rates.
struct WideCase {
    bids: Vec<Bid>,
    waves: Vec<Vec<AgentId>>,
    background: Vec<AgentId>,
    target_us: u64,
    memory: PerDevice<Option<u64>>,
    link_rate: PerLink<u64>,
}

impl WideCase {
    fn round(&self) -> Round<'_> {
        Round {
            waves: &self.waves,
            background: &self.background,
            memory: self.memory,
            link_rate: self.link_rate,
            ..round(Trigger::Refresh, self.target_us, &self.bids)
        }
    }
}

/// **The exact search finds the optimum whatever the round holds.** Over
/// 6 000 rounds of at most 4 agents × 4 rows drawn by [`generate_wide`] —
/// transfers on timed links, time on every device, both memories limited,
/// negative and zero priorities, kept envelopes whose memory was not known —
/// the exact plan with an ample budget is accepted by the shield and lends
/// the weighed quality of the brute-force optimum. The cut of a branch that
/// breaches with the agents left at their base rows is only sound where those
/// rows cost least on every device, memory and link; these rounds reach it.
#[test]
fn exact_finds_the_optimum_with_transfers_every_device_and_any_priority() {
    let mut rng = Rng(0xa77a_c4ed_0000_0001);
    let (mut admitted_rounds, mut greedy_short) = (0, 0);
    for number in 0..6_000 {
        let case = generate_wide(&mut rng, 4, 4);
        let round = case.round();
        let Admission::Admitted { bids, plans } = admit(&round) else {
            continue;
        };
        admitted_rounds += 1;
        let (best, best_worth) = optimum(&round, &bids, &plans);
        let found = Exact { node_budget: AMPLE }.propose(&round, &bids, &plans);
        assert_eq!(
            Shield::check(&round, &bids, &plans, &found),
            Ok(()),
            "round {number}: the exact plan {found:?} is refused"
        );
        let found_worth = worth(&bids, &plans, &found);
        let greedy = worth(&bids, &plans, &Greedy.propose(&round, &bids, &plans));
        if greedy + EPS < best_worth {
            greedy_short += 1;
        }
        assert!(
            (found_worth - best_worth).abs() <= EPS,
            "round {number}: the exact plan lends {found_worth}, the optimum {best_worth} \
             ({:?} against {:?})",
            rows_of(&bids, &plans, &found),
            rows_of(&bids, &plans, &best)
        );
    }
    assert!(
        admitted_rounds >= 3_000,
        "the generator draws rounds whose floors fit: {admitted_rounds} of 6000"
    );
    assert!(
        greedy_short > 0,
        "the generator draws rounds the Q-RAM pass does not solve"
    );
}

/// **The exact search never lends less than the Q-RAM pass**, whatever its
/// budget, down to a single node: over 300 rounds of at most 6 agents × 4
/// rows, its plan is accepted by the shield and lends at least the weighed
/// quality the Q-RAM pass lends.
#[test]
fn exact_never_lends_less_than_greedy() {
    const BUDGETS: [u32; 10] = [1, 2, 3, 5, 8, 13, 34, 144, 610, 4_096];
    let mut rng = Rng(0x0dd5_eed5_1234_5679);
    let mut admitted_rounds = 0;
    for number in 0..300 {
        let case = generate(&mut rng, 6, 4);
        let round = case.round();
        let Admission::Admitted { bids, plans } = admit(&round) else {
            continue;
        };
        admitted_rounds += 1;
        let greedy = worth(&bids, &plans, &Greedy.propose(&round, &bids, &plans));
        for node_budget in BUDGETS {
            let (found, _) = Exact { node_budget }.search(&round, &bids, &plans);
            assert_eq!(
                Shield::check(&round, &bids, &plans, &found),
                Ok(()),
                "round {number}, budget {node_budget}: the exact plan {found:?} is refused"
            );
            let found = worth(&bids, &plans, &found);
            assert!(
                found + EPS >= greedy,
                "round {number}, budget {node_budget}: the exact plan lends {found}, \
                 the Q-RAM pass {greedy}"
            );
        }
    }
    assert!(admitted_rounds >= 100, "{admitted_rounds} of 300 admitted");
}

/// **The exact search stops at its node budget.** Ten agents of four rows
/// each, their upgrades priced off the convex hull, in a frame that holds
/// some upgrades and not all. Under budgets of 1, 16 and 256 nodes, the
/// search visits no more nodes than its budget, and the plan it keeps is
/// accepted by the shield and lends at least what the Q-RAM pass lends. An
/// ample budget visits more than 16 nodes: the smallest budgets did stop it.
#[test]
fn the_exact_search_stops_at_its_node_budget() {
    let mut rng = Rng(0xb0d6_e7ed_0000_0010);
    let bids: Vec<Bid> = NAMES
        .iter()
        .map(|name| {
            let floor_us = 200 + rng.below(300);
            let mut rows = vec![row(LowPower, floor_us, 0.2)];
            let mut cost = floor_us;
            for id in [Balanced, HighPerformance, Custom(1)] {
                cost += 300 + rng.below(1_500);
                let quality = 0.3 + rng.below(70) as f32 / 100.0;
                rows.push(row(id, cost, quality));
            }
            let importance = [Critical, Important, Optional][rng.index(3)];
            bid(offer(name, rows, LowPower), importance)
        })
        .collect();
    let round = round(Trigger::Refresh, 12_000, &bids);
    let (admitted_bids, base) = admitted(&round);
    let greedy = worth(
        &admitted_bids,
        &base,
        &Greedy.propose(&round, &admitted_bids, &base),
    );

    for node_budget in [1, 16, 256] {
        let (found, nodes) = Exact { node_budget }.search(&round, &admitted_bids, &base);
        assert!(
            nodes <= node_budget,
            "budget {node_budget}: {nodes} nodes visited"
        );
        assert_eq!(
            Shield::check(&round, &admitted_bids, &base, &found),
            Ok(()),
            "budget {node_budget}: the plan kept {found:?} is refused"
        );
        let found = worth(&admitted_bids, &base, &found);
        assert!(
            found + EPS >= greedy,
            "budget {node_budget}: {found} lent, the Q-RAM pass lends {greedy}"
        );
    }

    let (_, unbounded) = Exact { node_budget: AMPLE }.search(&round, &admitted_bids, &base);
    assert!(
        unbounded > 16,
        "the round is large enough for a small budget to stop the search: {unbounded} nodes"
    );
}

// ── The policies ──────────────────────────────────────────────────

/// **The exact plan keeps every policy.** In a 100 ms frame where every top
/// row fits:
/// - `ash`, pinned (`Manual`) to `Balanced`, stays on it; `birch`, free,
///   takes its top row;
/// - `ash`, allowed `LowPower..=Balanced` (`Bounded`), takes `Balanced`;
/// - `ash`, capped at 5 ms, takes its 4 ms `Balanced`, not its 9 ms top row;
/// - `ash`, `Stable`, keeps 6 ms and 10 MiB that the graphics processor's
///   8 MiB can no longer hold: that envelope becomes its ceiling, and it
///   takes `Balanced` (4 ms), not its 8 ms top row, which the frame and the
///   memory would both hold;
/// - `ash`, `Stable`, keeps a 5 ms envelope that fits, and `birch`, late,
///   keeps 4 ms: both stay on their base rows, and `cedar`, free, takes its
///   top row.
#[test]
fn exact_keeps_pins_bounds_caps_ceilings_and_kept_envelopes() {
    let three = |name| {
        offer(
            name,
            vec![
                row(LowPower, 1_000, 0.4),
                row(Balanced, 2_000, 0.7),
                row(HighPerformance, 3_000, 1.0),
            ],
            LowPower,
        )
    };
    let two = |name| {
        offer(
            name,
            vec![row(LowPower, 1_000, 0.4), row(HighPerformance, 2_000, 1.0)],
            LowPower,
        )
    };

    let mut pinned = bid(three("ash"), Important);
    pinned.mode = AdaptationMode::Manual(Balanced);
    let bids = [pinned, bid(two("birch"), Important)];
    let rows = exact_rows(&round(Trigger::Refresh, 100_000, &bids));
    assert_eq!(row_for(&rows, "ash"), Some(Balanced), "pinned: {rows:?}");
    assert_eq!(
        row_for(&rows, "birch"),
        Some(HighPerformance),
        "free: {rows:?}"
    );

    let mut bounded = bid(three("ash"), Important);
    bounded.mode = AdaptationMode::Bounded {
        min: LowPower,
        max: Balanced,
    };
    let bids = [bounded];
    let rows = exact_rows(&round(Trigger::Refresh, 100_000, &bids));
    assert_eq!(row_for(&rows, "ash"), Some(Balanced), "bounded: {rows:?}");

    let mut capped = bid(
        offer(
            "ash",
            vec![
                row(LowPower, 1_000, 0.4),
                row(Balanced, 4_000, 0.7),
                row(HighPerformance, 9_000, 1.0),
            ],
            LowPower,
        ),
        Important,
    );
    capped.hints.cap_ms = Some(5.0);
    let bids = [capped];
    let rows = exact_rows(&round(Trigger::Refresh, 100_000, &bids));
    assert_eq!(row_for(&rows, "ash"), Some(Balanced), "capped: {rows:?}");

    let mut ceiled = bid(
        offer(
            "ash",
            vec![
                row_holding(LowPower, 1_000, MIB, 0.4),
                row_holding(Balanced, 4_000, 2 * MIB, 0.7),
                row_holding(HighPerformance, 8_000, 2 * MIB, 1.0),
            ],
            LowPower,
        ),
        Important,
    );
    ceiled.mode = AdaptationMode::Stable;
    let mut kept = cpu_envelope(6_000);
    kept.memory.gpu = 10 * MIB;
    ceiled.kept = Some(kept);
    let bids = [ceiled];
    let ceiling_round = Round {
        memory: PerDevice {
            gpu: Some(8 * MIB),
            ..PerDevice::default()
        },
        ..round(Trigger::Refresh, 100_000, &bids)
    };
    let (_, base) = admitted(&ceiling_round);
    assert_eq!(
        (base[0].fixed, base[0].ceiling),
        (None, Some(kept)),
        "the envelope kept no longer fits: it is a ceiling"
    );
    let rows = exact_rows(&ceiling_round);
    assert_eq!(row_for(&rows, "ash"), Some(Balanced), "ceiled: {rows:?}");

    let mut stable = bid(two("ash"), Important);
    stable.mode = AdaptationMode::Stable;
    stable.kept = Some(cpu_envelope(5_000));
    let mut late = bid(two("birch"), Important);
    late.late = true;
    late.kept = Some(cpu_envelope(4_000));
    let bids = [stable, late, bid(two("cedar"), Important)];
    let kept_round = round(Trigger::Refresh, 100_000, &bids);
    let (_, base) = admitted(&kept_round);
    assert!(
        base[0].fixed.is_some() && base[1].fixed.is_some(),
        "both envelopes are kept"
    );
    let rows = exact_rows(&kept_round);
    assert_eq!(row_for(&rows, "ash"), Some(LowPower), "stable: {rows:?}");
    assert_eq!(row_for(&rows, "birch"), Some(LowPower), "late: {rows:?}");
    assert_eq!(
        row_for(&rows, "cedar"),
        Some(HighPerformance),
        "free: {rows:?}"
    );
}

// ── Why ───────────────────────────────────────────────────────────

/// **The exact why names what stops each agent growing.** The knapsack the
/// Q-RAM pass loses, with a third agent: `fir` runs 0.1 ms holding 1 MiB at
/// 0.4, or the same time holding 20 MiB at 1.0, and the graphics processor
/// has 10 MiB. The exact plan upgrades `oak` alone (3.9 + 1 + 0.1 ms fill
/// the 5 ms frame). Each agent's next better row is then tried: `oak` has
/// none, `pine`'s breaks the host's time (3.9 + 1.5 + 0.1 ms), `fir`'s the
/// graphics processor's memory. The decision is the exact proposer's, names
/// those two refusals and no other, binds on one of them, and has no price.
#[test]
fn the_exact_why_names_what_stops_each_agent_growing() {
    let bids = [
        bid(
            offer(
                "oak",
                vec![row(LowPower, 1_000, 0.4), row(HighPerformance, 3_900, 1.0)],
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
        bid(
            offer(
                "fir",
                vec![
                    row_holding(LowPower, 100, MIB, 0.4),
                    row_holding(HighPerformance, 100, 20 * MIB, 1.0),
                ],
                LowPower,
            ),
            Important,
        ),
    ];
    let round = Round {
        memory: PerDevice {
            gpu: Some(10 * MIB),
            ..PerDevice::default()
        },
        ..round(Trigger::Refresh, 5_000, &bids)
    };

    let decision = Arbiter::with(vec![Box::new(Greedy), Box::new(Exact::default())]).decide(&round);

    assert_eq!(decision.why.by, DecidedBy::Exact, "{:?}", decision.why);
    let pine = Rejected {
        agent: AgentId::new("pine"),
        row: HighPerformance,
        over: Binding::Time(Device::Cpu),
    };
    let fir = Rejected {
        agent: AgentId::new("fir"),
        row: HighPerformance,
        over: Binding::Memory(Device::Gpu),
    };
    let rejected = &decision.why.rejected;
    assert_eq!(
        rejected.len(),
        2,
        "pine's and fir's next rows: {rejected:?}"
    );
    assert!(rejected.contains(&pine), "{rejected:?}");
    assert!(rejected.contains(&fir), "{rejected:?}");
    assert!(
        decision
            .why
            .binding
            .is_some_and(|binding| binding == pine.over || binding == fir.over),
        "{:?}",
        decision.why
    );
    assert_eq!(
        decision.allocation.price, None,
        "an exact plan has no cutoff: {:?}",
        decision.allocation
    );
    let oak = lent_to(&decision.allocation, "oak");
    assert!(
        fits(&bids[0].offer.rows[1].cost, &oak),
        "oak upgraded: {oak:?}"
    );
}

/// **The exact why names a `Stable` ceiling that stops an agent growing.**
/// The knapsack the Q-RAM pass loses, in a 6 ms frame, with `ash`, `Stable`,
/// keeping 0.5 ms: less than its 1 ms floor, so that envelope is a ceiling,
/// and its 1.2 ms top row is over it. The exact plan upgrades `oak` alone
/// (3.9 + 1 + 1 ms). What stops `ash` growing is its ceiling, as the Q-RAM
/// pass alone says of the same round: the exact why names it too.
#[test]
fn the_exact_why_names_a_stable_ceiling_that_stops_an_agent() {
    let mut ash = bid(
        offer(
            "ash",
            vec![row(LowPower, 1_000, 0.4), row(HighPerformance, 1_200, 1.0)],
            LowPower,
        ),
        Important,
    );
    ash.mode = AdaptationMode::Stable;
    ash.kept = Some(cpu_envelope(500));
    let bids = [
        bid(
            offer(
                "oak",
                vec![row(LowPower, 1_000, 0.4), row(HighPerformance, 3_900, 1.0)],
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
        ash,
    ];
    let round = round(Trigger::Refresh, 6_000, &bids);
    let ceiling = Rejected {
        agent: AgentId::new("ash"),
        row: HighPerformance,
        over: Binding::Ceiling(Device::Cpu),
    };

    let greedy = Arbiter::with(vec![Box::new(Greedy)]).decide(&round);
    assert!(
        greedy.why.rejected.contains(&ceiling),
        "the Q-RAM pass names the ceiling: {:?}",
        greedy.why
    );

    let decision = Arbiter::default().decide(&round);
    assert_eq!(decision.why.by, DecidedBy::Exact, "{:?}", decision.why);
    assert!(
        decision.why.rejected.contains(&ceiling),
        "the exact why names ash's ceiling: {:?}",
        decision.why
    );
}

/// **A tie keeps the Q-RAM plan.** Two identical agents and room for one
/// upgrade: the Q-RAM pass gives it to `alpha`, and upgrading `beta` instead
/// lends as much — the exact search finds nothing better. The decision is
/// the Q-RAM pass's, with its price and its refusals: the allocation is the
/// one the Q-RAM pass alone lends.
#[test]
fn a_tie_keeps_the_greedy_plan() {
    let twin = |name| {
        bid(
            offer(
                name,
                vec![row(LowPower, 1_000, 0.4), row(HighPerformance, 3_000, 1.0)],
                LowPower,
            ),
            Important,
        )
    };
    let bids = [twin("alpha"), twin("beta")];
    let round = round(Trigger::Refresh, 5_000, &bids);

    let (admitted_bids, base) = admitted(&round);
    let greedy_worth = worth(
        &admitted_bids,
        &base,
        &Greedy.propose(&round, &admitted_bids, &base),
    );
    let exact = Exact::default().propose(&round, &admitted_bids, &base);
    assert!(
        (worth(&admitted_bids, &base, &exact) - greedy_worth).abs() < EPS,
        "the exact plan lends what the Q-RAM plan lends: {exact:?}"
    );

    let alone = Arbiter::with(vec![Box::new(Greedy)]).decide(&round);
    let decision = Arbiter::with(vec![Box::new(Greedy), Box::new(Exact::default())]).decide(&round);

    assert_eq!(decision.why.by, DecidedBy::Greedy, "{:?}", decision.why);
    assert!(
        decision.allocation.price.is_some(),
        "the Q-RAM price is kept: {:?}",
        decision.allocation
    );
    assert_eq!(decision.allocation, alone.allocation);
    assert_eq!(decision.why, alone.why);
    let top = cpu_cost(3_000);
    assert!(fits(&top, &lent_to(&decision.allocation, "alpha")));
    assert!(!fits(&top, &lent_to(&decision.allocation, "beta")));
}
