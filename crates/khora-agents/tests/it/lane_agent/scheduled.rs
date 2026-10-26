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

//! `LaneAgent`s run by the scheduler: a lane reads the deck slot an earlier
//! phase wrote however the scheduler runs it — alone, in a concurrent wave,
//! at a fixed step — and what its family writes is audited like any agent's.

use std::sync::{Arc, Mutex, RwLock};
use std::time::Duration;

use khora_agents::lane_agent::LaneAgent;
use khora_agents::registered_agent::lane_agents;
use khora_control::{AgentRegistry, Context, ExecutionScheduler};
use khora_core::agent::gorna::{AgentId, StrategyId};
use khora_core::agent::registration::{AgentRegistration, BootError};
use khora_core::agent::{Agent, AgentImportance, EngineMode, ExecutionPhase, SharedEngineMode};
use khora_core::gpu::DeviceCaps;
use khora_core::lane::contract::{Lane, LaneReport};
use khora_core::lane::cost::Cost;
use khora_core::lane::family::{LaneFamily, Semantics};
use khora_core::lane::guard::{InputRefusal, OutputRefusal, SinkKind, SourceKind};
use khora_core::lane::io::{Declaration, InputSource, LaneInputs, LaneOutputs, OutputSink};
use khora_core::lane::level::Level;
use khora_core::lane::{LaneError, LaneKind};
use khora_core::Runtime;
use khora_data::ecs::World;

use super::fixtures::{level, NoInputs};

// ─── Slots ──────────────────────────────────────────────────────────────────

/// What the writing family leaves for a later phase.
#[derive(Debug, Default, Clone, Copy, PartialEq)]
pub struct Lit(pub u32);

/// What the reading family saw of [`Lit`].
#[derive(Debug, Default, Clone, Copy, PartialEq)]
pub struct Seen(pub Option<u32>);

/// The bystander's slot, so the reader has a wave to share.
#[derive(Debug, Default, Clone, Copy, PartialEq)]
pub struct Idle(pub u32);

/// A slot the stray family writes without declaring it.
#[derive(Debug, Default, Clone, Copy, PartialEq)]
pub struct Stray(pub u32);

// ─── Outputs ────────────────────────────────────────────────────────────────

pub struct LitOutputs {
    lit: Lit,
}

impl<'f> LaneOutputs<'f> for LitOutputs {
    fn declare(d: &mut Declaration) {
        d.write::<Lit>("lit", SinkKind::Deck);
    }
    fn open(_sink: &mut OutputSink<'f>) -> Result<Self, LaneError> {
        Ok(Self { lit: Lit(0) })
    }
    fn commit(self, sink: &mut OutputSink<'f>) -> Result<(), OutputRefusal> {
        sink.put("lit", self.lit)
    }
}

pub struct SeenOutputs {
    seen: Seen,
}

impl<'f> LaneOutputs<'f> for SeenOutputs {
    fn declare(d: &mut Declaration) {
        d.write::<Seen>("seen", SinkKind::Deck);
    }
    fn open(_sink: &mut OutputSink<'f>) -> Result<Self, LaneError> {
        Ok(Self { seen: Seen(None) })
    }
    fn commit(self, sink: &mut OutputSink<'f>) -> Result<(), OutputRefusal> {
        sink.put("seen", self.seen)
    }
}

pub struct IdleOutputs {
    idle: Idle,
}

impl<'f> LaneOutputs<'f> for IdleOutputs {
    fn declare(d: &mut Declaration) {
        d.write::<Idle>("idle", SinkKind::Deck);
    }
    fn open(_sink: &mut OutputSink<'f>) -> Result<Self, LaneError> {
        Ok(Self { idle: Idle(0) })
    }
    fn commit(self, sink: &mut OutputSink<'f>) -> Result<(), OutputRefusal> {
        sink.put("idle", self.idle)
    }
}

/// Declares `Idle` and also puts `Stray`, which it never declares.
pub struct StrayOutputs;

impl<'f> LaneOutputs<'f> for StrayOutputs {
    fn declare(d: &mut Declaration) {
        d.write::<Idle>("idle", SinkKind::Deck);
    }
    fn open(_sink: &mut OutputSink<'f>) -> Result<Self, LaneError> {
        Ok(Self)
    }
    fn commit(self, sink: &mut OutputSink<'f>) -> Result<(), OutputRefusal> {
        sink.put("idle", Idle(1))?;
        sink.put("stray", Stray(1))
    }
}

// ─── Inputs ─────────────────────────────────────────────────────────────────

/// The slot `Lit` an earlier phase wrote, if it did.
pub struct LitRead<'f> {
    lit: Option<&'f Lit>,
}

