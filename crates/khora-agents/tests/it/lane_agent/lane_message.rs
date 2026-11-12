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

//! A lane's message: what a recorded run says in its `LaneReport` is the
//! message of the agent's report until a recorded run says nothing; a
//! refusal or a failure says what went wrong instead.

use std::sync::Arc;

use khora_agents::lane_agent::{lane_agent, LaneAgent};
use khora_core::agent::gorna::{AgentId, StrategyId};
use khora_core::agent::registration::AgentRegistration;
use khora_core::agent::{Agent, AgentImportance, ExecutionPhase};
use khora_core::gpu::DeviceCaps;
use khora_core::lane::cost::Cost;
use khora_core::lane::family::{LaneFamily, Semantics};
use khora_core::lane::guard::{InputRefusal, SourceKind};
use khora_core::lane::io::{Declaration, InputSource, LaneInputs};
use khora_core::lane::level::Level;
use khora_core::lane::{Lane, LaneReport};
use khora_core::lane::{LaneError, OutputDeck};
use khora_core::Runtime;

use super::fixtures::{level, run, ProbeOutputs, Recorded};
use crate::protocol::{pin, said};

/// What the saying lane is told to do this frame.
#[derive(Debug, Clone, Copy)]
pub enum Say {
    /// Record, saying this.
    Message(&'static str),
    /// Record, saying nothing.
    Nothing,
    /// Fail.
    Fail,
}

pub struct SayInput<'f> {
    pub say: &'f Say,
}

impl<'f> LaneInputs<'f> for SayInput<'f> {
    fn declare(d: &mut Declaration) {
        d.read::<Say>("say", SourceKind::Resource);
    }

    fn fetch(src: &mut InputSource<'f>) -> Result<Self, InputRefusal> {
        Ok(Self {
            say: src.entry::<Say>("say", SourceKind::Resource)?,
        })
    }
}

/// A family of one lane, which says what it is told.
pub struct SayingFamily;

impl LaneFamily for SayingFamily {
    const NAME: &'static str = "saying";
    const AGENT: AgentId = AgentId::new("script");
    const SEMANTICS: Semantics = Semantics::Presentation;
    type Outputs<'f> = ProbeOutputs;
}

const SAYING_LEVELS: &[Level] = &[level(StrategyId::Balanced, "saying")];

/// Records what it is told to say, or fails.
#[derive(Default)]
pub struct SayingLane;

impl Lane for SayingLane {
    type Family = SayingFamily;
    type Inputs<'f> = SayInput<'f>;

    fn levels(&self) -> &[Level] {
        SAYING_LEVELS
    }

    fn cost(&self, _level: usize, _inputs: &SayInput<'_>) -> Cost {
        Cost::default()
    }

    fn record(
        &mut self,
        level: usize,
        inputs: &mut SayInput<'_>,
        out: &mut ProbeOutputs,
    ) -> Result<LaneReport, LaneError> {
        let message = match *inputs.say {
            Say::Message(message) => Some(message.to_owned()),
            Say::Nothing => None,
            Say::Fail => {
                return Err(LaneError::InvalidInput {
                    expected: "a lane told to record",
                    received: "one told to fail".to_owned(),
                })
            }
        };
        out.recorded = Recorded {
            lane: "saying",
            level,
            record: 1,
        };
        Ok(LaneReport {
            done: 1.0,
            stalled: false,
            message,
        })
    }
}

static SAYING: AgentRegistration = khora_core::register_agent! {
    family: SayingFamily,
    phase: ExecutionPhase::TRANSFORM,
    importance: AgentImportance::Important,
    after: [],
    fixed_step: None,
    lanes: [SayingLane],
};

fn saying_agent() -> LaneAgent {
    let mut agent = lane_agent(&SAYING, &DeviceCaps::default());
    pin(&mut agent, StrategyId::Balanced);
    agent
}

/// A runtime telling the lane to `say`.
fn telling(say: Say) -> Arc<Runtime> {
    let mut runtime = Runtime::new();
    runtime.resources.insert(say);
    Arc::new(runtime)
}

fn message(agent: &LaneAgent) -> String {
    said(agent)
}

const DEFERRED: &str = "2 critical behaviours deferred: Guard, Door";

/// **A report carries the message of the run it reports.** A run whose lane
/// says something reports it; the next run, whose lane says nothing, reports
/// no message — the first one's does not linger — and a run that says
/// something again reports that.
#[test]
fn a_report_carries_the_runs_message() {
    let mut agent = saying_agent();
    let mut deck = OutputDeck::new();

    run(&mut agent, 1, &telling(Say::Message(DEFERRED)), &mut deck);
    assert_eq!(
        deck.get::<Recorded>().map(|recorded| recorded.lane),
        Some("saying"),
        "the run recorded"
    );
    assert_eq!(agent.report().message, Some(DEFERRED.to_owned()));

    run(&mut agent, 2, &telling(Say::Nothing), &mut deck);
    assert_eq!(
        agent.report().message,
        None,
        "the lane said nothing, so the report holds no message"
    );

    run(
        &mut agent,
        3,
        &telling(Say::Message("1 behaviour deferred")),
        &mut deck,
    );
    assert_eq!(
        agent.report().message,
        Some("1 behaviour deferred".to_owned())
    );
}

/// **A refusal says what went wrong instead**: the lane did not run, so its
/// last message no longer stands. Once it records again, it is its message
/// that stands.
#[test]
fn a_refusal_replaces_the_lanes_message_until_it_records_again() {
    let mut agent = saying_agent();
    let mut deck = OutputDeck::new();
    run(&mut agent, 1, &telling(Say::Message(DEFERRED)), &mut deck);

    run(&mut agent, 2, &Arc::new(Runtime::new()), &mut deck);
    let refused = message(&agent);
    assert!(
        refused.contains("was refused") && refused.contains("say"),
        "the refusal names the missing input: {refused:?}"
    );

    run(
        &mut agent,
        3,
        &telling(Say::Message("1 behaviour deferred")),
        &mut deck,
    );
    assert_eq!(message(&agent), "1 behaviour deferred");
}

/// **A failure says what went wrong instead**, and a recorded run after it
/// says its own message.
#[test]
fn a_failure_replaces_the_lanes_message_until_it_records_again() {
    let mut agent = saying_agent();
    let mut deck = OutputDeck::new();
    run(&mut agent, 1, &telling(Say::Message(DEFERRED)), &mut deck);

    run(&mut agent, 2, &telling(Say::Fail), &mut deck);
    let failed = message(&agent);
    assert!(
        failed.contains("one told to fail"),
        "the failure is the message: {failed:?}"
    );

    run(&mut agent, 3, &telling(Say::Message(DEFERRED)), &mut deck);
    run(&mut agent, 4, &telling(Say::Message(DEFERRED)), &mut deck);
    assert_eq!(message(&agent), DEFERRED);
}
