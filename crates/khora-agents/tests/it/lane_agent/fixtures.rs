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

//! The runtime entries, families, lanes and registrations the `LaneAgent`
//! tests share, and the frame they run an agent through.
//!
//! khora-agents does not depend on khora-macros, so the lane inputs and
//! outputs implement their traits by hand, as the derives would.
//!
//! **A frame is the scheduler's.** A `LaneAgent` runs as layers: [`run`]
//! goes through the core `Place` frame logic exactly as the scheduler does —
//! the frame start swaps a staged layer in; the event pass wakes the agent,
//! only if its place holds a wake; then the installed layer runs, on its own
//! contention. So a wake a run raises is answered in the next frame's event
//! pass, and a layer the agent stages there runs from the frame after; a
//! layer staged into an empty place (the first frame) runs at once. An
//! allocation lent between two frames ([`lend`], `apply`, `apply_budget`) is
//! answered in the next frame's event pass.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use khora_agents::agent::LaneAgent;
use khora_core::agent::gorna::protocol::{Envelope, Epoch, OfferRequest, Report};
use khora_core::agent::gorna::{
    AgentId, NegotiationRequest, ResourceBudget, ResourceConstraints, StrategyId,
};
use khora_core::agent::registration::AgentRegistration;
use khora_core::agent::{
    Agent, AgentImportance, EngineMode, ExecutionPhase, ExecutionTiming, Place, Wake,
};
use khora_core::gpu::{DeviceCaps, GpuSetup};
use khora_core::lane::contract::{Lane, LaneReport};
use khora_core::lane::cost::{Cost, PerDevice};
use khora_core::lane::family::{LaneFamily, Semantics};
use khora_core::lane::guard::{InputRefusal, OutputRefusal, SinkKind, SourceKind};
use khora_core::lane::io::{
    Declaration, FrameId, InputSource, LaneInputs, LaneOutputs, Locked, OutputSink,
};
use khora_core::lane::level::{Impact, Level};
use khora_core::lane::{LaneBus, LaneError, LaneKind, OutputDeck};
use khora_core::{EngineContext, Runtime, WorldAccess};

// ─── Runtime entries ────────────────────────────────────────────────────────

/// How much work the frame holds: every level's cost scales with it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Load(pub u32);

/// What the ladder lane is told to do this frame.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Plan {
    /// The levels whose record fails.
    pub fail_levels: Vec<usize>,
    /// Whether the record reports it could not progress.
    pub stall: bool,
}

/// A runtime entry registered as `Arc<Mutex<Tally>>`, locked by the second
/// lane.
#[derive(Debug, Default)]
pub struct Tally(pub u32);

/// A runtime holding `load` and `plan`, and the tally.
pub fn runtime(load: u32, plan: Plan) -> Arc<Runtime> {
    let mut runtime = Runtime::new();
    runtime.resources.insert(Load(load));
    runtime.resources.insert(plan);
    runtime
        .resources
        .insert(Arc::new(Mutex::new(Tally::default())));
    Arc::new(runtime)
}

// ─── The family's output ────────────────────────────────────────────────────

/// What a lane recorded: which lane, at which level, its how-many-th record.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Recorded {
    pub lane: &'static str,
    pub level: usize,
    pub record: u32,
}

/// Every probe family's outputs: one deck slot.
pub struct ProbeOutputs {
    pub recorded: Recorded,
}

impl<'f> LaneOutputs<'f> for ProbeOutputs {
    fn declare(d: &mut Declaration) {
        d.write::<Recorded>("recorded", SinkKind::Deck);
    }

    fn open(_sink: &mut OutputSink<'f>) -> Result<Self, LaneError> {
        Ok(Self {
            recorded: Recorded::default(),
        })
    }

    fn commit(self, sink: &mut OutputSink<'f>) -> Result<(), OutputRefusal> {
        sink.put("recorded", self.recorded)
    }
}

// ─── Inputs ─────────────────────────────────────────────────────────────────