impl<'f> LaneInputs<'f> for LitRead<'f> {
    fn declare(d: &mut Declaration) {
        d.read::<Lit>("lit", SourceKind::Deck);
    }
    fn fetch(src: &mut InputSource<'f>) -> Result<Self, InputRefusal> {
        Ok(Self {
            lit: src.deck_slot::<Lit>("lit"),
        })
    }
}

// ─── Families and lanes ─────────────────────────────────────────────────────

macro_rules! family {
    ($name:ident, $kind:expr, $agent:expr, $outputs:ty) => {
        pub struct $name;
        impl LaneFamily for $name {
            const KIND: LaneKind = $kind;
            const NAME: &'static str = stringify!($name);
            const AGENT: AgentId = $agent;
            const SEMANTICS: Semantics = Semantics::Presentation;
            type Outputs<'f> = $outputs;
        }
    };
}

family!(LightFamily, LaneKind::Physics, AgentId::Physics, LitOutputs);
family!(EyeFamily, LaneKind::Render, AgentId::Skybox, SeenOutputs);
family!(IdleFamily, LaneKind::Ui, AgentId::Overlay, IdleOutputs);
family!(StrayFamily, LaneKind::Audio, AgentId::Audio, StrayOutputs);

const ONE_LEVEL: &[Level] = &[level(StrategyId::LowPower, "only")];

#[derive(Default)]
pub struct LightLane;

impl Lane for LightLane {
    type Family = LightFamily;
    type Inputs<'f> = NoInputs;
    fn levels(&self) -> &[Level] {
        ONE_LEVEL
    }
    fn cost(&self, _level: usize, _inputs: &NoInputs) -> Cost {
        Cost::default()
    }
    fn record(
        &mut self,
        _level: usize,
        _inputs: &mut NoInputs,
        out: &mut LitOutputs,
    ) -> Result<LaneReport, LaneError> {
        out.lit = Lit(7);
        Ok(LaneReport::default())
    }
}

#[derive(Default)]
pub struct EyeLane;

impl Lane for EyeLane {
    type Family = EyeFamily;
    type Inputs<'f> = LitRead<'f>;
    fn levels(&self) -> &[Level] {
        ONE_LEVEL
    }
    fn cost(&self, _level: usize, _inputs: &LitRead<'_>) -> Cost {
        Cost::default()
    }
    fn record(
        &mut self,
        _level: usize,
        inputs: &mut LitRead<'_>,
        out: &mut SeenOutputs,
    ) -> Result<LaneReport, LaneError> {
        out.seen = Seen(inputs.lit.map(|lit| lit.0));
        Ok(LaneReport::default())
    }
}

#[derive(Default)]
pub struct IdleLane;

impl Lane for IdleLane {
    type Family = IdleFamily;
    type Inputs<'f> = NoInputs;
    fn levels(&self) -> &[Level] {
        ONE_LEVEL
    }
    fn cost(&self, _level: usize, _inputs: &NoInputs) -> Cost {
        Cost::default()
    }
    fn record(
        &mut self,
        _level: usize,
        _inputs: &mut NoInputs,
        out: &mut IdleOutputs,
    ) -> Result<LaneReport, LaneError> {
        out.idle = Idle(1);
        Ok(LaneReport::default())
    }
}

#[derive(Default)]
pub struct StrayLane;

impl Lane for StrayLane {
    type Family = StrayFamily;
    type Inputs<'f> = NoInputs;
    fn levels(&self) -> &[Level] {
        ONE_LEVEL
    }
    fn cost(&self, _level: usize, _inputs: &NoInputs) -> Cost {
        Cost::default()
    }
    fn record(
        &mut self,
        _level: usize,
        _inputs: &mut NoInputs,
        _out: &mut StrayOutputs,
    ) -> Result<LaneReport, LaneError> {
        Ok(LaneReport::default())
    }
}

// ─── Registrations ──────────────────────────────────────────────────────────

/// Writes `Lit` in `TRANSFORM`, every frame.
static LIGHT: AgentRegistration = khora_core::register_agent! {
    family: LightFamily,
    phase: ExecutionPhase::TRANSFORM,
    importance: AgentImportance::Important,
    after: [],
    modes: [],
    fixed_step: None,
    lanes: [LightLane],
};

/// Writes `Lit` in `TRANSFORM`, at a fixed step of 4 ms.
static FIXED_LIGHT: AgentRegistration = khora_core::register_agent! {
    family: LightFamily,
    phase: ExecutionPhase::TRANSFORM,
    importance: AgentImportance::Important,
    after: [],
    modes: [],
    fixed_step: Some(Duration::from_millis(4)),
    lanes: [LightLane],
};

