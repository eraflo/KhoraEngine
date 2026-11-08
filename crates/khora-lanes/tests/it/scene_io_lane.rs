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

//! **The scene load lane hands the requests it takes to the runtime's
//! `SceneLoading`, and stages each prepared load under the lease it holds
//! to.** Run by hand as the scheduler runs a background lane: its inputs
//! fetched from a runtime holding the request channel and a fake
//! [`SceneLoading`], its outputs committed into a staging the reserve then
//! receives. A load whose lease broke while it waited is abandoned there,
//! and its request filed again, without its hold.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use khora_core::ecs::entity::EntityId;
use khora_core::event::{Channel, WhenFull};
use khora_core::lane::contract::Lane;
use khora_core::lane::io::{FrameId, InputSource, LaneInputs, LaneOutputs, OutputSink};
use khora_core::lane::{LaneBus, Lease, OutputDeck, Reserve, Staging};
use khora_core::Runtime;
use khora_data::ecs::World;
use khora_data::scene::loading::{
    LoadKind, LoadTicket, PreparedLoad, Refile, SceneLoadRequest, SceneLoading, SceneSource,
};
use khora_data::scene::NoPrefabs;
use khora_lanes::scene_io_lane::{SceneIoInputs, SceneIoLane, SceneIoOutputs};

/// A `SceneLoading` that prepares nothing: each request comes back as a
/// failed load naming its ticket, leased on a flag the test may lower —
/// standing for the world's components changing — and set to be filed again
/// on `requests`. It remembers the tickets it was asked, in order, and
/// reserves one id through each request's hold, if it carries one.
struct FakeLoading {
    requests: Channel<SceneLoadRequest>,
    current: Arc<AtomicBool>,
    asked: Mutex<Vec<LoadTicket>>,
    reserved: Mutex<Vec<EntityId>>,
}

impl FakeLoading {
    fn new(requests: &Channel<SceneLoadRequest>) -> Arc<Self> {
        Arc::new(Self {
            requests: requests.clone(),
            current: Arc::new(AtomicBool::new(true)),
            asked: Mutex::new(Vec::new()),
            reserved: Mutex::new(Vec::new()),
        })
    }

    fn asked(&self) -> Vec<LoadTicket> {
        self.asked
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone()
    }

    fn reserved(&self) -> Vec<EntityId> {
        self.reserved
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone()
    }

    /// The world's components changed since the loads were prepared.
    fn change(&self) {
        self.current.store(false, Ordering::SeqCst);
    }
}

impl SceneLoading for FakeLoading {
    fn prepare_leased(&self, request: SceneLoadRequest) -> (PreparedLoad, Option<Lease>) {
        self.asked
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .push(request.ticket);
        if let Some(hold) = &request.hold {
            self.reserved
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .push(hold.reserve());
        }
        let current = Arc::clone(&self.current);
        let prepared = PreparedLoad {
            ticket: request.ticket,
            replaces: matches!(request.kind, LoadKind::Replace(_)),
            outcome: Err(format!("load {} prepared by the fake", request.ticket.0)),
            refile: Some(Refile {
                requests: self.requests.clone(),
                request,
            }),
        };
        (
            prepared,
            Some(Lease::new(move || current.load(Ordering::SeqCst))),
        )
    }
}

fn request(ticket: u64) -> SceneLoadRequest {
    SceneLoadRequest {
        ticket: LoadTicket(ticket),
        kind: LoadKind::Replace(SceneSource::Bytes(Arc::from(&b"scene"[..]))),
        prefabs: Arc::new(NoPrefabs),
        hold: None,
    }
}

/// A runtime holding the request channel, filled with `requests`, and the
/// fake loading over it, as the engine installs the hub.
fn runtime_with(
    requests: Vec<SceneLoadRequest>,
) -> (Runtime, Channel<SceneLoadRequest>, Arc<FakeLoading>) {
    let channel = Channel::<SceneLoadRequest>::bounded(64, WhenFull::Reject);
    for request in requests {
        channel.send(request);
    }
    let loading = FakeLoading::new(&channel);
    let mut runtime = Runtime::new();
    runtime.resources.insert(channel.clone());
    runtime
        .resources
        .insert(Arc::clone(&loading) as Arc<dyn SceneLoading>);
    (runtime, channel, loading)
}

