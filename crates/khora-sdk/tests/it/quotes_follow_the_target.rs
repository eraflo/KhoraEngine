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

//! A family's lanes that do not run are quoted again when the frame's target
//! changes its key: a new colour format, depth format or sample count may
//! price every lane differently. While the key holds, their quotes stand.
//!
//! The frame's target reaches a `LaneAgent` through the `FrameContext`, which
//! only this crate's tests can build (it takes a tokio handle), so this lives
//! here rather than beside the other quote tests in khora-agents. The probe
//! lanes are run directly through `LaneAgent`, never submitted to the
//! registry, so no engine of this binary boots them.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

use khora_agents::agent::{lane_agent, LaneAgent};
use khora_sdk::khora_core::agent::gorna::{AgentId, ResourceBudget, StrategyId};
use khora_sdk::khora_core::agent::registration::AgentRegistration;
use khora_sdk::khora_core::agent::{Agent, AgentImportance, ExecutionPhase};
use khora_sdk::khora_core::gpu::DeviceCaps;
use khora_sdk::khora_core::lane::contract::{Lane, LaneReport};
use khora_sdk::khora_core::lane::cost::{Cost, PerDevice};
use khora_sdk::khora_core::lane::family::{LaneFamily, Semantics};
use khora_sdk::khora_core::lane::guard::{InputRefusal, OutputRefusal, SinkKind, SourceKind};
use khora_sdk::khora_core::lane::io::{
    Declaration, FrameId, InputSource, LaneInputs, LaneOutputs, OutputSink, Target, TargetKey,
};
use khora_sdk::khora_core::lane::level::{Impact, Level};
use khora_sdk::khora_core::lane::{LaneBus, LaneError, LaneKind, OutputDeck};
use khora_sdk::khora_core::math::Extent2D;
use khora_sdk::khora_core::renderer::api::frame::FrameContext;
use khora_sdk::khora_core::renderer::api::resource::{TextureFormat, TextureViewId};
use khora_sdk::khora_core::{EngineContext, Runtime, WorldAccess};

use crate::render_doubles::insert_frame_context;

// ─── The family ─────────────────────────────────────────────────────────────

/// What the drawing lane drew: how many frames.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Drawn(pub u32);

/// The family's outputs.
pub struct DrawnOutputs {
    drawn: Drawn,
}

impl<'f> LaneOutputs<'f> for DrawnOutputs {
    fn declare(d: &mut Declaration) {
        d.write::<Drawn>("drawn", SinkKind::Deck);
    }

    fn open(_sink: &mut OutputSink<'f>) -> Result<Self, LaneError> {
        Ok(Self {
            drawn: Drawn::default(),
        })
    }

    fn commit(self, sink: &mut OutputSink<'f>) -> Result<(), OutputRefusal> {
        sink.put("drawn", self.drawn)
    }
}

/// Two lanes drawing into the frame's target.
pub struct TargetFamily;

impl LaneFamily for TargetFamily {
    const KIND: LaneKind = LaneKind::Render;
    const NAME: &'static str = "target quotes";
    const AGENT: AgentId = AgentId::new("renderer");
    const SEMANTICS: Semantics = Semantics::Presentation;
    type Outputs<'f> = DrawnOutputs;
}

/// How many times the waiting lane was costed: once per quote.
#[derive(Debug, Default)]
pub struct WaitingQuotes(pub Arc<AtomicUsize>);

/// The frame's target, as `#[frame] target: Target` derives it.
pub struct TargetInput {
    target: Target,
}

impl<'f> LaneInputs<'f> for TargetInput {
    fn declare(d: &mut Declaration) {
        d.read::<Target>("target", SourceKind::Frame);
    }

    fn fetch(src: &mut InputSource<'f>) -> Result<Self, InputRefusal> {
        Ok(Self {
            target: src.target("target")?,
        })
    }

    fn target_key(&self) -> Option<TargetKey> {
        Some(self.target.key)
    }
}

/// The frame's target, and the count the waiting lane's quotes go to.
pub struct CountedTarget<'f> {
    target: Target,
    quotes: &'f WaitingQuotes,
}

impl<'f> LaneInputs<'f> for CountedTarget<'f> {
    fn declare(d: &mut Declaration) {
        d.read::<Target>("target", SourceKind::Frame);
        d.read::<WaitingQuotes>("quotes", SourceKind::Resource);
    }

    fn fetch(src: &mut InputSource<'f>) -> Result<Self, InputRefusal> {
        Ok(Self {
            target: src.target("target")?,
            quotes: src.entry::<WaitingQuotes>("quotes", SourceKind::Resource)?,
        })
    }

    fn target_key(&self) -> Option<TargetKey> {
        Some(self.target.key)
    }
}

const fn level(id: StrategyId, name: &'static str) -> Level {
    Level {
        id,
        name,
        impact: Impact::None,
        memory: PerDevice {
            cpu: 0,
            gpu: 0,
            npu: 0,
            other: 0,
        },
    }
}

const DRAWING_LEVELS: &[Level] = &[level(StrategyId::Balanced, "drawing")];
const WAITING_LEVELS: &[Level] = &[level(StrategyId::LowPower, "waiting")];

