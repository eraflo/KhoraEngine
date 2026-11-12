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

//! A lane family at `ExecutionPhase::BACKGROUND` under the real scheduler:
//! its lane takes the orders its `Take` mailbox holds, off the frame, and the
//! loaves it stages reach the runtime's reserve only at a frame start after
//! its run ended — where the frame's `Boundary` systems take them. A newer
//! loaf of one key replaces a waiting one, which is abandoned; a run under
//! way when its family switches off still lands, and waits.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

use khora_agents::agent::lane_agent;
use khora_control::{AgentRegistry, ExecutionScheduler};
use khora_core::agent::gorna::{AgentId, StrategyId};
use khora_core::agent::registration::AgentRegistration;
use khora_core::agent::{Activation, Agent, AgentImportance, ExecutionPhase};
use khora_core::event::{BetweenSessions, Channel, Mailbox, Reader, Supersedes, WhenFull};
use khora_core::gpu::DeviceCaps;
use khora_core::lane::compile_check::{BudgetField, StagedField, TakeField};
use khora_core::lane::cost::Cost;
use khora_core::lane::family::{LaneFamily, Semantics};
use khora_core::lane::guard::{InputRefusal, OutputRefusal};
use khora_core::lane::io::{Budget, Declaration, InputSource, LaneInputs, LaneOutputs, OutputSink};
use khora_core::lane::level::Level;
use khora_core::lane::{Lane, LaneReport};
use khora_core::lane::{LaneError, Reserve, Staged};
use khora_core::Runtime;
use khora_data::ecs::World;

use super::fixtures::level;

/// The bakery: the family whose `Take` mailbox holds [`Order`]s.
const BAKERY: AgentId = AgentId::new("bakery");

/// How long a test waits for something the scheduler does on its own.
const SETTLE: Duration = Duration::from_secs(5);

// ─── Orders and loaves ──────────────────────────────────────────────────────

/// An order: bake the loaf of `key`, batch `batch`.
#[derive(Debug, Clone, PartialEq)]
pub struct Order {
    pub key: u32,
    pub batch: u32,
}
impl Supersedes for Order {}

khora_script::inventory::submit! {
    Mailbox::of::<Order>(BAKERY, Reader::Take, BetweenSessions::Hold)
}

/// The loaves abandoned, by key and batch. Each test bakes keys of its own.
static ABANDONED: Mutex<Vec<(u32, u32)>> = Mutex::new(Vec::new());

/// A staged loaf: a newer batch of the same key supersedes it.
#[derive(Debug, Clone, PartialEq)]
pub struct Loaf {
    pub key: u32,
    pub batch: u32,
}

impl Supersedes for Loaf {
    const COALESCES: bool = true;

    fn supersedes(&self, earlier: &Self) -> bool {
        self.key == earlier.key
    }
}

impl Staged for Loaf {
    fn abandon(self) {
        ABANDONED
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .push((self.key, self.batch));
    }
}

fn abandoned(key: u32) -> Vec<(u32, u32)> {
    ABANDONED
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .iter()
        .copied()
        .filter(|(abandoned, _)| *abandoned == key)
        .collect()
}

fn batches(loaves: &[Loaf]) -> Vec<(u32, u32)> {
    loaves.iter().map(|loaf| (loaf.key, loaf.batch)).collect()
}

// ─── The family ─────────────────────────────────────────────────────────────

/// What a bake reads: the orders waiting, drained, and its budget. Written
/// out as `#[derive(LaneInputs)]` with `#[take] orders` and `#[budget]
/// budget` derives it (this crate's tests do not depend on khora-macros).
#[allow(dead_code)]
pub struct BakeInputs {
    pub orders: Vec<Order>,
    pub budget: Option<Budget>,
}

impl<'f> LaneInputs<'f> for BakeInputs {
    fn declare(d: &mut Declaration) {
        <Vec<Order> as TakeField<'f>>::declare(d, "orders");
        <Option<Budget> as BudgetField>::declare(d, "budget");
    }

