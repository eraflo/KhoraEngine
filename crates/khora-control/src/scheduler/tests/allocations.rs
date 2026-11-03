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

//! The allocation the DCC publishes is lent by the scheduler at its frame
//! start, before any agent runs, to every agent it names at once and under
//! each agent's lock — never in the middle of a frame, and never by the DCC.

use crate::agent_registry::AgentRegistry;
use crate::budget_channel::BudgetChannel;
use crate::dcc_context::Context;
use crate::scheduler::*;
use khora_core::agent::dependency::DependencyKind;
use khora_core::agent::gorna::protocol::{Allocation, Envelope, Epoch, Trigger};
use khora_core::agent::gorna::{
    AgentId, AgentStatus, NegotiationRequest, NegotiationResponse, ResourceBudget, StrategyId,
};
use khora_core::agent::{
    Agent, AgentDependency, EngineMode, ExecutionPhase, ExecutionTiming, SharedEngineMode,
};
use khora_core::lane::cost::PerDevice;
use khora_core::{EngineContext, Runtime};
use khora_data::ecs::World;
use std::sync::{Arc, Mutex, RwLock};
use std::time::Duration;

/// What the agents of a test were lent and ran on, in order.
#[derive(Debug, Default)]
pub(super) struct Log {
    /// Each `apply`: the agent, and the epoch and envelope it was lent.
    pub(super) lent: Vec<(AgentId, Epoch, Envelope)>,
    /// Each `execute`: the agent, and the epoch it held while it ran.
    pub(super) ran: Vec<(AgentId, Option<Epoch>)>,
}

pub(super) type SharedLog = Arc<Mutex<Log>>;

/// An agent that writes what it is lent and what it runs on into a log.
pub(super) struct Hand {
    id: AgentId,
    log: SharedLog,
    /// The epoch of the last envelope it was lent.
    held: Option<Epoch>,
    /// Published on the channel during its next run — the DCC publishing in
    /// the middle of a frame.
    publishes: Option<(BudgetChannel, Allocation)>,
    /// Its priority within its one phase (higher runs first).
    priority: f32,
    /// The agent it runs after, as a hard dependency: the scheduler does not
    /// order agents of one phase by priority alone.
    after: Option<AgentId>,
}

impl Hand {
    pub(super) fn new(name: &'static str, log: &SharedLog) -> Self {
        Self {
            id: AgentId::new(name),
            log: Arc::clone(log),
            held: None,
            publishes: None,
            priority: 0.5,
            after: None,
        }
    }

    /// The same agent, running after `name` within the frame.
    fn after(mut self, name: &'static str) -> Self {
        self.after = Some(AgentId::new(name));
        self
    }

    /// The same agent, publishing `allocation` on `channel` while it next runs.
    fn publishing(mut self, channel: &BudgetChannel, allocation: Allocation) -> Self {
        self.publishes = Some((channel.clone(), allocation));
        self
    }
}

impl Agent for Hand {
    fn id(&self) -> AgentId {
        self.id
    }
    fn negotiate(&mut self, _r: NegotiationRequest) -> NegotiationResponse {
        NegotiationResponse {
            strategies: vec![],
            timing_adjustment: None,
        }
    }
    fn apply_budget(&mut self, _b: ResourceBudget) {}
    fn report_status(&self) -> AgentStatus {
        AgentStatus {
            agent_id: self.id,
            current_strategy: StrategyId::Balanced,
            health_score: 1.0,
            is_stalled: false,
            message: String::new(),
        }
    }
    fn apply(
        &mut self,
        epoch: Epoch,
        envelope: &Envelope,
        _pin: Option<StrategyId>,
        _allowed: Option<&[StrategyId]>,
    ) {
        self.held = Some(epoch);
        self.log
            .lock()
            .expect("the log lock")
            .lent
            .push((self.id, epoch, *envelope));
    }
    fn execute(&mut self, _ctx: &mut EngineContext<'_>) {
        self.log
            .lock()
            .expect("the log lock")
            .ran
            .push((self.id, self.held));
        if let Some((channel, allocation)) = self.publishes.take() {
            channel.publish(allocation);
        }
    }
    fn execution_timing(&self) -> ExecutionTiming {
        ExecutionTiming {
            dependencies: self
                .after
                .map(|target| AgentDependency {
                    target,
                    kind: DependencyKind::Hard,
                    condition: None,
                })
                .into_iter()
                .collect(),
            ..one_phase(self.priority)
        }
    }
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }
}

