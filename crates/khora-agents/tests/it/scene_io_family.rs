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

//! The `scene_io` family, registered and booted as the engine boots it: a
//! family of its own at `ExecutionPhase::BACKGROUND`, whose lane takes the
//! scene loads a host files and stages what the runtime's `SceneLoading`
//! prepares — off the frame, whether or not the scripting family is on, and
//! giving back every id a load held when the engine winds down before it
//! landed.

use std::any::TypeId;
use std::collections::HashSet;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, RwLock};
use std::time::{Duration, Instant};

use khora_agents::agent::lane_agent;
use khora_agents::registered_agent::{lane_agents, registered_agents};
use khora_control::{AgentRegistry, Context, ExecutionScheduler};
use khora_core::agent::gorna::AgentId;
use khora_core::agent::registration::{check_registrations, AgentRegistration};
use khora_core::agent::{Activation, Agent, AgentImportance, ExecutionPhase};
use khora_core::event::{BetweenSessions, Channel, Mailbox, Reader, WhenFull};
use khora_core::gpu::DeviceCaps;
use khora_core::lane::family::LaneFamily;
use khora_core::lane::guard::{SinkKind, SourceKind};
use khora_core::lane::{Lease, Reserve};
use khora_core::scene::SerializationGoal;
use khora_core::Runtime;
use khora_data::ecs::{Name, Transform, World};
use khora_data::scene::loading::{
    LoadKind, LoadTicket, PreparedLoad, SceneFetch, SceneLoadRequest, SceneLoading, SceneSource,
};
use khora_data::scene::NoPrefabs;
use khora_io::scene_loads::{install_scene_loading, SceneLoadHub};
use khora_io::serialization::SerializationService;
use khora_lanes::scene_io_lane::SceneIoFamily;

/// The `scene_io` family's agent, by name.
const SCENE_IO: AgentId = AgentId::new("scene_io");

/// How long a test waits for the scheduler to run the background.
const SETTLE: Duration = Duration::from_secs(5);

// ─── Fixtures ───────────────────────────────────────────────────────────────

/// The `scene_io` family's registration, as the engine collects it.
fn scene_io_registration() -> &'static AgentRegistration {
    let found: Vec<&'static AgentRegistration> = registered_agents()
        .into_iter()
        .filter(|registration| registration.family == TypeId::of::<SceneIoFamily>())
        .collect();
    match found.as_slice() {
        [one] => one,
        [] => panic!(
            "the scene_io family is not registered: no registration of `SceneIoFamily` was submitted"
        ),
        many => panic!("the scene_io family is registered {} times", many.len()),
    }
}

/// A `SceneLoading` answering every request with a failed load naming its
/// ticket, remembering the thread it prepared on.
#[derive(Default)]
struct WatchedLoading {
    threads: Mutex<Vec<std::thread::ThreadId>>,
}

impl SceneLoading for WatchedLoading {
    fn prepare_leased(&self, request: SceneLoadRequest) -> (PreparedLoad, Option<Lease>) {
        self.threads
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .push(std::thread::current().id());
        (
            PreparedLoad {
                ticket: request.ticket,
                replaces: true,
                outcome: Err(format!("load {} prepared by the watch", request.ticket.0)),
                refile: None,
            },
            None,
        )
    }
}

/// A runtime as the engine wires the `scene_io` family over a watched
/// loading: the request channel, the loading, the reserve — and
/// `activation`, when given.
struct Wired {
    runtime: Arc<Runtime>,
    requests: Channel<SceneLoadRequest>,
    reserve: Reserve,
    loading: Arc<WatchedLoading>,
}

