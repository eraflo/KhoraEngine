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

//! The fixed step a lane reads as a frame value (`#[frame] FixedStep`): only
//! a fixed-step agent supplies it, so a family reading it must declare one.

use std::any::TypeId;
use std::time::Duration;

use khora_core::agent::gorna::{AgentId, StrategyId};
use khora_core::agent::registration::{check_registrations, AgentRegistration};
use khora_core::agent::{AgentImportance, ExecutionPhase};
use khora_core::gpu::{DeviceCaps, GpuSetup};
use khora_core::lane::contract::{Lane, LaneReport};
use khora_core::lane::cost::Cost;
use khora_core::lane::erased::{AnyLane, Erased, RunOutcome};
use khora_core::lane::family::{LaneFamily, Semantics};
use khora_core::lane::guard::{Reason, SourceKind};
use khora_core::lane::io::{FixedStep, FrameId, InputSource, OutputSink};
use khora_core::lane::level::Level;
use khora_core::lane::{LaneBus, LaneError, LaneKind, OutputDeck};
use khora_core::Runtime;
use khora_macros::{LaneInputs, LaneOutputs};

use crate::fixtures::level;

/// A slot the tick family writes: the simulated time its runs advanced.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct Advanced(pub Duration);

/// What the tick lane reads: the step its agent runs it at.
#[derive(LaneInputs)]
pub struct TickInputs {
    #[frame]
    pub step: FixedStep,
}

/// The tick family's outputs.
#[derive(LaneOutputs)]
pub struct TickOutputs {
    #[deck]
    pub advanced: Advanced,
}

/// Advances by its agent's fixed step.
pub struct TickFamily;

impl LaneFamily for TickFamily {
    const KIND: LaneKind = LaneKind::Physics;
    const NAME: &'static str = "tick";
    const AGENT: AgentId = AgentId::Physics;
    const SEMANTICS: Semantics = Semantics::Exact;
    type Outputs<'f> = TickOutputs;
}

const TICK_LEVELS: &[Level] = &[level(StrategyId::Balanced, "tick")];

/// Adds its step to what the slot holds.
#[derive(Default)]
pub struct TickLane;

impl Lane for TickLane {
    type Family = TickFamily;
    type Inputs<'f> = TickInputs;

    fn levels(&self) -> &[Level] {
        TICK_LEVELS
    }

    fn cost(&self, _level: usize, _inputs: &TickInputs) -> Cost {
        Cost::default()
    }

    fn record(
        &mut self,
        _level: usize,
        inputs: &mut TickInputs,
        out: &mut TickOutputs,
    ) -> Result<LaneReport, LaneError> {
        out.advanced.0 += inputs.step.0;
        Ok(LaneReport::default())
    }
}

fn tick_lane() -> Box<dyn AnyLane> {
    Box::new(Erased::new(TickLane))
}

/// The tick family's registration, at `fixed_step`.
fn tick_registration(fixed_step: Option<Duration>) -> AgentRegistration {
    AgentRegistration {
        family: TypeId::of::<TickFamily>(),
        family_name: TickFamily::NAME,
        kind: TickFamily::KIND,
        agent: TickFamily::AGENT,
        semantics: TickFamily::SEMANTICS,
        phase: ExecutionPhase::TRANSFORM,
        importance: AgentImportance::Critical,
        after: &[],
        modes: &[],
        fixed_step,
        lanes: &[tick_lane],
    }
}

/// The control case: a fixed-step family reading its step boots.
#[test]
fn boot_accepts_a_fixed_step_family_reading_its_step() {
    let tick = tick_registration(Some(Duration::from_millis(10)));

    let checked = check_registrations(&[&tick], &Runtime::new(), &DeviceCaps::default());

    assert_eq!(checked, Ok(()));
}

/// **A lane reading the fixed step needs a fixed-step agent.** Only a
/// fixed-step agent supplies `FixedStep`; a family that runs every frame
/// would have its lane refused on every frame (`Reason::Missing`), as a lane
/// reading a resource the runtime lacks would. Boot refuses that one once
/// (`UnknownResource`), and must refuse this one too.
#[test]
fn boot_refuses_a_lane_reading_the_fixed_step_of_a_family_without_one() {
    let tick = tick_registration(None);

    let checked = check_registrations(&[&tick], &Runtime::new(), &DeviceCaps::default());

    assert!(
        checked.is_err(),
        "a family with no fixed step whose lane reads `FixedStep` boots, and is refused every frame"
    );
}

/// A source with no step refuses the lane, naming the field; one given the
/// agent's step hands that step to the lane.
#[test]
fn the_fixed_step_reaches_the_lane_only_when_its_agent_gives_it() {
    let bus = LaneBus::new();
    let runtime = Runtime::new();
    let mut lane = Erased::new(TickLane);
    let mut gpu = GpuSetup::headless();
    let step = Duration::from_nanos(7_812_500);

    let mut deck = OutputDeck::new();
    let without = {
        let mut source = InputSource::new(FrameId(1), &bus, &runtime);
        let mut sink = OutputSink::new(&mut deck);
        lane.run(0, &mut source, &mut sink, &mut gpu)
    };
    match without.outcome {
        RunOutcome::Refused(refusal) => {
            assert_eq!(refusal.field, "step");
            assert_eq!(refusal.source, SourceKind::Frame);
            assert_eq!(refusal.reason, Reason::Missing);
        }
        other => panic!("a source with no step refuses the lane, got {other:?}"),
    }
    assert!(deck.get::<Advanced>().is_none(), "nothing was committed");

    let with = {
        let mut source = InputSource::new(FrameId(1), &bus, &runtime).with_step(FixedStep(step));
        let mut sink = OutputSink::new(&mut deck);
        lane.run(0, &mut source, &mut sink, &mut gpu)
    };
    assert!(
        matches!(with.outcome, RunOutcome::Recorded(_)),
        "{:?}",
        with.outcome
    );
    assert_eq!(deck.get::<Advanced>(), Some(&Advanced(step)));
}