/// Reads `Lit` in `OUTPUT`, every frame.
static EYE: AgentRegistration = khora_core::register_agent! {
    family: EyeFamily,
    phase: ExecutionPhase::OUTPUT,
    importance: AgentImportance::Important,
    after: [],
    modes: [],
    fixed_step: None,
    lanes: [EyeLane],
};

/// Reads `Lit` in `OUTPUT`, at a fixed step of 4 ms.
static FIXED_EYE: AgentRegistration = khora_core::register_agent! {
    family: EyeFamily,
    phase: ExecutionPhase::OUTPUT,
    importance: AgentImportance::Important,
    after: [],
    modes: [],
    fixed_step: Some(Duration::from_millis(4)),
    lanes: [EyeLane],
};

/// Writes `Idle` in `OUTPUT`: the reader's wave partner.
static IDLE: AgentRegistration = khora_core::register_agent! {
    family: IdleFamily,
    phase: ExecutionPhase::OUTPUT,
    importance: AgentImportance::Important,
    after: [],
    modes: [],
    fixed_step: None,
    lanes: [IdleLane],
};

/// Writes `Stray` without declaring it, in `OUTPUT`.
static STRAY: AgentRegistration = khora_core::register_agent! {
    family: StrayFamily,
    phase: ExecutionPhase::OUTPUT,
    importance: AgentImportance::Important,
    after: [],
    modes: [],
    fixed_step: None,
    lanes: [StrayLane],
};

// ─── Driving a frame ────────────────────────────────────────────────────────

fn playing() -> Arc<Runtime> {
    let mode: SharedEngineMode = Arc::new(RwLock::new(EngineMode::Playing));
    let mut runtime = Runtime::new();
    runtime.resources.insert(mode);
    Arc::new(runtime)
}

