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

//! `LaneAgent`s as layers under the real scheduler: on a quiet frame the
//! scheduler runs the installed layer without calling or locking the agent,
//! and an allocation the DCC publishes swaps the layer at a frame start.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{mpsc, Arc, Mutex, RwLock};
use std::time::Duration;

use khora_agents::agent::lane_agent;
use khora_control::{AgentRegistry, Context, ExecutionScheduler};
use khora_core::agent::gorna::protocol::{Allocation, Epoch, Trigger};
use khora_core::agent::gorna::{AgentId, StrategyId};
use khora_core::agent::registration::AgentRegistration;
use khora_core::agent::{Agent, AgentImportance, ExecutionPhase};
use khora_core::gpu::{DeviceCaps, GpuSetup};
use khora_core::lane::contract::{Lane, LaneReport};
use khora_core::lane::cost::{Cost, PerDevice};
use khora_core::lane::family::{LaneFamily, Semantics};
use khora_core::lane::io::TargetKey;
use khora_core::lane::level::{Impact, Level};
use khora_core::lane::{LaneError, LaneKind};
use khora_core::Runtime;
use khora_data::ecs::World;

use super::fixtures::{host_envelope, level, NoInputs, ProbeOutputs, Recorded};

pub struct ScheduledFamily;

impl LaneFamily for ScheduledFamily {
    const KIND: LaneKind = LaneKind::Audio;
    const NAME: &'static str = "scheduled";
    const AGENT: AgentId = AgentId::new("audio");
    const SEMANTICS: Semantics = Semantics::Presentation;
    type Outputs<'f> = ProbeOutputs;
}

/// How many times each lane of this file was prepared. Each registration is
/// built by one test only.
static TICK_PREPARES: AtomicUsize = AtomicUsize::new(0);
/// See [`TICK_PREPARES`].
static LOW_PREPARES: AtomicUsize = AtomicUsize::new(0);
/// See [`TICK_PREPARES`].
static HIGH_PREPARES: AtomicUsize = AtomicUsize::new(0);

/// The low lane draws a worse result than the high one, so the agent moves
/// to the high lane whenever its price fits — of two lanes of one impact it
/// would keep the cheaper.
const LOW_LEVELS: &[Level] = &[Level {
    impact: Impact::Minor,
    ..level(StrategyId::LowPower, "low")
}];
const HIGH_LEVELS: &[Level] = &[level(StrategyId::HighPerformance, "high")];
const TICK_LEVELS: &[Level] = &[level(StrategyId::Balanced, "tick")];

fn host(micros: u64) -> Cost {
    Cost {
        time: PerDevice {
            cpu: Duration::from_micros(micros),
            ..PerDevice::default()
        },
        ..Cost::default()
    }
}

/// Defines a lane of the scheduled family named `$name`, costing `$micros`,
/// counting its prepares in `$prepares` and its instance's records.
macro_rules! scheduled_lane {
    ($lane:ident, $name:literal, $levels:ident, $micros:literal, $prepares:ident) => {
        #[derive(Default)]
        pub struct $lane {
            records: u32,
        }

        impl Lane for $lane {
            type Family = ScheduledFamily;
            type Inputs<'f> = NoInputs;

            fn levels(&self) -> &[Level] {
                $levels
            }

            fn prepare(
                &mut self,
                _level: usize,
                _gpu: &mut GpuSetup<'_>,
                _key: Option<TargetKey>,
            ) -> Result<(), LaneError> {
                $prepares.fetch_add(1, Ordering::SeqCst);
                Ok(())
            }

            fn cost(&self, _level: usize, _inputs: &NoInputs) -> Cost {
                host($micros)
            }

            fn record(
                &mut self,
                level: usize,
                _inputs: &mut NoInputs,
                out: &mut ProbeOutputs,
            ) -> Result<LaneReport, LaneError> {
                self.records += 1;
                out.recorded = Recorded {
                    lane: $name,
                    level,
                    record: self.records,
                };
                Ok(LaneReport {
                    done: 1.0,
                    stalled: false,
                    message: None,
                })
            }
        }
    };
}

scheduled_lane!(TickLane, "tick", TICK_LEVELS, 10, TICK_PREPARES);
scheduled_lane!(LowLane, "low", LOW_LEVELS, 100, LOW_PREPARES);
scheduled_lane!(HighLane, "high", HIGH_LEVELS, 200, HIGH_PREPARES);

/// One lane, recording its instance's records.
static TICKING: AgentRegistration = khora_core::register_agent! {
    family: ScheduledFamily,
    phase: ExecutionPhase::OUTPUT,
    importance: AgentImportance::Important,
    after: [],
    fixed_step: None,
    lanes: [TickLane],
};