/// One run of the lane over `runtime`, as the scheduler runs it at the
/// background: its inputs fetched, the lane recorded, its outputs committed
/// into the staging returned — not yet received by a reserve.
fn run(runtime: &Runtime) -> Staging {
    let bus = LaneBus::new();
    let mut source = InputSource::new(FrameId(0), &bus, runtime);
    let mut inputs = SceneIoInputs::fetch(&mut source).expect("the scene load lane's inputs");
    let mut deck = OutputDeck::new();
    let mut staging = Staging::new();
    {
        let mut sink = OutputSink::new(&mut deck).with_staging(&mut staging);
        let mut outputs = SceneIoOutputs::open(&mut sink).expect("the scene load lane's outputs");
        let mut lane = SceneIoLane;
        lane.record(0, &mut inputs, &mut outputs)
            .expect("the run records");
        outputs.commit(&mut sink).expect("the outputs are staged");
    }
    staging
}

/// What `staging` lands in a fresh reserve, as the frame start receives it.
fn landed(staging: Staging) -> Vec<PreparedLoad> {
    let reserve = Reserve::new();
    reserve.receive(staging);
    reserve.take::<PreparedLoad>()
}

/// **A run prepares every request it takes and stages one load each.** Two
/// loads wait: the runtime's `SceneLoading` is asked for both, in the order
/// they were filed, each prepared load reaches the reserve under its own
/// ticket and kind, and the request channel is left empty.
#[test]
fn a_run_prepares_every_request_it_takes_and_stages_one_load_each() {
    let mut add = request(2);
    add.kind = LoadKind::Add(SceneSource::Bytes(Arc::from(&b"more"[..])));
    let (runtime, channel, loading) = runtime_with(vec![request(1), add]);

    let prepared = landed(run(&runtime));

    assert_eq!(loading.asked(), vec![LoadTicket(1), LoadTicket(2)]);
    let got: Vec<(LoadTicket, bool)> = prepared
        .iter()
        .map(|load| (load.ticket, load.replaces))
        .collect();
    assert_eq!(got, vec![(LoadTicket(1), true), (LoadTicket(2), false)]);
    assert!(channel.is_empty(), "the requests were taken");
}

/// **A load whose lease broke while it waited is filed again, without its
/// hold.** A load prepared with a hold is staged; the world's components
/// change before the frame start receives it: it never reaches the
/// reserve, its request is back on the channel under the same ticket and
/// with no hold, and the id its hold reserved is handed out again.
#[test]
fn a_load_whose_lease_broke_is_filed_again_without_its_hold() {
    let world = World::new();
    let mut held = request(7);
    held.hold = Some(world.hold_reservations());
    let (runtime, channel, loading) = runtime_with(vec![held]);

    let staging = run(&runtime);
    loading.change();
    let prepared = landed(staging);

    assert!(
        prepared.is_empty(),
        "an abandoned load never lands: {prepared:?}"
    );
    let refiled = channel.drain();
    assert_eq!(refiled.len(), 1, "filed again once: {refiled:?}");
    assert_eq!(refiled[0].ticket, LoadTicket(7));
    assert!(refiled[0].hold.is_none(), "filed again without its hold");
    let reserved = loading.reserved();
    assert_eq!(reserved.len(), 1, "the fake reserved one id");
    assert_eq!(
        world.reserver().reserve(),
        reserved[0],
        "the abandoned load's hold gave its id back"
    );
}

/// **A run with no request prepares nothing and stages nothing.**
#[test]
fn a_run_without_a_request_prepares_nothing() {
    let (runtime, _channel, loading) = runtime_with(Vec::new());

    let prepared = landed(run(&runtime));

    assert!(loading.asked().is_empty(), "nothing was asked");
    assert!(prepared.is_empty());
}