    fn fetch(src: &mut InputSource<'f>) -> Result<Self, InputRefusal> {
        Ok(Self {
            orders: <Vec<Order> as TakeField<'f>>::fetch(src, "orders")?,
            budget: <Option<Budget> as BudgetField>::fetch(src, "budget")?,
        })
    }
}

/// The bakery's outputs: the loaves it staged — as `#[derive(LaneOutputs)]`
/// with `#[staged] loaves` derives them.
pub struct BakeOutputs {
    pub loaves: Vec<Loaf>,
}

impl<'f> LaneOutputs<'f> for BakeOutputs {
    fn declare(d: &mut Declaration) {
        <Vec<Loaf> as StagedField<'f>>::declare(d, "loaves");
    }

    fn open(sink: &mut OutputSink<'f>) -> Result<Self, LaneError> {
        Ok(Self {
            loaves: <Vec<Loaf> as StagedField<'f>>::open(sink, "loaves")?,
        })
    }

    fn commit(self, sink: &mut OutputSink<'f>) -> Result<(), OutputRefusal> {
        <Vec<Loaf> as StagedField<'f>>::commit(self.loaves, sink, "loaves")
    }
}

pub struct BakeryFamily;

impl LaneFamily for BakeryFamily {
    const NAME: &'static str = "bakery";
    const AGENT: AgentId = BAKERY;
    const SEMANTICS: Semantics = Semantics::Presentation;
    type Outputs<'f> = BakeOutputs;
}

// ─── A gate a bake waits at ─────────────────────────────────────────────────

/// Holds the bakes of one lane until it is opened, counting the bakes that
/// started and finished.
pub struct Gate {
    open: Mutex<bool>,
    opened: Condvar,
    started: AtomicUsize,
    finished: AtomicUsize,
}

impl Gate {
    const fn closed() -> Self {
        Self {
            open: Mutex::new(false),
            opened: Condvar::new(),
            started: AtomicUsize::new(0),
            finished: AtomicUsize::new(0),
        }
    }

    fn let_through(&self) {
        *self.open.lock().unwrap_or_else(|e| e.into_inner()) = true;
        self.opened.notify_all();
    }

    fn pass(&self) {
        let open = self.open.lock().unwrap_or_else(|e| e.into_inner());
        let _ = self
            .opened
            .wait_timeout_while(open, Duration::from_secs(10), |open| !*open);
    }

    fn started(&self) -> usize {
        self.started.load(Ordering::SeqCst)
    }

    fn finished(&self) -> usize {
        self.finished.load(Ordering::SeqCst)
    }
}

/// The gate of [`HeldBakeLane`].
static HELD: Gate = Gate::closed();
/// The gate of [`SwitchedOffBakeLane`].
static SWITCHED_OFF: Gate = Gate::closed();

// ─── Lanes ──────────────────────────────────────────────────────────────────

const LEVELS: &[Level] = &[level(StrategyId::Balanced, "bake")];

/// A lane of the bakery baking one loaf per order, waiting at `$gate` when
/// it has one.
macro_rules! bake_lane {
    ($lane:ident, $gate:expr) => {
        #[derive(Default)]
        pub struct $lane;

        impl Lane for $lane {
            type Family = BakeryFamily;
            type Inputs<'f> = BakeInputs;

            fn levels(&self) -> &[Level] {
                LEVELS
            }

            fn cost(&self, _level: usize, _inputs: &BakeInputs) -> Cost {
                Cost::default()
            }

            fn record(
                &mut self,
                _level: usize,
                inputs: &mut BakeInputs,
                out: &mut BakeOutputs,
            ) -> Result<LaneReport, LaneError> {
                let gate: Option<&Gate> = $gate;
                if let Some(gate) = gate {
                    gate.started.fetch_add(1, Ordering::SeqCst);
                    gate.pass();
                }
                out.loaves.extend(inputs.orders.iter().map(|order| Loaf {
                    key: order.key,
                    batch: order.batch,
                }));
                if let Some(gate) = gate {
                    gate.finished.fetch_add(1, Ordering::SeqCst);
                }
                Ok(LaneReport {
                    done: 1.0,
                    stalled: false,
                    message: None,
                })
            }
        }
    };
}