/// Nothing at all.
pub struct NoInputs;

impl<'f> LaneInputs<'f> for NoInputs {
    fn declare(_d: &mut Declaration) {}

    fn fetch(_src: &mut InputSource<'f>) -> Result<Self, InputRefusal> {
        Ok(NoInputs)
    }
}

/// The load and the plan, both read without locking.
pub struct LoadAndPlan<'f> {
    pub load: &'f Load,
    pub plan: &'f Plan,
}

impl<'f> LaneInputs<'f> for LoadAndPlan<'f> {
    fn declare(d: &mut Declaration) {
        d.read::<Load>("load", SourceKind::Resource);
        d.read::<Plan>("plan", SourceKind::Resource);
    }

    fn fetch(src: &mut InputSource<'f>) -> Result<Self, InputRefusal> {
        Ok(Self {
            load: src.entry::<Load>("load", SourceKind::Resource)?,
            plan: src.entry::<Plan>("plan", SourceKind::Resource)?,
        })
    }
}

/// The load, read without locking.
pub struct LoadOnly<'f> {
    pub load: &'f Load,
}

impl<'f> LaneInputs<'f> for LoadOnly<'f> {
    fn declare(d: &mut Declaration) {
        d.read::<Load>("load", SourceKind::Resource);
    }

    fn fetch(src: &mut InputSource<'f>) -> Result<Self, InputRefusal> {
        Ok(Self {
            load: src.entry::<Load>("load", SourceKind::Resource)?,
        })
    }
}

/// The tally, locked.
pub struct TallyLock<'f> {
    pub tally: Locked<'f, Tally>,
}

impl<'f> LaneInputs<'f> for TallyLock<'f> {
    fn declare(d: &mut Declaration) {
        d.read::<Arc<Mutex<Tally>>>("tally", SourceKind::Locked);
    }

    fn fetch(src: &mut InputSource<'f>) -> Result<Self, InputRefusal> {
        Ok(Self {
            tally: src.lock::<Tally>("tally")?,
        })
    }
}

// ─── Families ───────────────────────────────────────────────────────────────

/// The family most tests run.
pub struct ProbeFamily;

impl LaneFamily for ProbeFamily {
    const KIND: LaneKind = LaneKind::Audio;
    const NAME: &'static str = "probe";
    const AGENT: AgentId = AgentId::new("audio");
    const SEMANTICS: Semantics = Semantics::Presentation;
    type Outputs<'f> = ProbeOutputs;
}

/// A second family, so two registrations can boot together.
pub struct OtherFamily;

impl LaneFamily for OtherFamily {
    const KIND: LaneKind = LaneKind::Ui;
    const NAME: &'static str = "other";
    const AGENT: AgentId = AgentId::new("ui");
    const SEMANTICS: Semantics = Semantics::Presentation;
    type Outputs<'f> = ProbeOutputs;
}

/// The family whose lanes count their shutdowns.
pub struct ReleasingFamily;

impl LaneFamily for ReleasingFamily {
    const KIND: LaneKind = LaneKind::Render;
    const NAME: &'static str = "releasing";
    const AGENT: AgentId = AgentId::new("renderer");
    const SEMANTICS: Semantics = Semantics::Presentation;
    type Outputs<'f> = ProbeOutputs;
}

// ─── Levels ─────────────────────────────────────────────────────────────────

