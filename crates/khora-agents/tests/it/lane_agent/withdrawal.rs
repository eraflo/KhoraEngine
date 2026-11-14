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

//! A `LaneAgent` withdrawn — its family off and on again — drops the
//! envelope, pin and allowed rows it held: its place's terms lend nothing,
//! so a `#[budget]` lane runs at its own price, on the row already chosen.
//! The next allocation lends it again.
//!
//! The family's one lane takes an `Option<Budget>` and records whether it was
//! lent one, and how much.

use std::sync::Arc;
use std::time::Duration;

use khora_agents::lane_agent::{lane_agent, LaneAgent};
use khora_core::agent::gorna::protocol::Epoch;
use khora_core::agent::gorna::{AgentId, StrategyId};
use khora_core::agent::registration::AgentRegistration;
use khora_core::agent::{Agent, AgentImportance, ExecutionPhase, Terms};
use khora_core::gpu::DeviceCaps;
use khora_core::lane::compile_check::BudgetField;
use khora_core::lane::cost::{Cost, PerDevice};
use khora_core::lane::family::{LaneFamily, Semantics};
use khora_core::lane::guard::InputRefusal;
use khora_core::lane::io::{Budget, Declaration, InputSource, LaneInputs};
use khora_core::lane::level::Level;
use khora_core::lane::{Lane, LaneError, LaneReport, OutputDeck};
use khora_core::Runtime;

use super::fixtures::{
    host_envelope, installed_row, level, locked, place_of, recorded_level, run, runtime, Plan,
    ProbeOutputs, Recorded, LADDER,
};

// ─── The family ─────────────────────────────────────────────────────────────

pub struct PricedFamily;

impl LaneFamily for PricedFamily {
    const NAME: &'static str = "priced";
    const AGENT: AgentId = AgentId::new("script");
    const SEMANTICS: Semantics = Semantics::Presentation;
    type Outputs<'f> = ProbeOutputs;
}

/// What the lane reads: the budget lent, if any — as `#[derive(LaneInputs)]`
/// with `#[budget] budget` derives it (this crate's tests do not depend on
/// khora-macros).
pub struct MaybeBudget {
    pub budget: Option<Budget>,
}

impl<'f> LaneInputs<'f> for MaybeBudget {
    fn declare(d: &mut Declaration) {
        <Option<Budget> as BudgetField>::declare(d, "budget");
    }

    fn fetch(src: &mut InputSource<'f>) -> Result<Self, InputRefusal> {
        Ok(Self {
            budget: <Option<Budget> as BudgetField>::fetch(src, "budget")?,
        })
    }
}

/// Two levels: `LowPower`, the default, at 100 µs, and `HighPerformance` at
/// 300 µs.
const PRICED_LEVELS: &[Level] = &[
    level(StrategyId::LowPower, "low"),
    level(StrategyId::HighPerformance, "high"),
];

/// What a run records when it was lent no budget: it ran at its own price.
const OWN_PRICE: &str = "own price";

/// What a run records when it was lent a budget.
const LENT: &str = "lent";

/// Records, each run, whether it was lent a budget and how many µs.
#[derive(Default)]
pub struct PricedLane;

impl Lane for PricedLane {
    type Family = PricedFamily;
    type Inputs<'f> = MaybeBudget;

    fn levels(&self) -> &[Level] {
        PRICED_LEVELS
    }

    fn cost(&self, level: usize, _inputs: &MaybeBudget) -> Cost {
        Cost {
            time: PerDevice {
                cpu: Duration::from_micros(100 + 200 * level as u64),
                ..PerDevice::default()
            },
            ..Cost::default()
        }
    }

    fn record(
        &mut self,
        level: usize,
        inputs: &mut MaybeBudget,
        out: &mut ProbeOutputs,
    ) -> Result<LaneReport, LaneError> {
        out.recorded = Recorded {
            lane: if inputs.budget.is_some() {
                LENT
            } else {
                OWN_PRICE
            },
            level,
            record: inputs.budget.map_or(0, |budget| {
                u32::try_from(budget.time.as_micros()).unwrap_or(u32::MAX)
            }),
        };
        Ok(LaneReport {
            done: 1.0,
            stalled: false,
            message: None,
        })
    }
}

static PRICED: AgentRegistration = khora_core::register_agent! {
    family: PricedFamily,
    phase: ExecutionPhase::TRANSFORM,
    importance: AgentImportance::Important,
    after: [],
    fixed_step: None,
    lanes: [PricedLane],
};