/// A lane of 100 µs and a `Minor` impact, then one of 200 µs and no impact.
static LOW_OR_HIGH: AgentRegistration = khora_core::register_agent! {
    family: ScheduledFamily,
    phase: ExecutionPhase::OUTPUT,
    importance: AgentImportance::Important,
    after: [],
    fixed_step: None,
    lanes: [LowLane, HighLane],
};

/// A scheduler over `agent` alone.
fn scheduling(agent: Arc<Mutex<dyn Agent>>) -> ExecutionScheduler {
    let registry = Arc::new(Mutex::new(AgentRegistry::new()));
    registry
        .lock()
        .expect("the registry lock")
        .register(agent, 1.0);
    ExecutionScheduler::from_registry(registry, Arc::new(RwLock::new(Context::default())))
}

/// What the last frame recorded: its lane and the instance's record count.
fn recorded(scheduler: &mut ExecutionScheduler) -> Option<(&'static str, u32)> {
    scheduler
        .deck_mut()
        .get::<Recorded>()
        .map(|recorded| (recorded.lane, recorded.record))
}

/// Epoch `epoch`, lending the scheduled family's agent `micros` on the host.
fn allocation(epoch: u64, micros: u64) -> Allocation {
    Allocation {
        epoch: Epoch(epoch),
        trigger: Trigger::Refresh,
        envelopes: vec![(ScheduledFamily::AGENT, host_envelope(micros))],
        pins: Vec::new(),
        allowed: Vec::new(),
        price: None,
        lever: false,
        late: Vec::new(),
    }
}

/// **A lane agent is neither called nor locked on a quiet frame.** Its first
/// frame installs its layer. On the next frame another thread holds the
/// agent's lock: the frame must not wait for it, and the layer still records
/// — its instance's second record.
#[test]
fn a_lane_agent_is_neither_called_nor_locked_on_a_quiet_frame() {
    let agent: Arc<Mutex<dyn Agent>> =
        Arc::new(Mutex::new(lane_agent(&TICKING, &DeviceCaps::default())));
    let mut scheduler = scheduling(Arc::clone(&agent));
    let mut world = World::new();
    let runtime = Arc::new(Runtime::new());
    scheduler.run_frame(&mut world, Arc::clone(&runtime));
    assert_eq!(recorded(&mut scheduler), Some(("tick", 1)));

    let (holding, held) = mpsc::channel();
    let (release, released) = mpsc::channel::<()>();
    let holder = std::thread::spawn(move || {
        let _guard = agent
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let _ = holding.send(());
        released.recv_timeout(Duration::from_secs(3)).is_err()
    });
    held.recv().expect("the holder took the agent's lock");
    scheduler.run_frame(&mut world, Arc::clone(&runtime));
    let _ = release.send(());
    let gave_up = holder.join().expect("the holder thread");

    assert!(
        !gave_up,
        "the quiet frame waited for the agent's lock until its holder gave it up"
    );
    assert_eq!(
        recorded(&mut scheduler),
        Some(("tick", 2)),
        "the installed layer recorded without the agent"
    );
}

/// **An allocation swaps a lane agent's layer at a frame start.** Epoch 1
/// lends 250 µs: the first frame runs the 200 µs lane. Epoch 2 lends 150 µs:
/// its frame prepares the 100 µs lane beside the installed one, which still
/// records; the next frame records on the new lane without preparing it
/// again.
#[test]
fn an_allocation_swaps_a_lane_agents_layer_at_a_frame_start() {
    let agent: Arc<Mutex<dyn Agent>> =
        Arc::new(Mutex::new(lane_agent(&LOW_OR_HIGH, &DeviceCaps::default())));
    let mut scheduler = scheduling(agent);
    let mut world = World::new();
    let runtime = Arc::new(Runtime::new());

    scheduler.budget_channel().publish(allocation(1, 250));
    scheduler.run_frame(&mut world, Arc::clone(&runtime));
    assert_eq!(recorded(&mut scheduler), Some(("high", 1)));

    scheduler.budget_channel().publish(allocation(2, 150));
    scheduler.run_frame(&mut world, Arc::clone(&runtime));
    assert_eq!(
        recorded(&mut scheduler),
        Some(("high", 2)),
        "the installed layer runs while the new one is prepared"
    );
    assert_eq!(LOW_PREPARES.load(Ordering::SeqCst), 1, "prepared beside");

    scheduler.run_frame(&mut world, Arc::clone(&runtime));
    assert_eq!(recorded(&mut scheduler), Some(("low", 1)));
    assert_eq!(
        LOW_PREPARES.load(Ordering::SeqCst),
        1,
        "the first frame on the new lane does no prepare"
    );
    assert_eq!(
        HIGH_PREPARES.load(Ordering::SeqCst),
        1,
        "the high lane, once"
    );
}