/// Boots `registrations` as the engine does, then runs one frame.
fn one_frame(registrations: &[&'static AgentRegistration], parallel: bool) -> ExecutionScheduler {
    let runtime = playing();
    let agents: Vec<LaneAgent> = lane_agents(registrations, &runtime, &DeviceCaps::default())
        .expect("the boot check accepts the families");
    let registry = Arc::new(Mutex::new(AgentRegistry::new()));
    for agent in agents {
        registry
            .lock()
            .expect("the registry lock")
            .register(Arc::new(Mutex::new(agent)) as Arc<Mutex<dyn Agent>>, 1.0);
    }
    let mut scheduler =
        ExecutionScheduler::from_registry(registry, Arc::new(RwLock::new(Context::default())));
    scheduler.set_parallel_execution(parallel);
    scheduler.run_frame(&mut World::new(), runtime);
    scheduler
}

fn seen(scheduler: &mut ExecutionScheduler) -> Option<Seen> {
    scheduler.deck_mut().get::<Seen>().copied()
}

// ─── Tests ──────────────────────────────────────────────────────────────────

/// **A deck read sees the earlier phase however the reader is scheduled.**
/// The boot check accepts the reader because `TRANSFORM` writes `Lit` before
/// `OUTPUT` reads it. Run alone, the reader sees it; run in a concurrent wave
/// beside another world-free agent, it must see the same thing — parallel and
/// sequential frames give the game the same result.
#[test]
fn a_lane_in_a_concurrent_wave_reads_the_slot_an_earlier_phase_wrote() {
    let sequential = seen(&mut one_frame(&[&LIGHT, &EYE, &IDLE], false));
    assert_eq!(sequential, Some(Seen(Some(7))), "run alone, it sees it");

    let parallel = seen(&mut one_frame(&[&LIGHT, &EYE, &IDLE], true));
    assert_eq!(
        parallel, sequential,
        "in a concurrent wave the reader must see the slot TRANSFORM wrote"
    );
}

/// **Boot refuses a fixed-step reader of a once-per-frame slot.** Every
/// fixed-step round runs before the once-per-frame phases, so a fixed-step
/// reader in `OUTPUT` would run before the `TRANSFORM` writer every frame and
/// never see its slot. The reader is refused at boot, naming the read.
#[test]
fn boot_refuses_a_fixed_step_reader_of_a_once_per_frame_slot() {
    let refused = lane_agents(&[&LIGHT, &FIXED_EYE], &playing(), &DeviceCaps::default());

    assert!(
        matches!(
            refused,
            Err(BootError::DeckReadWithoutEarlierWriter { field: "lit", .. })
        ),
        "{:?}",
        refused.err()
    );
}

/// **A fixed-step lane reads the slot a fixed-step lane of an earlier phase
/// wrote.** Each round runs the phases in order, so the `TRANSFORM` writer's
/// sub-step comes before the `OUTPUT` reader's: boot accepts the pair, and the
/// reader sees the slot.
#[test]
fn a_fixed_step_lane_reads_the_slot_an_earlier_fixed_step_phase_wrote() {
    let mut scheduler = one_frame(&[&FIXED_LIGHT, &FIXED_EYE], false);

    let seen = seen(&mut scheduler);
    assert!(seen.is_some(), "the fixed reader ran at least one sub-step");
    assert_eq!(
        seen,
        Some(Seen(Some(7))),
        "the fixed reader sees the slot the fixed TRANSFORM writer wrote"
    );
}

/// **What a lane agent commits is audited like any agent's writes.** Its
/// family's outputs put a slot their declaration does not name; the
/// scheduler's deck audit reports it, alone or in a concurrent wave — the
/// commit records every slot it moves into the frame's deck.
#[cfg(debug_assertions)]
#[test]
fn an_undeclared_slot_a_lane_agent_commits_is_reported() {
    let stray = std::any::TypeId::of::<Stray>();
    for parallel in [false, true] {
        let scheduler = one_frame(&[&STRAY, &EYE, &LIGHT], parallel);
        assert!(
            scheduler
                .undeclared_deck_writes()
                .contains(&(AgentId::Audio, stray)),
            "parallel = {parallel}: {:?}",
            scheduler.undeclared_deck_writes()
        );
    }
}

// ─── Fixed-step families sharing a wave ─────────────────────────────────────

/// How many sub-steps the ticking family has recorded so far.
#[derive(Debug, Default, Clone, Copy, PartialEq)]
pub struct Ticks(pub u32);

/// The partner family's slot.
#[derive(Debug, Default, Clone, Copy, PartialEq)]
pub struct PartnerTicks(pub u32);

pub struct TicksOutputs {
    ticks: Ticks,
}

impl<'f> LaneOutputs<'f> for TicksOutputs {
    fn declare(d: &mut Declaration) {
        d.write::<Ticks>("ticks", SinkKind::Deck);
    }
    fn open(_sink: &mut OutputSink<'f>) -> Result<Self, LaneError> {
        Ok(Self { ticks: Ticks(0) })
    }
    fn commit(self, sink: &mut OutputSink<'f>) -> Result<(), OutputRefusal> {
        sink.put("ticks", self.ticks)
    }
}

pub struct PartnerOutputs {
    ticks: PartnerTicks,
}

impl<'f> LaneOutputs<'f> for PartnerOutputs {
    fn declare(d: &mut Declaration) {
        d.write::<PartnerTicks>("ticks", SinkKind::Deck);
    }
    fn open(_sink: &mut OutputSink<'f>) -> Result<Self, LaneError> {
        Ok(Self {
            ticks: PartnerTicks(0),
        })
    }
    fn commit(self, sink: &mut OutputSink<'f>) -> Result<(), OutputRefusal> {
        sink.put("ticks", self.ticks)
    }
}

family!(
    TickFamily,
    LaneKind::Physics,
    AgentId::Physics,
    TicksOutputs
);
family!(
    PartnerFamily,
    LaneKind::Script,
    AgentId::Script,
    PartnerOutputs
);

#[derive(Default)]
pub struct TickLane {
    ticks: u32,
}

impl Lane for TickLane {
    type Family = TickFamily;
    type Inputs<'f> = NoInputs;
    fn levels(&self) -> &[Level] {
        ONE_LEVEL
    }
    fn cost(&self, _level: usize, _inputs: &NoInputs) -> Cost {
        Cost::default()
    }
    fn record(
        &mut self,
        _level: usize,
        _inputs: &mut NoInputs,
        out: &mut TicksOutputs,
    ) -> Result<LaneReport, LaneError> {
        self.ticks += 1;
        out.ticks = Ticks(self.ticks);
        Ok(LaneReport::default())
    }
}

#[derive(Default)]
pub struct PartnerLane {
    ticks: u32,
}

impl Lane for PartnerLane {
    type Family = PartnerFamily;
    type Inputs<'f> = NoInputs;
    fn levels(&self) -> &[Level] {
        ONE_LEVEL
    }
    fn cost(&self, _level: usize, _inputs: &NoInputs) -> Cost {
        Cost::default()
    }
    fn record(
        &mut self,
        _level: usize,
        _inputs: &mut NoInputs,
        out: &mut PartnerOutputs,
    ) -> Result<LaneReport, LaneError> {
        self.ticks += 1;
        out.ticks = PartnerTicks(self.ticks);
        Ok(LaneReport::default())
    }
}

/// Counts its sub-steps at a fixed step of 4 ms.
static TICKING: AgentRegistration = khora_core::register_agent! {
    family: TickFamily,
    phase: ExecutionPhase::TRANSFORM,
    importance: AgentImportance::Important,
    after: [],
    modes: [],
    fixed_step: Some(Duration::from_millis(4)),
    lanes: [TickLane],
};

/// Counts its sub-steps at the same step, in the same phase: the two share a
/// concurrent wave.
static PARTNER: AgentRegistration = khora_core::register_agent! {
    family: PartnerFamily,
    phase: ExecutionPhase::TRANSFORM,
    importance: AgentImportance::Important,
    after: [],
    modes: [],
    fixed_step: Some(Duration::from_millis(4)),
    lanes: [PartnerLane],
};

/// **Every sub-step commits in a concurrent wave too.** Two fixed-step lane
/// agents share a wave each sub-step; the frame's deck ends with the last
/// sub-step's output, as it does when they run one after the other — not the
/// first sub-step's, kept because a later shard was refused as a collision.
#[test]
fn each_sub_step_commits_in_a_concurrent_wave() {
    let mut sequential = one_frame(&[&TICKING, &PARTNER], false);
    let sequential = sequential.deck_mut().get::<Ticks>().copied();
    assert!(
        sequential.is_some_and(|ticks| ticks.0 > 1),
        "the frame sub-steps more than once: {sequential:?}"
    );

    let mut parallel = one_frame(&[&TICKING, &PARTNER], true);
    assert_eq!(
        parallel.deck_mut().get::<Ticks>().copied(),
        sequential,
        "the last sub-step's output is in the deck"
    );
}

// ─── Custom phases ──────────────────────────────────────────────────────────

/// A game's own phase, which the SDK runs right after `OUTPUT` — so before
/// `FINALIZE`, though its id is higher.
const LATE: ExecutionPhase = ExecutionPhase::custom(6);

/// Writes `Lit` in `FINALIZE`, every frame.
static FINAL_LIGHT: AgentRegistration = khora_core::register_agent! {
    family: LightFamily,
    phase: ExecutionPhase::FINALIZE,
    importance: AgentImportance::Important,
    after: [],
    modes: [],
    fixed_step: None,
    lanes: [LightLane],
};

/// Reads `Lit` in the game's phase [`LATE`], every frame.
static LATE_EYE: AgentRegistration = khora_core::register_agent! {
    family: EyeFamily,
    phase: LATE,
    importance: AgentImportance::Important,
    after: [],
    modes: [],
    fixed_step: None,
    lanes: [EyeLane],
};

/// **A deck read boot accepts sees its writer, in a game's phase too.** The
/// SDK inserts a game's phases after `OUTPUT`, so `FINALIZE` runs after them
/// whatever their ids. A reader in such a phase of a slot only `FINALIZE`
/// writes is either refused at boot or, if accepted, sees the slot.
#[test]
fn a_deck_read_boot_accepts_in_a_custom_phase_sees_its_writer() {
    let runtime = playing();
    // The order the SDK builds for a game declaring `LATE`.
    let order = [
        ExecutionPhase::INIT,
        ExecutionPhase::OBSERVE,
        ExecutionPhase::TRANSFORM,
        ExecutionPhase::MUTATE,
        ExecutionPhase::OUTPUT,
        LATE,
        ExecutionPhase::FINALIZE,
    ];
    let agents = match khora_agents::registered_agent::lane_agents_in(
        &[&FINAL_LIGHT, &LATE_EYE],
        &runtime,
        &DeviceCaps::default(),
        &order,
    ) {
        Err(BootError::DeckReadWithoutEarlierWriter { field: "lit", .. }) => return,
        Err(other) => panic!("refused for another reason: {other:?}"),
        Ok(agents) => agents,
    };
    let registry = Arc::new(Mutex::new(AgentRegistry::new()));
    for agent in agents {
        registry
            .lock()
            .expect("the registry lock")
            .register(Arc::new(Mutex::new(agent)) as Arc<Mutex<dyn Agent>>, 1.0);
    }
    let mut scheduler =
        ExecutionScheduler::from_registry(registry, Arc::new(RwLock::new(Context::default())));
    scheduler.set_phase_order(&order);
    scheduler.run_frame(&mut World::new(), runtime);

    assert_eq!(
        seen(&mut scheduler),
        Some(Seen(Some(7))),
        "boot accepted the read, yet the reader ran before its FINALIZE writer"
    );
}