fn wired(activation: Option<Arc<Activation>>) -> Wired {
    let requests = Channel::<SceneLoadRequest>::bounded(64, WhenFull::Reject);
    let reserve = Reserve::new();
    let loading = Arc::new(WatchedLoading::default());
    let mut runtime = Runtime::new();
    runtime.resources.insert(requests.clone());
    runtime.resources.insert(reserve.clone());
    runtime
        .resources
        .insert(Arc::clone(&loading) as Arc<dyn SceneLoading>);
    if let Some(activation) = activation {
        runtime.resources.insert(activation);
    }
    Wired {
        runtime: Arc::new(runtime),
        requests,
        reserve,
        loading,
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

/// The `scene_io` family's agent under the real scheduler.
fn scheduler_of(registration: &'static AgentRegistration) -> ExecutionScheduler {
    let agent: Arc<Mutex<dyn Agent>> =
        Arc::new(Mutex::new(lane_agent(registration, &DeviceCaps::default())));
    let registry = Arc::new(Mutex::new(AgentRegistry::new()));
    registry
        .lock()
        .expect("the registry lock")
        .register(agent, 1.0);
    ExecutionScheduler::from_registry(registry, Arc::new(RwLock::new(Context::default())))
}

/// Whole frames, as the host runs them — the frame start, then the rest —
/// until a frame start brings prepared loads into the reserve: they are
/// taken and returned. Panics after [`SETTLE`].
fn frames_until_prepared(
    scheduler: &mut ExecutionScheduler,
    world: &mut World,
    runtime: &Arc<Runtime>,
    reserve: &Reserve,
) -> Vec<PreparedLoad> {
    let deadline = Instant::now() + SETTLE;
    loop {
        scheduler.begin_frame(runtime);
        let taken = reserve.take::<PreparedLoad>();
        if !taken.is_empty() {
            return taken;
        }
        scheduler.run_frame(world, Arc::clone(runtime));
        assert!(
            Instant::now() < deadline,
            "timed out waiting for the scene load to reach the reserve"
        );
        std::thread::sleep(Duration::from_millis(5));
    }
}

// ─── The registration ───────────────────────────────────────────────────────

/// **The `scene_io` family registers once, at the background**: its own
/// agent (`scene_io`), optional, with one lane, written in `khora-agents`'
/// `scene_io_agent/agent.rs`. Its requests are taken from a mailbox it
/// holds between sessions — a load filed while it is off is prepared once
/// it is back on, never lost. Its lane reads only its requests, its loading
/// and its budget — no `World`, no bus, no lock, no deck — and only stages
/// the loads it prepares.
#[test]
fn scene_io_family_registers_background_optional_one_lane() {
    let registration = scene_io_registration();

    assert_eq!(registration.family_name, "scene_io");
    assert_eq!(registration.agent, SCENE_IO);
    assert_eq!(SceneIoFamily::AGENT, SCENE_IO);
    assert_eq!(registration.phase, ExecutionPhase::BACKGROUND);
    assert_eq!(registration.importance, AgentImportance::Optional);
    assert_eq!(registration.fixed_step, None);
    assert!(
        registration.module.ends_with("scene_io_agent::agent"),
        "registered in `scene_io_agent/agent.rs`: {}",
        registration.module
    );
    let lanes: Vec<&'static str> = registration
        .lanes
        .iter()
        .map(|make| make().name())
        .collect();
    assert_eq!(lanes.len(), 1, "one lane: {lanes:?}");
    assert!(
        lanes[0].ends_with("SceneIoLane"),
        "the scene load lane: {lanes:?}"
    );

    let mailboxes: Vec<&Mailbox> = khora_script::inventory::iter::<Mailbox>
        .into_iter()
        .filter(|mailbox| (mailbox.message)().ends_with("::SceneLoadRequest"))
        .collect();
    assert_eq!(
        mailboxes.len(),
        1,
        "one mailbox of `SceneLoadRequest`: {mailboxes:?}"
    );
    assert_eq!(
        (
            mailboxes[0].family,
            mailboxes[0].reader,
            mailboxes[0].between_sessions
        ),
        (SCENE_IO, Reader::Take, BetweenSessions::Hold)
    );

    let lane = (registration.lanes[0])();
    let declaration = lane.declaration();
    let inputs: Vec<(SourceKind, TypeId)> = declaration
        .inputs()
        .iter()
        .map(|input| (input.kind, input.ty))
        .collect();
    assert!(
        inputs.iter().all(|(kind, _)| matches!(
            kind,
            SourceKind::Take | SourceKind::Resource | SourceKind::Budget
        )),
        "only takes, resources and the budget: {:?}",
        declaration.inputs()
    );
    assert!(
        inputs.contains(&(SourceKind::Take, TypeId::of::<Channel<SceneLoadRequest>>())),
        "the requests, taken: {:?}",
        declaration.inputs()
    );
    assert!(
        inputs.contains(&(SourceKind::Resource, TypeId::of::<Arc<dyn SceneLoading>>())),
        "the loading, read: {:?}",
        declaration.inputs()
    );
    let outputs: Vec<(SinkKind, TypeId)> = declaration
        .outputs()
        .iter()
        .map(|output| (output.kind, output.ty))
        .collect();
    assert_eq!(
        outputs,
        vec![(SinkKind::Staged, TypeId::of::<PreparedLoad>())],
        "the prepared loads, staged: {:?}",
        declaration.outputs()
    );
}

/// **Boot accepts the `scene_io` family** on a runtime holding its request
/// channel and its loading, and builds one agent from it.
#[test]
fn boot_accepts_the_scene_io_family() {
    let wired = wired(None);
    let registration = scene_io_registration();

    assert_eq!(
        check_registrations(&[registration], &wired.runtime, &DeviceCaps::default()),
        Ok(())
    );
    let agents = lane_agents(&[registration], &wired.runtime, &DeviceCaps::default())
        .expect("the scene_io family boots");
    assert_eq!(agents.len(), 1);
    assert_eq!(agents[0].id(), SCENE_IO);
}

// ─── Under the scheduler ────────────────────────────────────────────────────

/// **A scene load is prepared off the frame thread.** The family's agent
/// under the real scheduler: a load filed before the first frame is taken,
/// prepared by the runtime's `SceneLoading` on another thread than the one
/// running the frames, and reaches the reserve within a bounded number of
/// frames — the request channel left empty.
#[test]
fn a_scene_load_is_prepared_off_the_frame_thread() {
    let wired = wired(None);
    let mut scheduler = scheduler_of(scene_io_registration());
    let mut world = World::new();
    wired.requests.send(request(1));

    let prepared =
        frames_until_prepared(&mut scheduler, &mut world, &wired.runtime, &wired.reserve);

    let tickets: Vec<LoadTicket> = prepared.iter().map(|load| load.ticket).collect();
    assert_eq!(tickets, vec![LoadTicket(1)]);
    assert!(wired.requests.is_empty(), "the request was taken");
    let frame_thread = std::thread::current().id();
    let threads = wired
        .loading
        .threads
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .clone();
    assert!(
        !threads.is_empty() && threads.iter().all(|thread| *thread != frame_thread),
        "prepared off the thread running the frames: {threads:?}"
    );
}

/// **A scene load is prepared while the scripting family is off**: an
/// editor editing a scene runs no script, yet the scene it opens — or the
/// one Stop restores — is prepared.
#[test]
fn a_scene_load_is_prepared_while_the_scripting_family_is_off() {
    let activation = Arc::new(Activation::new([SCENE_IO, khora_core::script::SCRIPT]));
    activation
        .deactivate(khora_core::script::SCRIPT)
        .expect("a known family");
    activation.settle();
    let wired = wired(Some(Arc::clone(&activation)));
    let mut scheduler = scheduler_of(scene_io_registration());
    let mut world = World::new();
    wired.requests.send(request(2));

    let prepared =
        frames_until_prepared(&mut scheduler, &mut world, &wired.runtime, &wired.reserve);

    assert!(
        !activation.is_active(khora_core::script::SCRIPT),
        "the premise: the scripts stayed off"
    );
    assert_eq!(prepared.len(), 1, "prepared while the scripts were off");
}

// ─── Shutdown ───────────────────────────────────────────────────────────────

/// A scene's bytes, read off the frame only once the test lets them: it
/// signals when the read started, and waits to be released (at most a few
/// seconds).
struct HeldFetch {
    bytes: Vec<u8>,
    entered: AtomicBool,
    released: AtomicBool,
}

impl SceneFetch for HeldFetch {
    fn fetch(&self) -> Result<Vec<u8>, String> {
        self.entered.store(true, Ordering::SeqCst);
        let deadline = Instant::now() + SETTLE;
        while !self.released.load(Ordering::SeqCst) && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(1));
        }
        Ok(self.bytes.clone())
    }
}

