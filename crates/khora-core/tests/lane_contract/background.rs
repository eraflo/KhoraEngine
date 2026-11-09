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

//! What boot says about a family at `ExecutionPhase::BACKGROUND`: it runs off
//! the frame, so it is in no phase order and boots all the same; its lanes
//! read only what outlives a frame — `#[take]`, `#[resource]`, `#[budget]` —
//! and a `#[take]` drains a channel its family's `Take` mailbox declares.

use std::any::TypeId;
use std::sync::{Arc, Mutex};

use khora_core::agent::registration::{
    check_registrations, check_registrations_in, AgentRegistration, BootError,
};
use khora_core::agent::{AgentImportance, ExecutionPhase};
use khora_core::event::{Channel, WhenFull};
use khora_core::gpu::DeviceCaps;
use khora_core::lane::erased::{AnyLane, Erased};
use khora_core::lane::family::LaneFamily;
use khora_core::lane::guard::SourceKind;
use khora_core::residency::Resident;
use khora_core::Runtime;

use crate::bakery::{
    BakeFamily, BusLane, DeckLane, FrameLane, Glazes, LockLane, OffFrameLane, Order, OrdersLane,
    QuietFamily, ResidentLane, TheirsLane, UnclaimedLane, WatchedLane,
};
use crate::fixtures::{Counter, MixSettings, SharedCounter};

type LaneConstructor = fn() -> Box<dyn AnyLane>;

fn off_frame_lane() -> Box<dyn AnyLane> {
    Box::new(Erased::new(OffFrameLane))
}
fn bus_lane() -> Box<dyn AnyLane> {
    Box::new(Erased::new(BusLane))
}
fn frame_lane() -> Box<dyn AnyLane> {
    Box::new(Erased::new(FrameLane))
}
fn deck_lane() -> Box<dyn AnyLane> {
    Box::new(Erased::new(DeckLane))
}
fn resident_lane() -> Box<dyn AnyLane> {
    Box::new(Erased::new(ResidentLane))
}
fn lock_lane() -> Box<dyn AnyLane> {
    Box::new(Erased::new(LockLane))
}
fn orders_lane() -> Box<dyn AnyLane> {
    Box::new(Erased::new(OrdersLane))
}
fn unclaimed_lane() -> Box<dyn AnyLane> {
    Box::new(Erased::new(UnclaimedLane))
}
fn watched_lane() -> Box<dyn AnyLane> {
    Box::new(Erased::new(WatchedLane))
}
fn theirs_lane() -> Box<dyn AnyLane> {
    Box::new(Erased::new(TheirsLane))
}

/// The registration `register_agent!` would build for family `F` at
/// `phase`.
fn registration<F: LaneFamily>(
    phase: ExecutionPhase,
    lanes: &'static [LaneConstructor],
) -> AgentRegistration {
    AgentRegistration {
        family: TypeId::of::<F>(),
        family_name: F::NAME,
        agent: F::AGENT,
        semantics: F::SEMANTICS,
        phase,
        importance: AgentImportance::Optional,
        after: &[],
        fixed_step: None,
        default_level: None,
        module: module_path!(),
        lanes,
    }
}

/// A runtime holding every entry the lanes of these tests read, so the only
/// thing boot can refuse is where a lane reads from.
fn runtime() -> Runtime {
    let mut runtime = Runtime::new();
    runtime.resources.insert(MixSettings { gain: 1.0 });
    let counter: SharedCounter = Arc::new(Mutex::new(Counter(0)));
    runtime.resources.insert(counter);
    runtime.resources.insert(Resident::<Glazes>::default());
    runtime
        .resources
        .insert(Channel::<Order>::bounded(8, WhenFull::Reject));
    runtime
}

/// **The background is named, and is in no phase order.** It runs off the
/// frame, so the default order — what the phase loop runs — does not hold it.
#[test]
fn the_background_phase_is_named_and_in_no_phase_order() {
    assert_eq!(ExecutionPhase::BACKGROUND.to_string(), "Background");
    assert!(!ExecutionPhase::DEFAULT_ORDER.contains(&ExecutionPhase::BACKGROUND));
}

/// **A family at the background boots, though no phase order runs it.** Its
/// lane reads a runtime entry and its budget, which outlive a frame. Boot
/// does not refuse it as a family whose phase never runs: the scheduler runs
/// it off the frame, whatever the order — the default one or a game's own.
#[test]
fn a_family_at_the_background_boots_though_no_phase_order_runs_it() {
    let quiet = registration::<QuietFamily>(ExecutionPhase::BACKGROUND, &[off_frame_lane]);

    assert_eq!(
        check_registrations(&[&quiet], &runtime(), &DeviceCaps::default()),
        Ok(())
    );
    let without_output: Vec<ExecutionPhase> = ExecutionPhase::DEFAULT_ORDER
        .iter()
        .copied()
        .filter(|phase| *phase != ExecutionPhase::OUTPUT)
        .collect();
    assert_eq!(
        check_registrations_in(
            &[&quiet],
            &runtime(),
            &DeviceCaps::default(),
            &without_output
        ),
        Ok(())
    );
}

/// **A background lane taking its family's mailbox boots.** It drains the
/// orders the bakery's `Take` mailbox declares, and reads a runtime entry and
/// its budget.
#[test]
fn a_background_lane_taking_its_familys_mailbox_boots() {
    let bake = registration::<BakeFamily>(ExecutionPhase::BACKGROUND, &[orders_lane]);

    assert_eq!(
        check_registrations(&[&bake], &runtime(), &DeviceCaps::default()),
        Ok(())
    );
}