// ─── Driving it ─────────────────────────────────────────────────────────────

/// The terms `agent`'s place lends its layer.
fn terms_of(agent: &LaneAgent) -> Terms {
    locked(&place_of(agent)).terms().clone()
}

/// What the lane recorded on `deck`'s last run.
fn recorded(deck: &OutputDeck) -> Option<Recorded> {
    deck.get::<Recorded>().cloned()
}

/// The priced family's agent after its first frame, then lent 1 ms pinned to
/// `HighPerformance` and run until that row is installed and has run on it;
/// the frame count it reached, the runtime and the deck.
fn lent_and_running() -> (LaneAgent, u64, Arc<Runtime>, OutputDeck) {
    let mut agent = lane_agent(&PRICED, &DeviceCaps::default());
    let runtime = Arc::new(Runtime::new());
    let mut deck = OutputDeck::new();
    run(&mut agent, 1, &runtime, &mut deck);
    agent.apply(
        Epoch(1),
        Some(&host_envelope(1_000)),
        Some(StrategyId::HighPerformance),
        None,
    );
    run(&mut agent, 2, &runtime, &mut deck);
    run(&mut agent, 3, &runtime, &mut deck);
    assert_eq!(installed_row(&agent), Some(StrategyId::HighPerformance));
    let terms = terms_of(&agent);
    assert_eq!(terms.envelope, Some(host_envelope(1_000)));
    assert_eq!(terms.budget, Some(Duration::from_micros(1_000)));
    assert_eq!(
        recorded(&deck),
        Some(Recorded {
            lane: LENT,
            level: 1,
            record: 1_000,
        }),
        "the lane runs on the budget lent"
    );
    (agent, 3, runtime, deck)
}

/// **A withdrawn `LaneAgent` lends its layer nothing, and keeps its row.**
/// Lent 1 ms and pinned to `HighPerformance`, the agent runs that row on a
/// 1 ms budget. Withdrawn, its place's terms hold no envelope and no budget
/// at once; its next frames run the same row, its `#[budget]` lane lent no
/// budget — at its own price.
#[test]
fn a_withdrawn_lane_agent_lends_its_layer_nothing_and_keeps_its_row() {
    let (mut agent, frame, runtime, mut deck) = lent_and_running();

    agent.withdraw();
    let terms = terms_of(&agent);
    assert_eq!(terms.envelope, None, "no envelope once withdrawn");
    assert_eq!(terms.budget, None, "no budget once withdrawn");

    for next in frame + 1..=frame + 2 {
        run(&mut agent, next, &runtime, &mut deck);
        let terms = terms_of(&agent);
        assert_eq!(terms.envelope, None, "frame {next}: {terms:?}");
        assert_eq!(terms.budget, None, "frame {next}: {terms:?}");
        assert_eq!(
            installed_row(&agent),
            Some(StrategyId::HighPerformance),
            "frame {next}: the row chosen stays"
        );
        assert_eq!(
            recorded(&deck),
            Some(Recorded {
                lane: OWN_PRICE,
                level: 1,
                record: 0,
            }),
            "frame {next}: the lane runs its row at its own price"
        );
    }
}

/// **A withdrawn `LaneAgent` is lent again by the next allocation.** Lent
/// 1 ms, then withdrawn, its lane runs at its own price; lent 2 ms for epoch
/// 2, its place's terms carry that envelope and budget, and its lane runs
/// on 2 ms.
#[test]
fn a_withdrawn_lane_agent_is_lent_again_by_the_next_allocation() {
    let (mut agent, frame, runtime, mut deck) = lent_and_running();

    agent.withdraw();
    run(&mut agent, frame + 1, &runtime, &mut deck);
    assert_eq!(
        recorded(&deck).map(|recorded| recorded.lane),
        Some(OWN_PRICE),
        "withdrawn: at its own price"
    );

    agent.apply(Epoch(2), Some(&host_envelope(2_000)), None, None);
    run(&mut agent, frame + 2, &runtime, &mut deck);
    run(&mut agent, frame + 3, &runtime, &mut deck);

    let terms = terms_of(&agent);
    assert_eq!(terms.envelope, Some(host_envelope(2_000)));
    assert_eq!(terms.budget, Some(Duration::from_micros(2_000)));
    assert_eq!(
        recorded(&deck).map(|recorded| (recorded.lane, recorded.record)),
        Some((LENT, 2_000)),
        "lent again: the lane runs on the new budget"
    );
}

