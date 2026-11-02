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

//! What an agent's health says: the lesser of how well its run kept to the
//! time GORNA allowed and how much of its work the lane reported done. A run
//! that was refused or failed reports nothing done, so it leaves the last
//! `done` as it was.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use khora_agents::agent::{lane_agent, LaneAgent};
use khora_core::agent::gorna::{AgentId, ResourceBudget, StrategyId};
use khora_core::agent::registration::AgentRegistration;
use khora_core::agent::{Agent, AgentImportance, ExecutionPhase};
use khora_core::gpu::DeviceCaps;
use khora_core::lane::contract::{Lane, LaneReport};
use khora_core::lane::cost::Cost;
use khora_core::lane::family::{LaneFamily, Semantics};
use khora_core::lane::guard::{InputRefusal, SourceKind};
use khora_core::lane::io::{Declaration, InputSource, LaneInputs};
use khora_core::lane::level::Level;
use khora_core::lane::{LaneError, LaneKind, OutputDeck};
use khora_core::Runtime;

use super::fixtures::{level, run, ProbeOutputs, Recorded};

/// What the effort lane is told to do this frame.
#[derive(Debug, Clone, Copy)]
pub struct Effort {
    /// The share of its work the lane reports done.
    pub done: f32,
    /// How long the record takes.
    pub takes: Duration,
    /// Whether the record fails.
    pub fails: bool,
    /// Whether the lane reports it could not progress.
    pub stalls: bool,
}

impl Effort {
    /// A record reporting `done`, at once.
    fn done(done: f32) -> Self {
        Self {
            done,
            takes: Duration::ZERO,
            fails: false,
            stalls: false,
        }
    }

    /// The same record, taking `takes`.
    fn taking(self, takes: Duration) -> Self {
        Self { takes, ..self }
    }
}

/// The effort, read without locking.
pub struct EffortInput<'f> {
    pub effort: &'f Effort,
}

impl<'f> LaneInputs<'f> for EffortInput<'f> {
    fn declare(d: &mut Declaration) {
        d.read::<Effort>("effort", SourceKind::Resource);
    }

    fn fetch(src: &mut InputSource<'f>) -> Result<Self, InputRefusal> {
        Ok(Self {
            effort: src.entry::<Effort>("effort", SourceKind::Resource)?,
        })
    }
}

/// A family of one lane, which reports the share of its work it was told.
pub struct EffortFamily;

impl LaneFamily for EffortFamily {
    const KIND: LaneKind = LaneKind::Script;
    const NAME: &'static str = "effort";
    const AGENT: AgentId = AgentId::new("script");
    const SEMANTICS: Semantics = Semantics::Presentation;
    type Outputs<'f> = ProbeOutputs;
}

const EFFORT_LEVELS: &[Level] = &[level(StrategyId::Balanced, "effort")];

/// Records as its effort says: taking that long, reporting that much done,
/// or failing.
#[derive(Default)]
pub struct EffortLane;

impl Lane for EffortLane {
    type Family = EffortFamily;
    type Inputs<'f> = EffortInput<'f>;

    fn levels(&self) -> &[Level] {
        EFFORT_LEVELS
    }

    fn cost(&self, _level: usize, _inputs: &EffortInput<'_>) -> Cost {
        Cost::default()
    }

    fn record(
        &mut self,
        level: usize,
        inputs: &mut EffortInput<'_>,
        out: &mut ProbeOutputs,
    ) -> Result<LaneReport, LaneError> {
        let effort = *inputs.effort;
        if !effort.takes.is_zero() {
            std::thread::sleep(effort.takes);
        }
        if effort.fails {
            return Err(LaneError::InvalidContext {
                expected: "an effort that records",
                received: "one told to fail".to_owned(),
            });
        }
        out.recorded = Recorded {
            lane: "effort",
            level,
            record: 1,
        };
        Ok(LaneReport {
            done: effort.done,
            stalled: effort.stalls,
        })
    }
}

static EFFORT: AgentRegistration = khora_core::register_agent! {
    family: EffortFamily,
    phase: ExecutionPhase::TRANSFORM,
    importance: AgentImportance::Important,
    after: [],
    modes: [],
    fixed_step: None,
    lanes: [EffortLane],
};

/// The effort agent, allowed `time_limit` a frame.
fn effort_agent(time_limit: Duration) -> LaneAgent {
    let mut agent = lane_agent(&EFFORT, &DeviceCaps::default());
    agent.apply_budget(ResourceBudget {
        strategy_id: StrategyId::Balanced,
        time_limit,
        memory_limit: None,
        extra_params: HashMap::new(),
    });
    agent
}

/// A runtime holding `effort`.
fn with(effort: Effort) -> Arc<Runtime> {
    let mut runtime = Runtime::new();
    runtime.resources.insert(effort);
    Arc::new(runtime)
}

fn health(agent: &LaneAgent) -> f32 {
    agent.report_status().health_score
}