bake_lane!(BakeLane, None);
bake_lane!(HeldBakeLane, Some(&HELD));
bake_lane!(SwitchedOffBakeLane, Some(&SWITCHED_OFF));

static BAKING: AgentRegistration = khora_core::register_agent! {
    family: BakeryFamily,
    phase: ExecutionPhase::BACKGROUND,
    importance: AgentImportance::Optional,
    after: [],
    fixed_step: None,
    lanes: [BakeLane],
};

static HELD_BAKING: AgentRegistration = khora_core::register_agent! {
    family: BakeryFamily,
    phase: ExecutionPhase::BACKGROUND,
    importance: AgentImportance::Optional,
    after: [],
    fixed_step: None,
    lanes: [HeldBakeLane],
};

static SWITCHED_OFF_BAKING: AgentRegistration = khora_core::register_agent! {
    family: BakeryFamily,
    phase: ExecutionPhase::BACKGROUND,
    importance: AgentImportance::Optional,
    after: [],
    fixed_step: None,
    lanes: [SwitchedOffBakeLane],
};

// ─── Driving frames ─────────────────────────────────────────────────────────

/// A bakery under the real scheduler, on a runtime holding its order
/// channel and a reserve.
struct Bakery {
    scheduler: ExecutionScheduler,
    world: World,
    runtime: Arc<Runtime>,
    orders: Channel<Order>,
    reserve: Reserve,
}

impl Bakery {
    fn new(registration: &'static AgentRegistration, activation: Option<Arc<Activation>>) -> Self {
        let agent: Arc<Mutex<dyn Agent>> =
            Arc::new(Mutex::new(lane_agent(registration, &DeviceCaps::default())));
        let registry = Arc::new(Mutex::new(AgentRegistry::new()));
        registry
            .lock()
            .expect("the registry lock")
            .register(agent, 1.0);
        let scheduler = ExecutionScheduler::from_registry(registry);
        let orders = Channel::<Order>::bounded(16, WhenFull::Reject);
        let reserve = Reserve::new();
        let mut runtime = Runtime::new();
        runtime.resources.insert(orders.clone());
        runtime.resources.insert(reserve.clone());
        if let Some(activation) = activation {
            runtime.resources.insert(activation);
        }
        Self {
            scheduler,
            world: World::new(),
            runtime: Arc::new(runtime),
            orders,
            reserve,
        }
    }

    fn order(&self, key: u32, batch: u32) {
        self.orders.send(Order { key, batch });
    }

    /// The frame start — what the host does before the frame's `Boundary`.
    fn begin(&mut self) {
        self.scheduler.begin_frame(&self.runtime);
    }

    /// The rest of the frame.
    fn run(&mut self) {
        self.scheduler
            .run_frame(&mut self.world, Arc::clone(&self.runtime));
    }