/// **A pin lent without an envelope is kept, and the lane runs at its own
/// price.** Running `HighPerformance` on a 1 ms envelope, the agent is lent
/// a pin to `LowPower` and no envelope — the round could not price it. Its
/// place's terms lend nothing, it installs `LowPower`, and the lane runs it
/// at its own price.
#[test]
fn a_pin_lent_without_an_envelope_is_kept_at_the_lanes_own_price() {
    let (mut agent, frame, runtime, mut deck) = lent_and_running();

    agent.apply(Epoch(2), None, Some(StrategyId::LowPower), None);
    run(&mut agent, frame + 1, &runtime, &mut deck);
    run(&mut agent, frame + 2, &runtime, &mut deck);

    let terms = terms_of(&agent);
    assert_eq!(terms.envelope, None, "no envelope lent");
    assert_eq!(terms.budget, None, "no budget lent");
    assert_eq!(installed_row(&agent), Some(StrategyId::LowPower), "the pin");
    assert_eq!(
        recorded(&deck),
        Some(Recorded {
            lane: OWN_PRICE,
            level: 0,
            record: 0,
        }),
        "the pinned row runs at its own price"
    );
}

/// **Allowed rows lent without an envelope keep the running row while they
/// allow it, and move off it once they do not.** Running `HighPerformance`
/// on a 1 ms envelope, the agent is lent no envelope and allowed
/// `LowPower` and `HighPerformance`: it stays on `HighPerformance`, not its
/// default `LowPower`, and its lane runs at its own price. Lent no envelope
/// again and allowed `LowPower` alone, it installs `LowPower`.
#[test]
fn allowed_rows_lent_without_an_envelope_keep_the_running_row_while_they_allow_it() {
    let (mut agent, frame, runtime, mut deck) = lent_and_running();

    agent.apply(
        Epoch(2),
        None,
        None,
        Some(&[StrategyId::LowPower, StrategyId::HighPerformance]),
    );
    run(&mut agent, frame + 1, &runtime, &mut deck);
    run(&mut agent, frame + 2, &runtime, &mut deck);
    assert_eq!(
        installed_row(&agent),
        Some(StrategyId::HighPerformance),
        "the running row is allowed: it stays"
    );
    assert_eq!(
        recorded(&deck),
        Some(Recorded {
            lane: OWN_PRICE,
            level: 1,
            record: 0,
        }),
        "the running row runs at its own price"
    );

    agent.apply(Epoch(3), None, None, Some(&[StrategyId::LowPower]));
    run(&mut agent, frame + 3, &runtime, &mut deck);
    run(&mut agent, frame + 4, &runtime, &mut deck);
    assert_eq!(
        installed_row(&agent),
        Some(StrategyId::LowPower),
        "the running row is no longer allowed: the row the policy allows"
    );
    assert_eq!(
        recorded(&deck).map(|recorded| recorded.level),
        Some(0),
        "the allowed row runs"
    );
}

/// **A row that failed with no envelope lent is passed by while an allowed
/// row that has not failed is left.** The ladder starts on its first level.
/// Lent no envelope — the round could not price it — and allowed only its
/// middle and last levels, it runs the middle one, the first the policy
/// allows. The plan fails the middle level: the agent marks it failed and
/// falls back to the level below, which the policy does not allow. It must
/// not go back to the middle level, which failed under this allocation,
/// while the last level is allowed and has not failed.
#[test]
fn a_row_failed_without_an_envelope_is_passed_by_while_an_allowed_row_is_left() {
    let mut agent = lane_agent(&LADDER, &DeviceCaps::default());
    agent.apply(
        Epoch(1),
        None,
        None,
        Some(&[StrategyId::Balanced, StrategyId::HighPerformance]),
    );
    let failing = runtime(
        100,
        Plan {
            fail_levels: vec![1],
            ..Plan::default()
        },
    );

    let mut levels = Vec::new();
    for frame in 1..=8 {
        let mut deck = OutputDeck::new();
        run(&mut agent, frame, &failing, &mut deck);
        levels.push(recorded_level(&deck));
    }

    assert_eq!(
        &levels[5..],
        &[Some(2), Some(2), Some(2)],
        "the middle level failed: the last level, allowed, runs — not the failed \
         row again ({levels:?})"
    );
}