/// **Health is the lesser of the time ratio and what was done.** Well within
/// a whole second, a lane reporting 60 % of its work done gives 0.6, and one
/// reporting all of it gives the time ratio, 1. A lane that takes three times
/// the millisecond it was allowed is held to its time ratio — a third at most
/// — whether it reports everything done or only 60 %.
#[test]
fn health_is_the_lesser_of_the_time_ratio_and_what_was_done() {
    let mut deck = OutputDeck::new();

    let mut roomy = effort_agent(Duration::from_secs(1));
    run(&mut roomy, 1, &with(Effort::done(0.6)), &mut deck);
    assert_eq!(health(&roomy), 0.6, "within its time, 60 % done");
    run(&mut roomy, 2, &with(Effort::done(1.0)), &mut deck);
    assert_eq!(health(&roomy), 1.0, "within its time, everything done");

    let mut tight = effort_agent(Duration::from_millis(1));
    let slow = Duration::from_millis(3);
    run(
        &mut tight,
        1,
        &with(Effort::done(1.0).taking(slow)),
        &mut deck,
    );
    let all_done = health(&tight);
    assert!(
        all_done > 0.0 && all_done <= 1.0 / 3.0,
        "everything done three times over its time: the time ratio, got {all_done}"
    );
    run(
        &mut tight,
        2,
        &with(Effort::done(0.6).taking(slow)),
        &mut deck,
    );
    let partly_done = health(&tight);
    assert!(
        partly_done > 0.0 && partly_done <= 1.0 / 3.0,
        "60 % done three times over its time: the time ratio, the lesser, got {partly_done}"
    );
}

/// **A refused run keeps the previous `done`.** The lane reported 60 % done;
/// the next frame's input is missing, so the lane did not run and reported
/// nothing: the agent's health still says 0.6.
#[test]
fn a_refused_run_keeps_the_previous_done() {
    let mut agent = effort_agent(Duration::from_secs(1));
    let mut deck = OutputDeck::new();
    run(&mut agent, 1, &with(Effort::done(0.6)), &mut deck);
    assert_eq!(health(&agent), 0.6);

    run(&mut agent, 2, &Arc::new(Runtime::new()), &mut deck);

    assert!(
        !agent.report_status().message.is_empty(),
        "the second run was refused"
    );
    assert_eq!(health(&agent), 0.6, "the refused run left `done` as it was");
}

/// A failed run reports nothing done either: the health keeps the last
/// `done` a recorded run gave.
#[test]
fn a_failed_run_keeps_the_previous_done() {
    let mut agent = effort_agent(Duration::from_secs(1));
    let mut deck = OutputDeck::new();
    run(&mut agent, 1, &with(Effort::done(0.6)), &mut deck);
    assert_eq!(health(&agent), 0.6);

    let failing = Effort {
        fails: true,
        ..Effort::done(1.0)
    };
    run(&mut agent, 2, &with(failing), &mut deck);

    assert!(
        !agent.report_status().message.is_empty(),
        "the second run failed"
    );
    assert_eq!(health(&agent), 0.6, "the failed run left `done` as it was");
}

/// **Health stays between 0 and 1 whatever share a lane reports.** A lane
/// reporting a negative share gives a health of 0, one reporting more than
/// all of its work gives 1, and one reporting no number at all (NaN) still
/// gives a health within the range.
#[test]
fn health_stays_within_zero_and_one_whatever_done_a_lane_reports() {
    let mut deck = OutputDeck::new();
    let mut agent = effort_agent(Duration::from_secs(1));

    run(&mut agent, 1, &with(Effort::done(-0.5)), &mut deck);
    assert_eq!(health(&agent), 0.0, "a negative share is nothing done");

    run(&mut agent, 2, &with(Effort::done(1.5)), &mut deck);
    assert_eq!(health(&agent), 1.0, "more than all of it is all of it");

    run(&mut agent, 3, &with(Effort::done(f32::NAN)), &mut deck);
    let unreported = health(&agent);
    assert!(
        (0.0..=1.0).contains(&unreported),
        "a NaN share keeps the health in range, got {unreported}"
    );
}

/// **A share that is not a number counts as nothing done.** A lane that ran
/// within its time but reported NaN gives a health of 0, not the time ratio.
#[test]
fn a_nan_done_counts_as_nothing_done() {
    let mut deck = OutputDeck::new();
    let mut agent = effort_agent(Duration::from_secs(1));

    run(&mut agent, 1, &with(Effort::done(f32::NAN)), &mut deck);

    assert_eq!(health(&agent), 0.0, "NaN done is nothing done");
}

/// **A report says when the lane stalled**, so the DCC learns of a stall from
/// the report the scheduler publishes, without asking the agent: a run whose
/// lane could not progress reports `stalled`, and the next run that does
/// progress no longer does.
#[test]
fn a_report_says_when_the_lane_stalled() {
    let mut agent = effort_agent(Duration::from_secs(1));
    let mut deck = OutputDeck::new();
    let stalling = Effort {
        stalls: true,
        ..Effort::done(0.0)
    };

    run(&mut agent, 1, &with(stalling), &mut deck);
    assert!(agent.report_status().is_stalled, "the status says so");
    assert!(agent.report().stalled, "and so does the report");

    run(&mut agent, 2, &with(Effort::done(1.0)), &mut deck);
    assert!(!agent.report().stalled, "the lane progresses again");
}