/// **A background lane reads nothing that lives inside one frame.** A run at
/// the background spans frames: a View, the frame's target, a deck slot, a
/// resident or a lock is one frame's, and the run would read it after that
/// frame ended. Boot refuses each, naming the lane, the field and the source.
#[test]
fn boot_refuses_a_background_lane_reading_what_lives_inside_a_frame() {
    let cases: [(LaneConstructor, &str, &str, SourceKind); 5] = [
        (bus_lane, "BusLane", "view", SourceKind::Bus),
        (frame_lane, "FrameLane", "target", SourceKind::Frame),
        (deck_lane, "DeckLane", "earlier", SourceKind::Deck),
        (
            resident_lane,
            "ResidentLane",
            "glazes",
            SourceKind::Resident,
        ),
        (lock_lane, "LockLane", "counter", SourceKind::Locked),
    ];
    for (make, lane_name, field_name, kind) in cases {
        let lanes: &'static [LaneConstructor] = Box::leak(Box::new([make]));
        let quiet = registration::<QuietFamily>(ExecutionPhase::BACKGROUND, lanes);

        match check_registrations(&[&quiet], &runtime(), &DeviceCaps::default()) {
            Err(BootError::BackgroundInput {
                family,
                lane,
                field,
                source,
            }) => {
                assert_eq!(family, "quiet");
                assert!(lane.ends_with(lane_name), "the lane is named: {lane}");
                assert_eq!(field, field_name);
                assert_eq!(source, kind);
            }
            other => panic!("{lane_name}: expected BackgroundInput, got {other:?}"),
        }
    }
}

/// **A `#[take]` drains only a channel its family's `Take` mailbox
/// declares.** The mailbox is what says the family is the one reader of the
/// channel, and what the boundary empties while the family is off. Boot
/// refuses a take of a channel no mailbox declares, of one the family reads
/// by cursor, and of one another family takes — naming the field and the
/// channel.
#[test]
fn boot_refuses_a_take_no_take_mailbox_of_its_family_declares() {
    let cases: [(LaneConstructor, &str, &str, &str); 3] = [
        (unclaimed_lane, "UnclaimedLane", "unclaimed", "Unclaimed"),
        (watched_lane, "WatchedLane", "watched", "Watched"),
        (theirs_lane, "TheirsLane", "theirs", "Theirs"),
    ];
    for (make, lane_name, field_name, message) in cases {
        let lanes: &'static [LaneConstructor] = Box::leak(Box::new([make]));
        let bake = registration::<BakeFamily>(ExecutionPhase::BACKGROUND, lanes);

        match check_registrations(&[&bake], &runtime(), &DeviceCaps::default()) {
            Err(BootError::TakeWithoutMailbox {
                family,
                lane,
                field,
                channel,
            }) => {
                assert_eq!(family, "bake");
                assert!(lane.ends_with(lane_name), "the lane is named: {lane}");
                assert_eq!(field, field_name);
                assert!(channel.contains(message), "the channel is named: {channel}");
            }
            other => panic!("{lane_name}: expected TakeWithoutMailbox, got {other:?}"),
        }
    }
}

/// A family other than the bakery, which takes the bakery's orders too.
const RIVAL: khora_core::agent::gorna::AgentId = khora_core::agent::gorna::AgentId::new("rival");

inventory::submit! {
    khora_core::event::Mailbox::of::<Order>(
        RIVAL,
        khora_core::event::Reader::Take,
        khora_core::event::BetweenSessions::Drop
    )
}

struct RivalFamily;

impl LaneFamily for RivalFamily {
    const NAME: &'static str = "rival";
    const AGENT: khora_core::agent::gorna::AgentId = RIVAL;
    const SEMANTICS: khora_core::lane::family::Semantics =
        khora_core::lane::family::Semantics::Presentation;
    type Outputs<'f> = crate::bakery::Nothing;
}

const RIVAL_LEVELS: &[khora_core::lane::level::Level] = &[crate::fixtures::level(
    khora_core::agent::gorna::StrategyId::Balanced,
    "rival",
)];

/// A frame lane of the rival, taking the orders.
#[derive(Default)]
struct RivalOrdersLane;

impl khora_core::lane::Lane for RivalOrdersLane {
    type Family = RivalFamily;
    type Inputs<'f> = crate::bakery::OnlyOrders;

    fn levels(&self) -> &[khora_core::lane::level::Level] {
        RIVAL_LEVELS
    }

    fn cost(
        &self,
        _level: usize,
        _inputs: &crate::bakery::OnlyOrders,
    ) -> khora_core::lane::cost::Cost {
        khora_core::lane::cost::Cost::default()
    }

    fn record(
        &mut self,
        _level: usize,
        _inputs: &mut crate::bakery::OnlyOrders,
        _out: &mut crate::bakery::Nothing,
    ) -> Result<khora_core::lane::LaneReport, khora_core::lane::LaneError> {
        Ok(khora_core::lane::LaneReport::default())
    }
}

fn rival_orders_lane() -> Box<dyn AnyLane> {
    Box::new(Erased::new(RivalOrdersLane))
}

/// **Boot refuses two families taking one channel.** A `Take` reader is the
/// channel's one reader. The bakery drains its orders off the frame, where no
/// wave's contention orders it against anything; a frame family draining the
/// same channel would split the orders between them by timing alone.
#[test]
fn boot_refuses_two_families_taking_one_channel() {
    let bake = registration::<BakeFamily>(ExecutionPhase::BACKGROUND, &[orders_lane]);
    let rival = registration::<RivalFamily>(ExecutionPhase::OUTPUT, &[rival_orders_lane]);

    let booted = check_registrations(&[&bake, &rival], &runtime(), &DeviceCaps::default());

    assert!(
        booted.is_err(),
        "two families each drain the orders, and boot lets them"
    );
}
