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

//! The agent lends a `#[pass]` output the run's pipeline system beside the
//! device: the one the runtime holds, the one the lane was prepared with. A
//! lane resolving a draw's pipeline while it records reaches it through the
//! pass, and declares no pipeline system of its own.

use std::sync::Arc;

use khora_agents::agent::lane_agent;
use khora_core::agent::gorna::{AgentId, StrategyId};
use khora_core::agent::registration::AgentRegistration;
use khora_core::agent::{AgentImportance, ExecutionPhase};
use khora_core::gpu::DeviceCaps;
use khora_core::lane::compile_check::PassField;
use khora_core::lane::contract::{Lane, LaneReport};
use khora_core::lane::cost::{Cost, PerDevice};
use khora_core::lane::family::{LaneFamily, Semantics};
use khora_core::lane::guard::{InputRefusal, OutputRefusal, SinkKind};
use khora_core::lane::io::{
    Declaration, FrameId, InputSource, LaneInputs, LaneOutputs, OutputSink, Pass, PassSlot,
};
use khora_core::lane::level::{Impact, Level};
use khora_core::lane::{LaneBus, LaneError, LaneKind, OutputDeck};
use khora_core::renderer::api::command::CommandBufferId;
use khora_core::renderer::traits::PipelineSystem;

use crate::shadows_family::fixtures::{run_once, shadows_runtime, SubmitLog};

/// A pass slot holding the buffer its pass finished into.
#[derive(Debug, Default, PartialEq)]
pub struct LentPass(pub Option<CommandBufferId>);

impl PassSlot for LentPass {
    type Descriptor = ();

    fn descriptor() {}

    fn close(_descriptor: (), command_buffer: CommandBufferId) -> Self {
        LentPass(Some(command_buffer))
    }
}

/// The address of the pipeline system the pass lent, `None` when it lent
/// none.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct PipelinesSeen(pub Option<usize>);

/// The family's outputs: one pass, and what it lent — written out as
/// `#[derive(LaneOutputs)]` with `#[pass] pass` and `#[deck] seen` derives
/// them (this crate's tests do not depend on khora-macros).
pub struct LentOutputs<'f> {
    pub pass: Pass<'f, LentPass>,
    pub seen: PipelinesSeen,
}

impl<'f> LaneOutputs<'f> for LentOutputs<'f> {
    fn declare(d: &mut Declaration) {
        <Pass<'f, LentPass> as PassField<'f>>::declare(d, "pass");
        d.write::<PipelinesSeen>("seen", SinkKind::Deck);
    }

    fn open(sink: &mut OutputSink<'f>) -> Result<Self, LaneError> {
        Ok(Self {
            pass: <Pass<'f, LentPass> as PassField<'f>>::open(sink, "pass")?,
            seen: PipelinesSeen::default(),
        })
    }

    fn commit(self, sink: &mut OutputSink<'f>) -> Result<(), OutputRefusal> {
        self.pass.commit(sink, "pass")?;
        sink.put("seen", self.seen)
    }
}

/// A family recording one pass.
pub struct LentFamily;

impl LaneFamily for LentFamily {
    const KIND: LaneKind = LaneKind::Render;
    const NAME: &'static str = "lent pipelines";
    const AGENT: AgentId = AgentId::new("lent pipelines");
    const SEMANTICS: Semantics = Semantics::Presentation;
    type Outputs<'f> = LentOutputs<'f>;
}

/// The lane reads nothing.
pub struct Nothing;

impl<'f> LaneInputs<'f> for Nothing {
    fn declare(_d: &mut Declaration) {}

    fn fetch(_src: &mut InputSource<'f>) -> Result<Self, InputRefusal> {
        Ok(Nothing)
    }
}

const LEVELS: &[Level] = &[Level {
    id: StrategyId::Balanced,
    name: "one pass",
    impact: Impact::None,
    memory: PerDevice {
        cpu: 0,
        gpu: 0,
        npu: 0,
        other: 0,
    },
}];

/// Writes down the pipeline system its pass lent.
#[derive(Default)]
pub struct LookingLane;

impl Lane for LookingLane {
    type Family = LentFamily;
    type Inputs<'f> = Nothing;

    fn levels(&self) -> &[Level] {
        LEVELS
    }

    fn cost(&self, _level: usize, _inputs: &Nothing) -> Cost {
        Cost::default()
    }

    fn record(
        &mut self,
        _level: usize,
        _inputs: &mut Nothing,
        out: &mut LentOutputs<'_>,
    ) -> Result<LaneReport, LaneError> {
        out.seen = PipelinesSeen(
            out.pass
                .pipelines()
                .map(|pipelines| pipelines as *const dyn PipelineSystem as *const () as usize),
        );
        Ok(LaneReport {
            done: 1.0,
            stalled: false,
            message: None,
        })
    }
}

static LENT: AgentRegistration = khora_core::register_agent! {
    family: LentFamily,
    phase: ExecutionPhase::OUTPUT,
    importance: AgentImportance::Important,
    after: [],
    modes: [],
    fixed_step: None,
    lanes: [LookingLane],
};

/// **The agent lends a pass the runtime's pipeline system.** Run once on a
/// runtime holding a device and a pipeline system, the lane's pass hands it
/// that very pipeline system, and the pass closes into its slot.
#[test]
fn the_agent_lends_a_pass_the_runtimes_pipeline_system() {
    let device = Arc::new(SubmitLog::new());
    let runtime = Arc::new(shadows_runtime(&device));
    let ours = runtime
        .resources
        .get::<Arc<dyn PipelineSystem>>()
        .map(|pipelines| Arc::as_ptr(pipelines) as *const () as usize)
        .expect("the runtime holds a pipeline system");
    let mut agent = lane_agent(&LENT, &DeviceCaps::default());
    let mut bus = LaneBus::new();
    bus.set_frame(FrameId(1));
    let mut deck = OutputDeck::new();

    run_once(&mut agent, &runtime, &bus, &mut deck);

    assert_eq!(
        deck.get::<PipelinesSeen>(),
        Some(&PipelinesSeen(Some(ours))),
        "the pass lent the runtime's pipeline system"
    );
    assert!(
        deck.get::<LentPass>().is_some_and(|pass| pass.0.is_some()),
        "the pass closed into its slot"
    );
}