/// A host-only cost of `micros`: the same on every frame, so no quote ever
/// drifts and only the target can call for a new one.
fn host(micros: u64) -> Cost {
    Cost {
        time: PerDevice {
            cpu: Duration::from_micros(micros),
            ..PerDevice::default()
        },
        ..Cost::default()
    }
}

/// The lane that runs.
#[derive(Default)]
pub struct DrawingLane;

impl Lane for DrawingLane {
    type Family = TargetFamily;
    type Inputs<'f> = TargetInput;

    fn levels(&self) -> &[Level] {
        DRAWING_LEVELS
    }

    fn cost(&self, _level: usize, _inputs: &TargetInput) -> Cost {
        host(100)
    }

    fn record(
        &mut self,
        _level: usize,
        _inputs: &mut TargetInput,
        out: &mut DrawnOutputs,
    ) -> Result<LaneReport, LaneError> {
        out.drawn = Drawn(1);
        Ok(LaneReport {
            done: 1.0,
            stalled: false,
        })
    }
}

/// The lane that never runs, counting its quotes.
#[derive(Default)]
pub struct WaitingLane;

impl Lane for WaitingLane {
    type Family = TargetFamily;
    type Inputs<'f> = CountedTarget<'f>;

    fn levels(&self) -> &[Level] {
        WAITING_LEVELS
    }

    fn cost(&self, _level: usize, inputs: &CountedTarget<'_>) -> Cost {
        inputs.quotes.0.fetch_add(1, Ordering::SeqCst);
        host(50)
    }

    fn record(
        &mut self,
        _level: usize,
        _inputs: &mut CountedTarget<'_>,
        out: &mut DrawnOutputs,
    ) -> Result<LaneReport, LaneError> {
        out.drawn = Drawn(2);
        Ok(LaneReport {
            done: 1.0,
            stalled: false,
        })
    }
}

static DRAWING_AND_WAITING: AgentRegistration = khora_sdk::khora_core::register_agent! {
    family: TargetFamily,
    phase: ExecutionPhase::OUTPUT,
    importance: AgentImportance::Important,
    after: [],
    modes: [],
    fixed_step: None,
    lanes: [DrawingLane, WaitingLane],
};

/// Runs `agent` once for `frame`, as the scheduler runs it: world-free, with
/// its own declaration as the permit, committing into `deck`.
fn run(agent: &mut LaneAgent, frame: u64, runtime: &Arc<Runtime>, deck: &mut OutputDeck) {
    let mut bus = LaneBus::new();
    bus.set_frame(FrameId(frame));
    let permit = agent.contention();
    let id = agent.id();
    let mut context = EngineContext::for_agent(
        WorldAccess::None,
        Arc::clone(runtime),
        &bus,
        deck,
        &permit,
        Some(id),
    );
    agent.execute(&mut context);
}

/// A 640×480 target in `color`, with a 32-bit float depth.
fn target(color: TextureFormat) -> Target {
    let depth = TextureFormat::Depth32Float;
    Target {
        color: TextureViewId(1),
        depth: Some(TextureViewId(2)),
        size: Extent2D {
            width: 640,
            height: 480,
        },
        key: TargetKey {
            color,
            depth: Some(depth),
            samples: 1,
        },
    }
}

/// **A new target key quotes every row again.** The drawing lane runs three
/// frames on one target, its quote never moving; the waiting lane is quoted
/// by the first run only. The target then turns to a 16-bit float colour: the
/// next run quotes the waiting lane again — and a further run on that same
/// key does not.
#[test]
fn a_target_key_change_requotes_the_other_lanes() {
    let mut runtime = Runtime::new();
    let _tokio = insert_frame_context(&mut runtime);
    let quotes = Arc::new(AtomicUsize::new(0));
    runtime.resources.insert(WaitingQuotes(Arc::clone(&quotes)));
    let runtime = Arc::new(runtime);
    let context = Arc::clone(
        runtime
            .resources
            .get::<Arc<FrameContext>>()
            .expect("the frame context"),
    );
    let count = || quotes.load(Ordering::SeqCst);
    let mut agent = lane_agent(&DRAWING_AND_WAITING, &DeviceCaps::default());
    agent.apply_budget(ResourceBudget {
        strategy_id: StrategyId::Balanced,
        time_limit: Duration::from_millis(4),
        memory_limit: None,
        extra_params: Default::default(),
    });
    let mut deck = OutputDeck::new();

    context.insert(target(TextureFormat::Rgba8UnormSrgb));
    for frame in 1..=3 {
        run(&mut agent, frame, &runtime, &mut deck);
    }
    assert_eq!(
        deck.get::<Drawn>(),
        Some(&Drawn(1)),
        "the drawing lane ran; status: {:?}",
        agent.report_status().message
    );
    assert_eq!(count(), 1, "one target: quoted by the first run only");

    context.insert(target(TextureFormat::Rgba16Float));
    run(&mut agent, 4, &runtime, &mut deck);
    assert_eq!(count(), 2, "a new key quotes the waiting lane again");

    run(&mut agent, 5, &runtime, &mut deck);
    assert_eq!(count(), 2, "the new key holds: its quote stands");
}