/// A level holding nothing, named by `id`.
pub const fn level(id: StrategyId, name: &'static str) -> Level {
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

/// The ladder's three levels, cheapest first.
pub const LADDER_LEVELS: &[Level] = &[
    level(StrategyId::LowPower, "low"),
    level(StrategyId::Balanced, "mid"),
    level(StrategyId::HighPerformance, "high"),
];

const FIRST_LEVELS: &[Level] = &[level(StrategyId::LowPower, "first")];
const SECOND_LEVELS: &[Level] = &[level(StrategyId::Balanced, "second")];
const STORAGE_LEVELS: &[Level] = &[level(StrategyId::HighPerformance, "storage")];
const RELEASING_A_LEVELS: &[Level] = &[level(StrategyId::LowPower, "a")];
const RELEASING_B_LEVELS: &[Level] = &[level(StrategyId::Balanced, "b")];

/// What level `level` of the ladder costs on `load`: `load × (level + 1)`
/// microseconds on the host, a thousand bytes per unit on the graphics device,
/// nothing anywhere else.
pub fn ladder_cost(load: u32, level: usize) -> Cost {
    let units = u64::from(load) * (level as u64 + 1);
    Cost {
        time: PerDevice {
            cpu: Duration::from_micros(units),
            ..PerDevice::default()
        },
        memory: PerDevice {
            gpu: units * 1000,
            ..PerDevice::default()
        },
        ..Cost::default()
    }
}

// ─── Lanes ──────────────────────────────────────────────────────────────────

/// Three levels; fails or stalls as the plan says; counts its records.
#[derive(Default)]
pub struct LadderLane {
    records: u32,
}

impl Lane for LadderLane {
    type Family = ProbeFamily;
    type Inputs<'f> = LoadAndPlan<'f>;

    fn levels(&self) -> &[Level] {
        LADDER_LEVELS
    }

    fn cost(&self, level: usize, inputs: &LoadAndPlan<'_>) -> Cost {
        ladder_cost(inputs.load.0, level)
    }

    fn record(
        &mut self,
        level: usize,
        inputs: &mut LoadAndPlan<'_>,
        out: &mut ProbeOutputs,
    ) -> Result<LaneReport, LaneError> {
        if inputs.plan.fail_levels.contains(&level) {
            return Err(LaneError::InvalidContext {
                expected: "a level the plan lets record",
                received: format!("level {level}, which the plan fails"),
            });
        }
        self.records += 1;
        out.recorded = Recorded {
            lane: "ladder",
            level,
            record: self.records,
        };
        Ok(LaneReport {
            done: 1.0,
            stalled: inputs.plan.stall,
        })
    }
}

/// One `LowPower` level, reading the load.
#[derive(Default)]
pub struct FirstLane;

impl Lane for FirstLane {
    type Family = ProbeFamily;
    type Inputs<'f> = LoadOnly<'f>;

    fn levels(&self) -> &[Level] {
        FIRST_LEVELS
    }

    fn cost(&self, _level: usize, _inputs: &LoadOnly<'_>) -> Cost {
        Cost::default()
    }

    fn record(
        &mut self,
        level: usize,
        inputs: &mut LoadOnly<'_>,
        out: &mut ProbeOutputs,
    ) -> Result<LaneReport, LaneError> {
        out.recorded = Recorded {
            lane: "first",
            level,
            record: inputs.load.0,
        };
        Ok(LaneReport::default())
    }
}

/// One `Balanced` level, locking the tally.
#[derive(Default)]
pub struct SecondLane;

impl Lane for SecondLane {
    type Family = ProbeFamily;
    type Inputs<'f> = TallyLock<'f>;

    fn levels(&self) -> &[Level] {
        SECOND_LEVELS
    }

    fn cost(&self, _level: usize, _inputs: &TallyLock<'_>) -> Cost {
        Cost::default()
    }

    fn record(
        &mut self,
        level: usize,
        inputs: &mut TallyLock<'_>,
        out: &mut ProbeOutputs,
    ) -> Result<LaneReport, LaneError> {
        inputs.tally.0 += 1;
        out.recorded = Recorded {
            lane: "second",
            level,
            record: inputs.tally.0,
        };
        Ok(LaneReport::default())
    }
}

/// One `HighPerformance` level, on a device with storage buffers only.
#[derive(Default)]
pub struct StorageLane;

impl Lane for StorageLane {
    type Family = ProbeFamily;
    type Inputs<'f> = NoInputs;

    fn levels(&self) -> &[Level] {
        STORAGE_LEVELS
    }

    fn supports(&self, caps: &DeviceCaps) -> bool {
        caps.storage_buffers
    }

    fn cost(&self, _level: usize, _inputs: &NoInputs) -> Cost {
        Cost::default()
    }

    fn record(
        &mut self,
        level: usize,
        _inputs: &mut NoInputs,
        out: &mut ProbeOutputs,
    ) -> Result<LaneReport, LaneError> {
        out.recorded = Recorded {
            lane: "storage",
            level,
            record: 1,
        };
        Ok(LaneReport::default())
    }
}

/// Numbers the releasing lanes' instances, from 1.
static INSTANCES: AtomicU32 = AtomicU32::new(1);

/// Every releasing lane instance shut down so far: its lane, and its number.
/// Only the shutdown test builds these lanes, so nothing else adds to it.
static RELEASED: Mutex<Vec<(&'static str, u32)>> = Mutex::new(Vec::new());

/// Every releasing lane instance shut down so far, in order.
pub fn released() -> Vec<(&'static str, u32)> {
    RELEASED
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .clone()
}

fn release(lane: &'static str, instance: u32) {
    RELEASED
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .push((lane, instance));
}

/// Records its instance's number; logs its shutdown in [`released`].
pub struct ReleasingLaneA {
    instance: u32,
}

impl Default for ReleasingLaneA {
    fn default() -> Self {
        Self {
            instance: INSTANCES.fetch_add(1, Ordering::SeqCst),
        }
    }
}

impl Lane for ReleasingLaneA {
    type Family = ReleasingFamily;
    type Inputs<'f> = NoInputs;

    fn levels(&self) -> &[Level] {
        RELEASING_A_LEVELS
    }

    fn cost(&self, _level: usize, _inputs: &NoInputs) -> Cost {
        Cost::default()
    }

    fn record(
        &mut self,
        level: usize,
        _inputs: &mut NoInputs,
        out: &mut ProbeOutputs,
    ) -> Result<LaneReport, LaneError> {
        out.recorded = Recorded {
            lane: "a",
            level,
            record: self.instance,
        };
        Ok(LaneReport::default())
    }

    fn shutdown(&mut self, _gpu: &mut GpuSetup<'_>) {
        release("a", self.instance);
    }
}

/// Records its instance's number; logs its shutdown in [`released`].
pub struct ReleasingLaneB {
    instance: u32,
}

impl Default for ReleasingLaneB {
    fn default() -> Self {
        Self {
            instance: INSTANCES.fetch_add(1, Ordering::SeqCst),
        }
    }
}

impl Lane for ReleasingLaneB {
    type Family = ReleasingFamily;
    type Inputs<'f> = NoInputs;

    fn levels(&self) -> &[Level] {
        RELEASING_B_LEVELS
    }

    fn cost(&self, _level: usize, _inputs: &NoInputs) -> Cost {
        Cost::default()
    }

    fn record(
        &mut self,
        level: usize,
        _inputs: &mut NoInputs,
        out: &mut ProbeOutputs,
    ) -> Result<LaneReport, LaneError> {
        out.recorded = Recorded {
            lane: "b",
            level,
            record: self.instance,
        };
        Ok(LaneReport::default())
    }

    fn shutdown(&mut self, _gpu: &mut GpuSetup<'_>) {
        release("b", self.instance);
    }
}

// ─── Registrations ──────────────────────────────────────────────────────────

/// The ladder, every frame, in `TRANSFORM`.
pub static LADDER: AgentRegistration = khora_core::register_agent! {
    family: ProbeFamily,
    phase: ExecutionPhase::TRANSFORM,
    importance: AgentImportance::Important,
    after: [],
    modes: [],
    fixed_step: None,
    lanes: [LadderLane],
};

/// The step [`FIXED_LADDER`] runs at.
pub const FIXED_STEP: Duration = Duration::from_micros(16_667);

/// The ladder at a fixed step, critical, after physics, while playing.
pub static FIXED_LADDER: AgentRegistration = khora_core::register_agent! {
    family: ProbeFamily,
    phase: ExecutionPhase::TRANSFORM,
    importance: AgentImportance::Critical,
    after: [AgentId::new("physics")],
    modes: [EngineMode::Playing],
    fixed_step: Some(FIXED_STEP),
    lanes: [LadderLane],
};

/// Two lanes reading different things: the load, then the tally's lock.
pub static TWO_LANES: AgentRegistration = khora_core::register_agent! {
    family: ProbeFamily,
    phase: ExecutionPhase::OUTPUT,
    importance: AgentImportance::Important,
    after: [],
    modes: [],
    fixed_step: None,
    lanes: [FirstLane, SecondLane],
};

/// A lane needing storage buffers, then one running anywhere.
pub static STORAGE_OR_PLAIN: AgentRegistration = khora_core::register_agent! {
    family: ProbeFamily,
    phase: ExecutionPhase::OUTPUT,
    importance: AgentImportance::Important,
    after: [],
    modes: [],
    fixed_step: None,
    lanes: [StorageLane, FirstLane],
};

/// Two lanes counting their shutdowns.
pub static RELEASING: AgentRegistration = khora_core::register_agent! {
    family: ReleasingFamily,
    phase: ExecutionPhase::OUTPUT,
    importance: AgentImportance::Important,
    after: [],
    modes: [],
    fixed_step: None,
    lanes: [ReleasingLaneA, ReleasingLaneB],
};

/// The other family, reading the load.
pub static OTHER: AgentRegistration = khora_core::register_agent! {
    family: OtherFamily,
    phase: ExecutionPhase::OUTPUT,
    importance: AgentImportance::Important,
    after: [],
    modes: [],
    fixed_step: None,
    lanes: [OtherLane],
};

/// The other family's one lane, reading the load.
#[derive(Default)]
pub struct OtherLane;

const OTHER_LEVELS: &[Level] = &[level(StrategyId::Balanced, "other")];

impl Lane for OtherLane {
    type Family = OtherFamily;
    type Inputs<'f> = LoadOnly<'f>;

    fn levels(&self) -> &[Level] {
        OTHER_LEVELS
    }

    fn cost(&self, _level: usize, _inputs: &LoadOnly<'_>) -> Cost {
        Cost::default()
    }

    fn record(
        &mut self,
        level: usize,
        _inputs: &mut LoadOnly<'_>,
        out: &mut ProbeOutputs,
    ) -> Result<LaneReport, LaneError> {
        out.recorded = Recorded {
            lane: "other",
            level,
            record: 1,
        };
        Ok(LaneReport::default())
    }
}

// ─── Driving an agent ───────────────────────────────────────────────────────

/// A budget naming `strategy`.
pub fn budget(strategy: StrategyId) -> ResourceBudget {
    ResourceBudget {
        strategy_id: strategy,
        time_limit: Duration::from_millis(4),
        memory_limit: None,
        extra_params: HashMap::new(),
    }
}

/// A negotiation request with a 60 Hz target and no constraint.
pub fn request() -> NegotiationRequest {
    NegotiationRequest {
        target_latency: Duration::from_micros(16_667),
        priority_weight: 1.0,
        constraints: ResourceConstraints::default(),
        current_mode: EngineMode::Playing,
        agent_timing: ExecutionTiming::default(),
    }
}

/// Runs one frame of `agent` as the scheduler does, world-free, committing
/// into `deck`; the report of its run, `None` when nothing ran.
pub fn run(
    agent: &mut LaneAgent,
    frame: u64,
    runtime: &Arc<Runtime>,
    deck: &mut OutputDeck,
) -> Option<Report> {
    run_sub_steps(agent, frame, runtime, deck, 1)
        .pop()
        .flatten()
}

/// Runs one frame of `agent` as the scheduler does, its installed layer run
/// `steps` times as a fixed-step family's sub-steps are: the frame start and
/// the event pass happen once, before the first. The report of each run.
///
/// 1. **Frame start**: the place swaps its staged layer in.
/// 2. **Event pass**: if the place holds wakes, the agent is woken for them,
///    with the permit of its own contention (what it quotes and prepares
///    with) and its place locked.
/// 3. **Waves**: the installed layer runs on the permit of its contention.
pub fn run_sub_steps(
    agent: &mut LaneAgent,
    frame: u64,
    runtime: &Arc<Runtime>,
    deck: &mut OutputDeck,
    steps: usize,
) -> Vec<Option<Report>> {
    let place = place_of(agent);
    let mut bus = LaneBus::new();
    bus.set_frame(FrameId(frame));
    let id = agent.id();

    locked(&place).start_frame(&mut GpuSetup::headless());

    let why = locked(&place).take_wakes();
    if !why.is_empty() {
        let permit = agent.contention();
        let mut context = EngineContext::for_agent(
            WorldAccess::None,
            Arc::clone(runtime),
            &bus,
            deck,
            &permit,
            Some(id),
        );
        let mut held = locked(&place);
        agent.wake(&why, &mut held, &mut context);
    }

    (0..steps)
        .map(|_| {
            let mut held = locked(&place);
            let permit = held.contention();
            let mut context = EngineContext::for_agent(
                WorldAccess::None,
                Arc::clone(runtime),
                &bus,
                deck,
                &permit,
                Some(id),
            );
            held.run(&mut context)
        })
        .collect()
}

/// `agent`'s place: a `LaneAgent` runs as layers.
pub fn place_of(agent: &LaneAgent) -> Arc<Mutex<Place>> {
    agent
        .place()
        .expect("a LaneAgent runs as layers: it has a place")
}

/// `place`, locked.
pub fn locked(place: &Mutex<Place>) -> MutexGuard<'_, Place> {
    place
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// The wakes `agent`'s place holds for the next event pass.
pub fn wakes_of(agent: &LaneAgent) -> Vec<Wake> {
    locked(&place_of(agent)).wakes().to_vec()
}

/// The row of the layer installed at `agent`'s place.
pub fn installed_row(agent: &LaneAgent) -> Option<StrategyId> {
    locked(&place_of(agent)).installed_row()
}

/// The strategy ids `agent` offers, in the order it offers them.
pub fn offered(agent: &mut LaneAgent) -> Vec<StrategyId> {
    agent
        .negotiate(request())
        .strategies
        .iter()
        .map(|option| option.id)
        .collect()
}

// ─── Driving an agent through the protocol ──────────────────────────────────

/// Room for anything on every device's memory.
pub const ANY_MEMORY: PerDevice<u64> = PerDevice {
    cpu: u64::MAX,
    gpu: u64::MAX,
    npu: u64::MAX,
    other: u64::MAX,
};

/// An envelope of `micros` on the host, nothing on the other devices, and
/// room for anything on every device's memory.
pub fn host_envelope(micros: u64) -> Envelope {
    Envelope {
        time: PerDevice {
            cpu: Duration::from_micros(micros),
            ..PerDevice::default()
        },
        memory: ANY_MEMORY,
    }
}

/// Lends `agent` `envelope` for epoch 1, with no pin and every row allowed.
pub fn lend(agent: &mut LaneAgent, envelope: Envelope) {
    agent.apply(Epoch(1), &envelope, None, None);
}

/// A request for an offer for `epoch`, at a 60 Hz target, while playing.
pub fn offer_request(epoch: u64) -> OfferRequest {
    OfferRequest {
        epoch: Epoch(epoch),
        target: Duration::from_micros(16_667),
        pressure: 0.0,
        mode: EngineMode::Playing,
        reply_by: Instant::now() + Duration::from_secs(1),
    }
}

/// The level `deck`'s last record ran at.
pub fn recorded_level(deck: &OutputDeck) -> Option<usize> {
    deck.get::<Recorded>().map(|recorded| recorded.level)
}