/// A scene of `count` authored entities, as a file's bytes.
fn scene_of(count: usize) -> Vec<u8> {
    let mut src = World::new();
    for i in 0..count {
        let entity = src.spawn((Transform::identity(), Name::new(format!("Loaded {i}"))));
        src.mark_authored(entity).expect("authored");
    }
    SerializationService::new()
        .save_world(&src, SerializationGoal::EditorInterchange)
        .expect("the scene saves")
        .to_bytes()
}

/// **A load in flight when the engine winds down gives its ids back.** The
/// real scene loading under the scheduler: eight entities despawned, then a
/// scene of eight filed — its ids held from the freed slots. The engine
/// winds its background down while the load is being read: the prepared
/// load is dropped, never committed, and the next spawns are handed the
/// freed slots again rather than indices past the end of the store.
#[test]
fn a_load_in_flight_at_shutdown_gives_its_ids_back() {
    let mut runtime = Runtime::new();
    let reserve = Reserve::new();
    runtime.resources.insert(reserve.clone());
    install_scene_loading(&mut runtime);
    let runtime = Arc::new(runtime);
    let hub = runtime
        .resources
        .get::<Arc<SceneLoadHub>>()
        .cloned()
        .expect("the runtime holds the scene load hub");
    let mut scheduler = scheduler_of(scene_io_registration());
    let mut world = World::new();
    let spawned: Vec<_> = (0..8).map(|_| world.spawn(Transform::identity())).collect();
    for entity in &spawned {
        assert!(world.despawn(*entity));
    }
    let freed: HashSet<u32> = spawned.iter().map(|entity| entity.index).collect();
    let fetch = Arc::new(HeldFetch {
        bytes: scene_of(8),
        entered: AtomicBool::new(false),
        released: AtomicBool::new(false),
    });
    let _ticket = hub.file(
        &mut world,
        LoadKind::Replace(SceneSource::Fetch(Arc::clone(&fetch) as Arc<dyn SceneFetch>)),
        Arc::new(NoPrefabs),
    );

    let deadline = Instant::now() + SETTLE;
    while !fetch.entered.load(Ordering::SeqCst) {
        scheduler.begin_frame(&runtime);
        scheduler.run_frame(&mut world, Arc::clone(&runtime));
        assert!(
            Instant::now() < deadline,
            "timed out waiting for the load to start"
        );
        std::thread::sleep(Duration::from_millis(5));
    }
    fetch.released.store(true, Ordering::SeqCst);
    scheduler.wind_down();

    assert!(
        reserve.is_empty::<PreparedLoad>(),
        "a load wound down never reaches the reserve"
    );
    assert_eq!(world.entity_count(), 0, "nothing was committed");
    let respawned: HashSet<u32> = (0..8)
        .map(|_| world.spawn(Transform::identity()).index)
        .collect();
    assert_eq!(
        respawned, freed,
        "the wound-down load gave the freed slots back"
    );
}