/// An envelope of `micros` on the host.
pub(super) fn envelope(micros: u64) -> Envelope {
    Envelope {
        time: PerDevice {
            cpu: Duration::from_micros(micros),
            ..PerDevice::default()
        },
        ..Envelope::default()
    }
}

/// Epoch `epoch`, lending each named agent an envelope of its `micros`.
pub(super) fn allocation(epoch: u64, named: &[(&'static str, u64)]) -> Allocation {
    Allocation {
        epoch: Epoch(epoch),
        trigger: Trigger::Refresh,
        envelopes: named
            .iter()
            .map(|&(name, micros)| (AgentId::new(name), envelope(micros)))
            .collect(),
        pins: Vec::new(),
        allowed: Vec::new(),
        price: None,
        lever: false,
        late: Vec::new(),
    }
}

/// A scheduler over `registry`, its own context in `Playing`.
pub(super) fn scheduler_over(registry: &Arc<Mutex<AgentRegistry>>) -> ExecutionScheduler {
    ExecutionScheduler::from_registry(
        Arc::clone(registry),
        Arc::new(RwLock::new(Context::default())),
    )
}

/// A runtime carrying the frame's mode, and the handle that writes it.
pub(super) fn runtime_in(mode: EngineMode) -> (Arc<Runtime>, SharedEngineMode) {
    let shared: SharedEngineMode = Arc::new(RwLock::new(mode));
    let mut runtime = Runtime::new();
    runtime.resources.insert(Arc::clone(&shared));
    (Arc::new(runtime), shared)
}

/// A timing that runs once a frame, in `OUTPUT` alone, at `priority` within
/// it (higher runs first) — so a test's agent runs once per frame, in the
/// order the test means.
pub(super) fn one_phase(priority: f32) -> ExecutionTiming {
    ExecutionTiming {
        allowed_phases: vec![ExecutionPhase::OUTPUT],
        default_phase: ExecutionPhase::OUTPUT,
        priority,
        ..ExecutionTiming::default()
    }
}

/// Registers `agent` in every mode at `priority` (higher runs first).
fn register(registry: &Arc<Mutex<AgentRegistry>>, mut agent: Hand, priority: f32) {
    agent.priority = priority;
    registry
        .lock()
        .expect("the registry lock")
        .register(Arc::new(Mutex::new(agent)), priority);
}

/// What ran since the last call, in run order.
fn ran(log: &SharedLog) -> Vec<(AgentId, Option<Epoch>)> {
    std::mem::take(&mut log.lock().expect("the log lock").ran)
}

/// The epochs and envelopes `name` was lent, in order.
pub(super) fn lent_to(log: &SharedLog, name: &'static str) -> Vec<(Epoch, Envelope)> {
    log.lock()
        .expect("the log lock")
        .lent
        .iter()
        .filter(|(agent, _, _)| *agent == AgentId::new(name))
        .map(|(_, epoch, envelope)| (*epoch, *envelope))
        .collect()
}

/// **I8: an allocation applies at the next frame start, for every agent at
/// once.** Epoch 1 is published before the first frame: both agents start it
/// on epoch 1. While `ash`, which runs first, is running, the DCC publishes
/// epoch 2: `birch`, running after it in the same frame, still runs on
/// epoch 1. At the next frame start both move to epoch 2 together. A frame
/// with nothing new lends nothing again.
#[test]
fn an_allocation_applies_at_the_next_frame_start_for_every_agent() {
    let log = SharedLog::default();
    let registry = Arc::new(Mutex::new(AgentRegistry::new()));
    let mut scheduler = scheduler_over(&registry);
    let channel = scheduler.budget_channel().clone();
    let second = allocation(2, &[("ash", 5_000), ("birch", 6_000)]);
    register(
        &registry,
        Hand::new("ash", &log).publishing(&channel, second),
        2.0,
    );
    register(&registry, Hand::new("birch", &log).after("ash"), 1.0);
    let (runtime, _mode) = runtime_in(EngineMode::Playing);
    let mut world = World::new();
    let (ash, birch) = (AgentId::new("ash"), AgentId::new("birch"));

    channel.publish(allocation(1, &[("ash", 3_000), ("birch", 4_000)]));
    scheduler.run_frame(&mut world, Arc::clone(&runtime));
    assert_eq!(
        ran(&log),
        vec![(ash, Some(Epoch(1))), (birch, Some(Epoch(1)))],
        "one epoch for the whole frame, though epoch 2 was published while ash ran"
    );

    scheduler.run_frame(&mut world, Arc::clone(&runtime));
    assert_eq!(
        ran(&log),
        vec![(ash, Some(Epoch(2))), (birch, Some(Epoch(2)))],
        "both move at the next frame start"
    );

    scheduler.run_frame(&mut world, Arc::clone(&runtime));
    assert_eq!(
        ran(&log),
        vec![(ash, Some(Epoch(2))), (birch, Some(Epoch(2)))]
    );
    assert_eq!(
        lent_to(&log, "ash"),
        vec![(Epoch(1), envelope(3_000)), (Epoch(2), envelope(5_000))],
        "each epoch lent once, with its own envelope"
    );
    assert_eq!(
        lent_to(&log, "birch"),
        vec![(Epoch(1), envelope(4_000)), (Epoch(2), envelope(6_000))]
    );
}

/// **An agent busy at a frame start keeps its envelope, and is lent the new
/// one at the next frame start.** Another thread holds `ash`'s lock when the
/// frame starts (the DCC asking it for an offer): the frame does not wait for
/// it to lend epoch 2 — `ash` runs that frame on epoch 1 — and the next frame
/// start lends it epoch 2. Nothing is lost.
#[test]
fn an_agent_busy_at_a_frame_start_is_lent_at_the_next_one() {
    let log = SharedLog::default();
    let registry = Arc::new(Mutex::new(AgentRegistry::new()));
    let mut scheduler = scheduler_over(&registry);
    let channel = scheduler.budget_channel().clone();
    let ash = Arc::new(Mutex::new(Hand::new("ash", &log)));
    registry
        .lock()
        .expect("the registry lock")
        .register(ash.clone(), 1.0);
    let (runtime, _mode) = runtime_in(EngineMode::Playing);
    let mut world = World::new();

    channel.publish(allocation(1, &[("ash", 3_000)]));
    scheduler.run_frame(&mut world, Arc::clone(&runtime));
    assert_eq!(ran(&log), vec![(AgentId::new("ash"), Some(Epoch(1)))]);
    channel.publish(allocation(2, &[("ash", 5_000)]));

    let (holding, held) = std::sync::mpsc::channel::<()>();
    let busy = {
        let ash = Arc::clone(&ash);
        std::thread::spawn(move || {
            let _asked_for_an_offer = ash.lock().expect("the agent lock");
            holding.send(()).expect("the test waits");
            std::thread::sleep(Duration::from_millis(200));
        })
    };
    held.recv().expect("the agent is busy");
    scheduler.run_frame(&mut world, Arc::clone(&runtime));
    busy.join().expect("the busy thread");
    assert_eq!(
        ran(&log),
        vec![(AgentId::new("ash"), Some(Epoch(1)))],
        "busy at the frame start, it ran that frame on the envelope it held"
    );
    assert_eq!(lent_to(&log, "ash"), vec![(Epoch(1), envelope(3_000))]);

    scheduler.run_frame(&mut world, Arc::clone(&runtime));
    assert_eq!(ran(&log), vec![(AgentId::new("ash"), Some(Epoch(2)))]);
    assert_eq!(
        lent_to(&log, "ash"),
        vec![(Epoch(1), envelope(3_000)), (Epoch(2), envelope(5_000))],
        "lent at the next frame start"
    );
}

/// **An agent the allocation does not name keeps what it holds.** Epoch 2
/// names `ash` alone: `ash` is lent it, `birch` is lent nothing and keeps
/// running on the envelope epoch 1 lent it.
#[test]
fn an_agent_the_allocation_does_not_name_keeps_what_it_holds() {
    let log = SharedLog::default();
    let registry = Arc::new(Mutex::new(AgentRegistry::new()));
    let mut scheduler = scheduler_over(&registry);
    let channel = scheduler.budget_channel().clone();
    register(&registry, Hand::new("ash", &log), 2.0);
    register(&registry, Hand::new("birch", &log).after("ash"), 1.0);
    let (runtime, _mode) = runtime_in(EngineMode::Playing);
    let mut world = World::new();

    channel.publish(allocation(1, &[("ash", 3_000), ("birch", 4_000)]));
    scheduler.run_frame(&mut world, Arc::clone(&runtime));
    channel.publish(allocation(2, &[("ash", 5_000)]));
    scheduler.run_frame(&mut world, Arc::clone(&runtime));

    assert_eq!(
        lent_to(&log, "ash"),
        vec![(Epoch(1), envelope(3_000)), (Epoch(2), envelope(5_000))]
    );
    assert_eq!(
        lent_to(&log, "birch"),
        vec![(Epoch(1), envelope(4_000))],
        "not named, not lent: it keeps epoch 1's envelope"
    );
    assert_eq!(
        ran(&log).last(),
        Some(&(AgentId::new("birch"), Some(Epoch(1))))
    );
}

/// **An agent not active in the frame's mode is lent the allocation once it
/// is.** The script runs in `Playing` only. While the editor edits, the
/// allocation naming it is lent to the renderer but not to the script, which
/// does not run; when the game plays, the allocation still in force is lent
/// to the script at that frame's start, before it runs, and the renderer is
/// not lent it twice.
#[test]
fn an_agent_not_active_in_the_mode_is_lent_the_allocation_once_it_is() {
    let log = SharedLog::default();
    let registry = Arc::new(Mutex::new(AgentRegistry::new()));
    let mut scheduler = scheduler_over(&registry);
    let channel = scheduler.budget_channel().clone();
    register(&registry, Hand::new("renderer", &log), 2.0);
    registry
        .lock()
        .expect("the registry lock")
        .register_for_mode(
            Arc::new(Mutex::new(Hand::new("script", &log))),
            1.0,
            vec![EngineMode::Playing],
        );
    let (runtime, mode) = runtime_in(EngineMode::Custom("editor".to_owned()));
    let mut world = World::new();

    channel.publish(allocation(1, &[("renderer", 3_000), ("script", 2_000)]));
    scheduler.run_frame(&mut world, Arc::clone(&runtime));
    assert_eq!(lent_to(&log, "renderer"), vec![(Epoch(1), envelope(3_000))]);
    assert!(
        lent_to(&log, "script").is_empty(),
        "the editor runs no script, and lends it nothing"
    );

    *mode.write().expect("the mode lock") = EngineMode::Playing;
    scheduler.run_frame(&mut world, Arc::clone(&runtime));
    assert_eq!(
        lent_to(&log, "script"),
        vec![(Epoch(1), envelope(2_000))],
        "lent once it plays"
    );
    assert!(
        ran(&log).contains(&(AgentId::new("script"), Some(Epoch(1)))),
        "and lent before it runs"
    );
    assert_eq!(
        lent_to(&log, "renderer").len(),
        1,
        "the renderer is not lent the same allocation twice"
    );
}

/// **A newly published allocation is lent whatever its epoch's number.** A
/// replay lends a trace recorded earlier, whose epochs may be numbered below
/// the live epoch in force: epoch 2, published after epoch 5, is lent.
#[test]
fn a_new_allocation_is_lent_whatever_its_epoch_number() {
    let log = SharedLog::default();
    let registry = Arc::new(Mutex::new(AgentRegistry::new()));
    let mut scheduler = scheduler_over(&registry);
    let channel = scheduler.budget_channel().clone();
    register(&registry, Hand::new("ash", &log), 1.0);
    let (runtime, _mode) = runtime_in(EngineMode::Playing);
    let mut world = World::new();

    channel.publish(allocation(5, &[("ash", 3_000)]));
    scheduler.run_frame(&mut world, Arc::clone(&runtime));
    channel.publish(allocation(2, &[("ash", 7_000)]));
    scheduler.run_frame(&mut world, Arc::clone(&runtime));

    assert_eq!(
        lent_to(&log, "ash"),
        vec![(Epoch(5), envelope(3_000)), (Epoch(2), envelope(7_000))]
    );
}

/// **An agent replaced by another of its name is lent the allocation in
/// force.** `ash` is lent epoch 1; then the registry is emptied and a new
/// `ash` registered (an agent removed and added again): at the next frame
/// start the newcomer, which holds nothing yet, is lent epoch 1 before it
/// runs — what the scheduler remembers is lent per agent, not per name.
#[test]
fn an_agent_replaced_under_its_name_is_lent_the_allocation_in_force() {
    let log = SharedLog::default();
    let registry = Arc::new(Mutex::new(AgentRegistry::new()));
    let mut scheduler = scheduler_over(&registry);
    let channel = scheduler.budget_channel().clone();
    register(&registry, Hand::new("ash", &log), 1.0);
    let (runtime, _mode) = runtime_in(EngineMode::Playing);
    let mut world = World::new();

    channel.publish(allocation(1, &[("ash", 3_000)]));
    scheduler.run_frame(&mut world, Arc::clone(&runtime));
    *registry.lock().expect("the registry lock") = AgentRegistry::new();
    register(&registry, Hand::new("ash", &log), 1.0);
    ran(&log);
    scheduler.run_frame(&mut world, Arc::clone(&runtime));

    assert_eq!(
        lent_to(&log, "ash"),
        vec![(Epoch(1), envelope(3_000)), (Epoch(1), envelope(3_000))],
        "the first `ash`, then the newcomer"
    );
    assert_eq!(
        ran(&log),
        vec![(AgentId::new("ash"), Some(Epoch(1)))],
        "lent before it runs"
    );
}

/// **The same allocation published twice is lent twice.** A replay may
/// publish an allocation equal to the one in force, epoch and all: it is a
/// new publication, and each agent is lent it again — once.
#[test]
fn the_same_allocation_published_twice_is_lent_twice() {
    let log = SharedLog::default();
    let registry = Arc::new(Mutex::new(AgentRegistry::new()));
    let mut scheduler = scheduler_over(&registry);
    let channel = scheduler.budget_channel().clone();
    register(&registry, Hand::new("ash", &log), 1.0);
    let (runtime, _mode) = runtime_in(EngineMode::Playing);
    let mut world = World::new();

    channel.publish(allocation(1, &[("ash", 3_000)]));
    scheduler.run_frame(&mut world, Arc::clone(&runtime));
    channel.publish(allocation(1, &[("ash", 3_000)]));
    scheduler.run_frame(&mut world, Arc::clone(&runtime));
    scheduler.run_frame(&mut world, Arc::clone(&runtime));

    assert_eq!(
        lent_to(&log, "ash"),
        vec![(Epoch(1), envelope(3_000)), (Epoch(1), envelope(3_000))]
    );
}

/// An agent sub-stepping at a fixed step its envelope sets: a frame's tick
/// at first, a quarter of it once it is lent an envelope.
struct Stepper {
    step: Duration,
    runs: Arc<Mutex<u32>>,
}

impl Agent for Stepper {
    fn id(&self) -> AgentId {
        AgentId::new("stepper")
    }
    fn negotiate(&mut self, _r: NegotiationRequest) -> NegotiationResponse {
        NegotiationResponse {
            strategies: vec![],
            timing_adjustment: None,
        }
    }
    fn apply_budget(&mut self, _b: ResourceBudget) {}
    fn report_status(&self) -> AgentStatus {
        AgentStatus {
            agent_id: self.id(),
            current_strategy: StrategyId::Balanced,
            health_score: 1.0,
            is_stalled: false,
            message: String::new(),
        }
    }
    fn apply(
        &mut self,
        _epoch: Epoch,
        _envelope: &Envelope,
        _pin: Option<StrategyId>,
        _allowed: Option<&[StrategyId]>,
    ) {
        self.step = Duration::from_secs_f32(khora_core::time::DEFAULT_FIXED_DELTA_SECONDS / 4.0);
    }
    fn execute(&mut self, _ctx: &mut EngineContext<'_>) {
        *self.runs.lock().expect("the runs lock") += 1;
    }
    fn execution_timing(&self) -> ExecutionTiming {
        ExecutionTiming {
            fixed_timestep: Some(self.step),
            ..one_phase(0.5)
        }
    }
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }
}

/// **A fixed step an allocation sets cuts the frame it is lent in.** The
/// allocation is lent at the frame start, before the scheduler reads the
/// fixed steps: the first frame's one tick is cut at the quarter step the
/// envelope set — about four sub-steps — not at the step held before it.
#[test]
fn a_fixed_step_an_allocation_sets_cuts_the_frame_it_is_lent_in() {
    let runs = Arc::new(Mutex::new(0));
    let registry = Arc::new(Mutex::new(AgentRegistry::new()));
    registry.lock().expect("the registry lock").register(
        Arc::new(Mutex::new(Stepper {
            step: Duration::from_secs_f32(khora_core::time::DEFAULT_FIXED_DELTA_SECONDS),
            runs: Arc::clone(&runs),
        })),
        1.0,
    );
    let mut scheduler = scheduler_over(&registry);
    let (runtime, _mode) = runtime_in(EngineMode::Playing);

    scheduler
        .budget_channel()
        .publish(allocation(1, &[("stepper", 3_000)]));
    scheduler.run_frame(&mut World::new(), runtime);

    let runs = *runs.lock().expect("the runs lock");
    assert!(runs >= 3, "the quarter step cut the tick: {runs} sub-steps");
}

/// **An agent busy at the frame start that takes a replayed epoch is still
/// lent that epoch.** A replay publishes its epochs in turn, one per frame
/// start, so each recorded epoch reaches every agent. `ash` is busy (another
/// thread holds its lock, as the DCC does while it asks for offers) at the
/// frame start that takes epoch 41: `birch` is lent 41 there. At the next
/// frame start, which takes 42, `ash` must not have skipped 41.
#[test]
fn an_agent_busy_when_a_replayed_epoch_is_taken_is_still_lent_it() {
    let log = SharedLog::default();
    let registry = Arc::new(Mutex::new(AgentRegistry::new()));
    let mut scheduler = scheduler_over(&registry);
    let channel = scheduler.budget_channel().clone();
    let ash = Arc::new(Mutex::new(Hand::new("ash", &log)));
    registry
        .lock()
        .expect("the registry lock")
        .register(ash.clone(), 2.0);
    register(&registry, Hand::new("birch", &log).after("ash"), 1.0);
    let (runtime, _mode) = runtime_in(EngineMode::Playing);
    let mut world = World::new();

    channel.publish_in_turn(allocation(41, &[("ash", 3_000), ("birch", 4_000)]));
    channel.publish_in_turn(allocation(42, &[("ash", 5_000), ("birch", 6_000)]));

    let (holding, held) = std::sync::mpsc::channel::<()>();
    let busy = {
        let ash = Arc::clone(&ash);
        std::thread::spawn(move || {
            let _asked_for_an_offer = ash.lock().expect("the agent lock");
            holding.send(()).expect("the test waits");
            std::thread::sleep(Duration::from_millis(200));
        })
    };
    held.recv().expect("the agent is busy");
    scheduler.run_frame(&mut world, Arc::clone(&runtime));
    busy.join().expect("the busy thread");
    scheduler.run_frame(&mut world, Arc::clone(&runtime));
    scheduler.run_frame(&mut world, Arc::clone(&runtime));

    assert_eq!(
        lent_to(&log, "birch"),
        vec![(Epoch(41), envelope(4_000)), (Epoch(42), envelope(6_000))]
    );
    assert!(
        lent_to(&log, "ash").contains(&(Epoch(41), envelope(3_000))),
        "every replayed epoch reaches every agent: ash was lent {:?}",
        lent_to(&log, "ash")
    );
}