    /// Whole frames, as the host runs them, until `done` holds; panics after
    /// [`SETTLE`] with `what`.
    fn frames_until(&mut self, what: &str, mut done: impl FnMut(&mut Self) -> bool) {
        let deadline = Instant::now() + SETTLE;
        loop {
            self.begin();
            if done(self) {
                return;
            }
            self.run();
            assert!(Instant::now() < deadline, "timed out waiting for {what}");
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    /// Whole frames until a frame start brings loaves into the reserve,
    /// which the frame's `Boundary` would take: they are taken and returned.
    /// Between one frame start and the next nothing reaches the reserve.
    fn boundaries_until_loaves(&mut self) -> Vec<Loaf> {
        let deadline = Instant::now() + SETTLE;
        loop {
            assert!(
                self.reserve.is_empty::<Loaf>(),
                "loaves reached the reserve between two frame starts"
            );
            self.begin();
            let taken = self.reserve.take::<Loaf>();
            if !taken.is_empty() {
                return taken;
            }
            self.run();
            assert!(Instant::now() < deadline, "timed out waiting for loaves");
            std::thread::sleep(Duration::from_millis(10));
        }
    }
}

/// Waits until `done` holds, or panics after [`SETTLE`] with `what`.
fn wait_until(what: &str, done: impl Fn() -> bool) {
    let deadline = Instant::now() + SETTLE;
    while !done() {
        assert!(Instant::now() < deadline, "timed out waiting for {what}");
        std::thread::sleep(Duration::from_millis(5));
    }
}

// ─── Tests ──────────────────────────────────────────────────────────────────

/// **A background lane bakes what its mailbox holds, and the loaves wait in
/// the reserve for a boundary.** Two orders are sent before the first frame
/// — while the agent installs its layer — and none is lost to it: the run
/// takes both, the channel is left empty, and a frame start brings both
/// loaves into the reserve, in order.
#[test]
fn a_background_lane_bakes_its_orders_into_the_reserve() {
    let mut bakery = Bakery::new(&BAKING, None);
    bakery.order(1, 1);
    bakery.order(2, 1);

    let loaves = bakery.boundaries_until_loaves();

    assert_eq!(batches(&loaves), vec![(1, 1), (2, 1)]);
    assert!(bakery.orders.is_empty(), "the orders were taken");
}

/// **What a run staged reaches the reserve only at a frame start after the
/// run ended.** The bake is held: frames go by with nothing in the reserve.
/// Let through, it finishes — and still nothing is in the reserve until the
/// next frame start, which brings its loaf.
#[test]
fn what_a_run_staged_reaches_the_reserve_only_at_a_frame_start() {
    let mut bakery = Bakery::new(&HELD_BAKING, None);
    bakery.order(10, 1);

    bakery.frames_until("the bake to start", |_| HELD.started() == 1);
    for _ in 0..3 {
        bakery.run();
        bakery.begin();
        assert!(bakery.reserve.is_empty::<Loaf>(), "the bake is held");
    }
    bakery.run();

    HELD.let_through();
    wait_until("the bake to finish", || HELD.finished() == 1);
    std::thread::sleep(Duration::from_millis(100));
    let loaves = bakery.boundaries_until_loaves();

    assert_eq!(batches(&loaves), vec![(10, 1)]);
}

/// **A newer batch of a key replaces the waiting loaf, which is abandoned.**
/// Batch 1 of key 20 waits in the reserve, untaken; batch 2 of the same key
/// is baked after it. Batch 1 is never applied — it goes through its
/// abandon — and batch 2 is what the boundary takes.
#[test]
fn a_newer_batch_replaces_a_waiting_loaf_which_is_abandoned() {
    let mut bakery = Bakery::new(&BAKING, None);
    bakery.order(20, 1);
    bakery.frames_until("batch 1 to wait in the reserve", |bakery| {
        bakery.reserve.len::<Loaf>() == 1
    });
    bakery.run();

    bakery.order(20, 2);
    bakery.frames_until("batch 1 to be abandoned", |_| {
        abandoned(20) == vec![(20, 1)]
    });

    assert_eq!(batches(&bakery.reserve.take::<Loaf>()), vec![(20, 2)]);
}

/// **A run under way when its family switches off still lands, and waits.**
/// The bake is held when the bakery is switched off. Let through, it
/// finishes; its loaf reaches the reserve at a frame start though the
/// bakery is off, and waits there, neither applied nor abandoned, frame
/// after frame.
#[test]
fn a_run_under_way_when_its_family_switches_off_lands_and_waits() {
    let activation = Arc::new(Activation::new([BAKERY]));
    let mut bakery = Bakery::new(&SWITCHED_OFF_BAKING, Some(Arc::clone(&activation)));
    bakery.order(30, 1);
    bakery.frames_until("the bake to start", |_| SWITCHED_OFF.started() == 1);
    bakery.run();

    activation.deactivate(BAKERY).expect("a known family");
    activation.settle();
    SWITCHED_OFF.let_through();
    wait_until("the bake to finish", || SWITCHED_OFF.finished() == 1);
    bakery.frames_until("the loaf to reach the reserve", |bakery| {
        bakery.reserve.len::<Loaf>() == 1
    });
    for _ in 0..3 {
        bakery.run();
        bakery.begin();
    }

    assert!(!activation.is_active(BAKERY), "the bakery is still off");
    assert_eq!(bakery.reserve.len::<Loaf>(), 1, "the loaf waits");
    assert!(abandoned(30).is_empty(), "nothing replaced it");
}

// ─── What spans frames, at its edges ────────────────────────────────────────

/// A second kind of request the bakery's mailboxes hold, which the lanes
/// below never take: another lane of a family may, at another row.
#[derive(Debug, Clone, PartialEq)]
pub struct Extra(pub u32);
impl Supersedes for Extra {}

khora_script::inventory::submit! {
    Mailbox::of::<Extra>(BAKERY, Reader::Take, BetweenSessions::Hold)
}

/// The runs of [`CountingBakeLane`].
static COUNTED_RUNS: AtomicUsize = AtomicUsize::new(0);

/// A bake lane counting its runs, which takes orders and never `Extra`.
#[derive(Default)]
pub struct CountingBakeLane;

impl Lane for CountingBakeLane {
    type Family = BakeryFamily;
    type Inputs<'f> = BakeInputs;

    fn levels(&self) -> &[Level] {
        LEVELS
    }

    fn cost(&self, _level: usize, _inputs: &BakeInputs) -> Cost {
        Cost::default()
    }

    fn record(
        &mut self,
        _level: usize,
        inputs: &mut BakeInputs,
        out: &mut BakeOutputs,
    ) -> Result<LaneReport, LaneError> {
        COUNTED_RUNS.fetch_add(1, Ordering::SeqCst);
        out.loaves.extend(inputs.orders.iter().map(|order| Loaf {
            key: order.key,
            batch: order.batch,
        }));
        Ok(LaneReport {
            done: 1.0,
            stalled: false,
            message: None,
        })
    }
}

static COUNTED_BAKING: AgentRegistration = khora_core::register_agent! {
    family: BakeryFamily,
    phase: ExecutionPhase::BACKGROUND,
    importance: AgentImportance::Optional,
    after: [],
    fixed_step: None,
    lanes: [CountingBakeLane],
};

/// The key whose order makes [`PanickingBakeLane`] panic.
const BURNT: u32 = 666;

/// A bake lane that panics on an order of key [`BURNT`], and bakes the rest.
#[derive(Default)]
pub struct PanickingBakeLane;

impl Lane for PanickingBakeLane {
    type Family = BakeryFamily;
    type Inputs<'f> = BakeInputs;

    fn levels(&self) -> &[Level] {
        LEVELS
    }

    fn cost(&self, _level: usize, _inputs: &BakeInputs) -> Cost {
        Cost::default()
    }

    fn record(
        &mut self,
        _level: usize,
        inputs: &mut BakeInputs,
        out: &mut BakeOutputs,
    ) -> Result<LaneReport, LaneError> {
        assert!(
            inputs.orders.iter().all(|order| order.key != BURNT),
            "the oven burns order {BURNT}"
        );
        out.loaves.extend(inputs.orders.iter().map(|order| Loaf {
            key: order.key,
            batch: order.batch,
        }));
        Ok(LaneReport {
            done: 1.0,
            stalled: false,
            message: None,
        })
    }
}

static PANICKING_BAKING: AgentRegistration = khora_core::register_agent! {
    family: BakeryFamily,
    phase: ExecutionPhase::BACKGROUND,
    importance: AgentImportance::Optional,
    after: [],
    fixed_step: None,
    lanes: [PanickingBakeLane],
};

/// What a frame lane of the bakery reads: its budget alone.
#[allow(dead_code)]
pub struct BudgetOnly {
    pub budget: Option<Budget>,
}

impl<'f> LaneInputs<'f> for BudgetOnly {
    fn declare(d: &mut Declaration) {
        <Option<Budget> as BudgetField>::declare(d, "budget");
    }

    fn fetch(src: &mut InputSource<'f>) -> Result<Self, InputRefusal> {
        Ok(Self {
            budget: <Option<Budget> as BudgetField>::fetch(src, "budget")?,
        })
    }
}

/// A lane of the bakery placed in a frame phase, staging one loaf a run.
#[derive(Default)]
pub struct FrameBakeLane;

impl Lane for FrameBakeLane {
    type Family = BakeryFamily;
    type Inputs<'f> = BudgetOnly;

    fn levels(&self) -> &[Level] {
        LEVELS
    }

    fn cost(&self, _level: usize, _inputs: &BudgetOnly) -> Cost {
        Cost::default()
    }

    fn record(
        &mut self,
        _level: usize,
        _inputs: &mut BudgetOnly,
        out: &mut BakeOutputs,
    ) -> Result<LaneReport, LaneError> {
        out.loaves.push(Loaf { key: 50, batch: 1 });
        Ok(LaneReport {
            done: 1.0,
            stalled: false,
            message: None,
        })
    }
}

static FRAME_BAKING: AgentRegistration = khora_core::register_agent! {
    family: BakeryFamily,
    phase: ExecutionPhase::OUTPUT,
    importance: AgentImportance::Optional,
    after: [],
    fixed_step: None,
    lanes: [FrameBakeLane],
};

/// **A run that panics comes home, and its family runs again.** The first
/// order burns the bake; the layer is checked back in all the same, so the
/// next order starts a run of its own, whose loaf reaches the reserve.
#[test]
fn a_background_run_that_panics_comes_home_and_the_family_runs_again() {
    let mut bakery = Bakery::new(&PANICKING_BAKING, None);
    bakery.order(BURNT, 1);
    bakery.frames_until("the burnt order to be taken", |bakery| {
        bakery.orders.is_empty()
    });
    for _ in 0..3 {
        bakery.run();
        bakery.begin();
    }
    bakery.run();

    bakery.order(40, 1);
    let loaves = bakery.boundaries_until_loaves();

    assert_eq!(batches(&loaves), vec![(40, 1)]);
}

/// **Mail the installed layer does not take starts no run.** The bakery's
/// mailboxes hold an `Extra`, which its installed lane never takes: no run
/// can answer it, so none starts — not one a frame, frame after frame.
#[test]
fn mail_the_installed_layer_does_not_take_starts_no_background_run() {
    let mut bakery = Bakery::new(&COUNTED_BAKING, None);
    let extras = Channel::<Extra>::bounded(4, WhenFull::Reject);
    let mut runtime = Runtime::new();
    runtime.resources.insert(bakery.orders.clone());
    runtime.resources.insert(bakery.reserve.clone());
    runtime.resources.insert(extras.clone());
    bakery.runtime = Arc::new(runtime);
    // The layer installs, and answers an order once.
    bakery.order(60, 1);
    let first = bakery.boundaries_until_loaves();
    assert_eq!(batches(&first), vec![(60, 1)]);
    wait_until("the order's run to be counted", || {
        COUNTED_RUNS.load(Ordering::SeqCst) == 1
    });

    extras.send(Extra(1));
    for _ in 0..20 {
        bakery.run();
        std::thread::sleep(Duration::from_millis(5));
        bakery.begin();
    }

    assert_eq!(
        COUNTED_RUNS.load(Ordering::SeqCst),
        1,
        "the layer was run for mail it never takes"
    );
    assert_eq!(
        extras.len(),
        1,
        "the extra still waits for a lane taking it"
    );
}

/// **What a lane at a frame phase stages is not silently kept.** The bakery
/// is placed at `OUTPUT` with a `#[staged]` output. Either boot refuses that,
/// or what its runs stage reaches the reserve like a background run's —
/// never piling up in the layer, out of every boundary's reach.
#[test]
fn what_a_frame_phase_lane_stages_reaches_the_reserve_or_boot_refuses_it() {
    let mut bakery = Bakery::new(&FRAME_BAKING, None);
    let refused = khora_core::agent::check_registrations(
        &[&FRAME_BAKING],
        &bakery.runtime,
        &DeviceCaps::default(),
    )
    .is_err();
    if refused {
        return;
    }

    let deadline = Instant::now() + Duration::from_secs(2);
    while bakery.reserve.is_empty::<Loaf>() && Instant::now() < deadline {
        bakery.begin();
        bakery.run();
    }

    assert!(
        !bakery.reserve.is_empty::<Loaf>(),
        "a frame-phase lane staged loaves, boot let it, and none reached the reserve"
    );
}
