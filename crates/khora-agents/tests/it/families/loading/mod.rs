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

//! The `loading` family, registered and booted as the engine boots it: one
//! family at `ExecutionPhase::BACKGROUND` whose two lanes — rush and paced —
//! are two ways of loading the same requests. It takes the scene loads a
//! host files, the compiles the hot-reload pump files and the decodes the
//! asset service files, and stages what the runtime's `SceneLoading`,
//! `ScriptCompiler` and `AssetDecoding` return — off the frame, whether or
//! not the scripting family is on, and giving back every id a load held when
//! the engine winds down before it landed. GORNA picks rush when the
//! envelope covers the work, paced when it does not.

use std::any::{Any, TypeId};
use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use khora_agents::lane_agent::{choose, lane_agent, Choice};
use khora_agents::registered_agent::{lane_agents, registered_agents};
use khora_control::{AgentRegistry, ExecutionScheduler};
use khora_core::agent::gorna::protocol::Envelope;
use khora_core::agent::gorna::StrategyId;
use khora_core::agent::registration::{check_registrations, AgentRegistration};
use khora_core::agent::{Activation, Agent, AgentImportance, ExecutionPhase};
use khora_core::asset::{
    AssetDecoding, AssetIndexing, AssetUUID, DecodeRequest, Decoded, IndexRequest, IndexedAssets,
};
use khora_core::event::{BetweenSessions, Channel, Mailbox, Reader, WhenFull};
use khora_core::gpu::DeviceCaps;
use khora_core::lane::cost::{Cost, PerDevice};
use khora_core::lane::erased::{AnyLane, Erased};
use khora_core::lane::family::LaneFamily;
use khora_core::lane::guard::{SinkKind, SourceKind};
use khora_core::lane::io::{Budget, FrameId, InputSource};
use khora_core::lane::{LaneBus, Lease, Reserve, Staged};
use khora_core::scene::SerializationGoal;
use khora_core::Runtime;
use khora_data::ecs::{Name, Transform, World};
use khora_data::scene::loading::{
    LoadKind, LoadTicket, PreparedLoad, SceneFetch, SceneLoadRequest, SceneLoading, SceneSource,
};
use khora_data::scene::NoPrefabs;
use khora_io::scene_loads::{install_scene_loading, SceneLoadHub};
use khora_io::serialization::SerializationService;
use khora_lanes::loading::LOADING;
use khora_lanes::loading::{LoadingFamily, PacedLane, RushLane};
use khora_script::reload::ScriptReload;
use khora_script::toolchain::{
    CompileDone, CompileRequest, Recompiled, ScriptCompiler, ScriptDeclarations,
};
use khora_script::vm::Program;

/// How long a test waits for the scheduler to run the background.
const SETTLE: Duration = Duration::from_secs(5);

// ─── Fixtures ───────────────────────────────────────────────────────────────

/// The `loading` family's registration, as the engine collects it.
fn loading_registration() -> &'static AgentRegistration {
    let found: Vec<&'static AgentRegistration> = registered_agents()
        .into_iter()
        .filter(|registration| registration.family == TypeId::of::<LoadingFamily>())
        .collect();
    match found.as_slice() {
        [one] => one,
        [] => panic!(
            "the loading family is not registered: no registration of `LoadingFamily` was submitted"
        ),
        many => panic!("the loading family is registered {} times", many.len()),
    }
}

/// The threads a loader ran on.
#[derive(Default)]
struct Threads(Mutex<Vec<std::thread::ThreadId>>);

impl Threads {
    fn note(&self) {
        self.0
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .push(std::thread::current().id());
    }

    fn all(&self) -> Vec<std::thread::ThreadId> {
        self.0
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone()
    }

    /// Whether the loader ran, and never on the thread running the frames.
    fn all_off(&self, frame_thread: std::thread::ThreadId) -> bool {
        let all = self.all();
        !all.is_empty() && all.iter().all(|thread| *thread != frame_thread)
    }
}

/// A decoder answering every request with the asset's uuid as its value,
/// remembering the thread it decoded on.
#[derive(Default)]
struct EchoDecoding {
    threads: Threads,
}

impl AssetDecoding for EchoDecoding {
    fn decode(&self, uuid: &AssetUUID, type_id: TypeId) -> Decoded {
        self.threads.note();
        Decoded {
            uuid: *uuid,
            type_id,
            value: Ok(Box::new(*uuid) as Box<dyn Any + Send>),
            external_files: Vec::new(),
        }
    }
}

/// A compiler answering every request with a program and an empty set of
/// declarations for each module it names, remembering the thread it compiled
/// on.
#[derive(Default)]
struct EchoCompiler {
    threads: Threads,
}

impl ScriptCompiler for EchoCompiler {
    fn compile(&self, request: &CompileRequest) -> Recompiled {
        self.threads.note();
        Recompiled {
            reloads: request
                .modules
                .iter()
                .map(|module| ScriptReload {
                    module: module.clone(),
                    program: Some(Program::default()),
                })
                .collect(),
            declarations: request
                .modules
                .iter()
                .map(|module| ScriptDeclarations {
                    module: module.clone(),
                    components: Vec::new(),
                })
                .collect(),
        }
    }
}

/// A `SceneLoading` answering every request with a failed load naming its
/// ticket, remembering the thread it prepared on.
#[derive(Default)]
struct WatchedLoading {
    threads: Threads,
}

impl SceneLoading for WatchedLoading {
    fn prepare_leased(&self, request: SceneLoadRequest) -> (PreparedLoad, Option<Lease>) {
        self.threads.note();
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

/// An indexer answering every request with an empty index, remembering the
/// thread it indexed on.
#[derive(Default)]
struct WatchedIndexing {
    threads: Threads,
}

impl AssetIndexing for WatchedIndexing {
    fn index_leased(&self, request: &IndexRequest) -> (IndexedAssets, Option<Lease>) {
        self.threads.note();
        (
            IndexedAssets {
                ticket: request.ticket,
                kind: request.kind,
                project_root: request.project_root.clone(),
                index: Ok(Vec::new()),
            },
            None,
        )
    }
}

/// Installs in `runtime` an idle index request channel and an indexer: the
/// family's lanes read them whatever kind of load a test files.
fn install_idle_indexing(runtime: &mut Runtime) {
    runtime
        .resources
        .insert(Channel::<IndexRequest>::bounded(8, WhenFull::Reject));
    runtime
        .resources
        .insert(Arc::new(WatchedIndexing::default()) as Arc<dyn AssetIndexing>);
}

/// A runtime as the engine wires the `loading` family over watched loaders:
/// the four request channels, the four loaders, the reserve — and
/// `activation`, when given.
struct Wired {
    runtime: Arc<Runtime>,
    scenes: Channel<SceneLoadRequest>,
    scripts: Channel<CompileRequest>,
    assets: Channel<DecodeRequest>,
    reserve: Reserve,
    loading: Arc<WatchedLoading>,
    compiler: Arc<EchoCompiler>,
    decoding: Arc<EchoDecoding>,
}

impl Wired {
    fn all_taken(&self) -> bool {
        self.scenes.is_empty() && self.scripts.is_empty() && self.assets.is_empty()
    }
}

fn wired(activation: Option<Arc<Activation>>) -> Wired {
    let scenes = Channel::<SceneLoadRequest>::bounded(64, WhenFull::Reject);
    let scripts = Channel::<CompileRequest>::bounded(64, WhenFull::DropOldest);
    let assets = Channel::<DecodeRequest>::bounded(64, WhenFull::DropOldest);
    let reserve = Reserve::new();
    let loading = Arc::new(WatchedLoading::default());
    let compiler = Arc::new(EchoCompiler::default());
    let decoding = Arc::new(EchoDecoding::default());
    let mut runtime = Runtime::new();
    install_idle_indexing(&mut runtime);
    runtime.resources.insert(scenes.clone());
    runtime.resources.insert(scripts.clone());
    runtime.resources.insert(assets.clone());
    runtime.resources.insert(reserve.clone());
    runtime
        .resources
        .insert(Arc::clone(&loading) as Arc<dyn SceneLoading>);
    runtime
        .resources
        .insert(Arc::clone(&compiler) as Arc<dyn ScriptCompiler>);
    runtime
        .resources
        .insert(Arc::clone(&decoding) as Arc<dyn AssetDecoding>);
    if let Some(activation) = activation {
        runtime.resources.insert(activation);
    }
    Wired {
        runtime: Arc::new(runtime),
        scenes,
        scripts,
        assets,
        reserve,
        loading,
        compiler,
        decoding,
    }
}

fn scene(ticket: u64) -> SceneLoadRequest {
    SceneLoadRequest {
        ticket: LoadTicket(ticket),
        kind: LoadKind::Replace(SceneSource::Bytes(Arc::from(&b"scene"[..]))),
        prefabs: Arc::new(NoPrefabs),
        hold: None,
    }
}

fn script(root: &str, modules: &[&str]) -> CompileRequest {
    CompileRequest::new(
        PathBuf::from(root),
        modules.iter().map(|module| (*module).to_owned()),
    )
}

fn asset(path: &str) -> DecodeRequest {
    DecodeRequest {
        uuid: AssetUUID::new_v5(path),
        type_id: TypeId::of::<AssetUUID>(),
        generation: 0,
    }
}

/// The `loading` family's agent under the real scheduler.
fn scheduler_of(registration: &'static AgentRegistration) -> ExecutionScheduler {
    let agent: Arc<Mutex<dyn Agent>> =
        Arc::new(Mutex::new(lane_agent(registration, &DeviceCaps::default())));
    let registry = Arc::new(Mutex::new(AgentRegistry::new()));
    registry
        .lock()
        .expect("the registry lock")
        .register(agent, 1.0);
    ExecutionScheduler::from_registry(registry)
}

/// Whole frames, as the host runs them — the frame start, then the rest —
/// until a frame start brings values of `T` into the reserve: they are taken
/// and returned. Panics after [`SETTLE`].
fn frames_until_landed<T: Staged>(
    scheduler: &mut ExecutionScheduler,
    world: &mut World,
    runtime: &Arc<Runtime>,
    reserve: &Reserve,
) -> Vec<T> {
    let deadline = Instant::now() + SETTLE;
    loop {
        scheduler.begin_frame(runtime);
        let taken = reserve.take::<T>();
        if !taken.is_empty() {
            return taken;
        }
        scheduler.run_frame(world, Arc::clone(runtime));
        assert!(
            Instant::now() < deadline,
            "timed out waiting for {} to reach the reserve",
            std::any::type_name::<T>()
        );
        std::thread::sleep(Duration::from_millis(5));
    }
}

// ─── The registration ───────────────────────────────────────────────────────

/// **The `loading` family registers two lanes in the background**: its own
/// agent (`loading`, not the scripting family's), optional, every run on
/// request, its lanes the rush lane then the paced lane, rush by default —
/// until a quote exists, it loads everything filed, as the three families it
/// replaces did. Its four kinds of request are each taken from a mailbox
/// it holds between sessions: a request filed while it is off is loaded once
/// it is back on, never lost.
#[test]
fn the_loading_family_registers_two_lanes_in_the_background() {
    let registration = loading_registration();

    assert_eq!(registration.family_name, "loading");
    assert_eq!(registration.agent, LOADING);
    assert_eq!(LoadingFamily::AGENT, LOADING);
    assert_eq!(LOADING, khora_core::agent::gorna::AgentId::new("loading"));
    assert_ne!(
        registration.agent,
        khora_core::script::SCRIPT,
        "a family of its own: it runs while the scripts are off"
    );
    assert_eq!(registration.phase, ExecutionPhase::BACKGROUND);
    assert_eq!(registration.importance, AgentImportance::Optional);
    assert_eq!(registration.fixed_step, None);
    assert_eq!(
        registration.default_level,
        Some(StrategyId::HighPerformance),
        "rush until a quote exists"
    );
    assert!(
        registration.module == "khora_lanes::loading::registration",
        "registered in its family folder, `khora-lanes/src/loading/`: {}",
        registration.module
    );
    let lanes: Vec<&'static str> = registration
        .lanes
        .iter()
        .map(|make| make().name())
        .collect();
    assert_eq!(lanes.len(), 2, "two lanes: {lanes:?}");
    assert!(lanes[0].ends_with("RushLane"), "rush first: {lanes:?}");
    assert!(lanes[1].ends_with("PacedLane"), "then paced: {lanes:?}");

    for message in [
        "::IndexRequest",
        "::DecodeRequest",
        "::SceneLoadRequest",
        "::CompileRequest",
    ] {
        let found: Vec<&Mailbox> = khora_script::inventory::iter::<Mailbox>
            .into_iter()
            .filter(|mailbox| (mailbox.message)().ends_with(message))
            .collect();
        assert_eq!(found.len(), 1, "one mailbox of `{message}`: {found:?}");
        assert_eq!(
            (found[0].family, found[0].reader, found[0].between_sessions),
            (LOADING, Reader::Take, BetweenSessions::Hold),
            "`{message}`"
        );
    }
}

/// **The loading lanes read only their requests, their loaders and their
/// budget, and only stage what they load**: no `World`, no bus, no lock, no
/// deck — what a lane spanning frames may hold. Both lanes read the same
/// inputs and stage the same outputs: they are two ways of loading the same
/// requests.
#[test]
fn the_loading_lanes_read_their_requests_their_loaders_and_their_budget_and_stage() {
    for make in loading_registration().lanes {
        let lane = make();
        let declaration = lane.declaration();
        let name = lane.name();

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
            "{name}: only takes, resources and the budget: {:?}",
            declaration.inputs()
        );
        for taken in [
            TypeId::of::<Channel<SceneLoadRequest>>(),
            TypeId::of::<Channel<CompileRequest>>(),
            TypeId::of::<Channel<DecodeRequest>>(),
        ] {
            assert!(
                inputs.contains(&(SourceKind::Take, taken)),
                "{name}: each kind of request, taken: {:?}",
                declaration.inputs()
            );
        }
        for loader in [
            TypeId::of::<Arc<dyn SceneLoading>>(),
            TypeId::of::<Arc<dyn ScriptCompiler>>(),
            TypeId::of::<Arc<dyn AssetDecoding>>(),
        ] {
            assert!(
                inputs.contains(&(SourceKind::Resource, loader)),
                "{name}: each loader, read: {:?}",
                declaration.inputs()
            );
        }
        let outputs: Vec<(SinkKind, TypeId)> = declaration
            .outputs()
            .iter()
            .map(|output| (output.kind, output.ty))
            .collect();
        assert_eq!(outputs.len(), 6, "{name}: {:?}", declaration.outputs());
        for staged in [
            TypeId::of::<Decoded>(),
            TypeId::of::<PreparedLoad>(),
            TypeId::of::<ScriptReload>(),
            TypeId::of::<ScriptDeclarations>(),
            TypeId::of::<CompileDone>(),
        ] {
            assert!(
                outputs.contains(&(SinkKind::Staged, staged)),
                "{name}: every result, staged: {:?}",
                declaration.outputs()
            );
        }
    }
}

/// **The loading lanes read the indexer**: each lane's declaration takes the
/// `IndexRequest`s, reads the runtime's `AssetIndexing` and stages
/// `IndexedAssets`; the `loading` family holds a mailbox of `IndexRequest`,
/// taken, held between sessions — an index filed while the family is off is
/// built once it is back on.
#[test]
fn the_loading_lanes_read_the_indexer() {
    for make in loading_registration().lanes {
        let lane = make();
        let declaration = lane.declaration();
        let name = lane.name();
        let inputs: Vec<(SourceKind, TypeId)> = declaration
            .inputs()
            .iter()
            .map(|input| (input.kind, input.ty))
            .collect();
        assert!(
            inputs.contains(&(SourceKind::Take, TypeId::of::<Channel<IndexRequest>>())),
            "{name}: the index requests, taken: {:?}",
            declaration.inputs()
        );
        assert!(
            inputs.contains(&(SourceKind::Resource, TypeId::of::<Arc<dyn AssetIndexing>>())),
            "{name}: the indexer, read: {:?}",
            declaration.inputs()
        );
        let outputs: Vec<(SinkKind, TypeId)> = declaration
            .outputs()
            .iter()
            .map(|output| (output.kind, output.ty))
            .collect();
        assert!(
            outputs.contains(&(SinkKind::Staged, TypeId::of::<IndexedAssets>())),
            "{name}: the indexes, staged: {:?}",
            declaration.outputs()
        );
    }
    let found: Vec<&Mailbox> = khora_script::inventory::iter::<Mailbox>
        .into_iter()
        .filter(|mailbox| (mailbox.message)().ends_with("::IndexRequest"))
        .collect();
    assert_eq!(found.len(), 1, "one mailbox of `IndexRequest`: {found:?}");
    assert_eq!(
        (found[0].family, found[0].reader, found[0].between_sessions),
        (LOADING, Reader::Take, BetweenSessions::Hold)
    );
}

/// **A filed index is built off the frame, and lands in the reserve at a
/// frame start.** The family's agent under the real scheduler: an index
/// filed before the first frame is taken, built by the runtime's indexer on
/// another thread than the one running the frames, and its result reaches
/// the reserve within a bounded number of frames — the index channel left
/// empty.
#[test]
fn a_filed_index_is_built_off_the_frame_into_the_reserve() {
    let scenes = Channel::<SceneLoadRequest>::bounded(8, WhenFull::Reject);
    let indexes = Channel::<IndexRequest>::bounded(8, WhenFull::Reject);
    let indexing = Arc::new(WatchedIndexing::default());
    let reserve = Reserve::new();
    let mut runtime = Runtime::new();
    runtime.resources.insert(indexes.clone());
    runtime
        .resources
        .insert(Arc::clone(&indexing) as Arc<dyn AssetIndexing>);
    runtime.resources.insert(scenes);
    runtime
        .resources
        .insert(Channel::<CompileRequest>::bounded(8, WhenFull::DropOldest));
    runtime
        .resources
        .insert(Channel::<DecodeRequest>::bounded(8, WhenFull::DropOldest));
    runtime.resources.insert(reserve.clone());
    runtime
        .resources
        .insert(Arc::new(WatchedLoading::default()) as Arc<dyn SceneLoading>);
    runtime
        .resources
        .insert(Arc::new(EchoCompiler::default()) as Arc<dyn ScriptCompiler>);
    runtime
        .resources
        .insert(Arc::new(EchoDecoding::default()) as Arc<dyn AssetDecoding>);
    let runtime = Arc::new(runtime);
    let mut scheduler = scheduler_of(loading_registration());
    let mut world = World::new();
    indexes.send(IndexRequest {
        ticket: khora_core::asset::IndexTicket(3),
        project_root: PathBuf::from("game"),
        kind: khora_core::asset::IndexKind::Open,
    });

    let indexed: Vec<IndexedAssets> =
        frames_until_landed(&mut scheduler, &mut world, &runtime, &reserve);

    assert_eq!(indexed.len(), 1, "one result: {indexed:?}");
    assert_eq!(indexed[0].ticket, khora_core::asset::IndexTicket(3));
    assert_eq!(indexed[0].kind, khora_core::asset::IndexKind::Open);
    assert!(indexes.is_empty(), "the request was taken");
    assert!(
        indexing.threads.all_off(std::thread::current().id()),
        "indexed off the thread running the frames: {:?}",
        indexing.threads.all()
    );
}

// ─── Boot ───────────────────────────────────────────────────────────────────

/// **Boot accepts the `loading` family** on a runtime holding its three
/// request channels and its three loaders — the background's rules among
/// the checks — and builds one agent from it.
#[test]
fn boot_accepts_the_loading_family() {
    let wired = wired(None);
    let registration = loading_registration();

    assert_eq!(
        check_registrations(&[registration], &wired.runtime, &DeviceCaps::default()),
        Ok(())
    );
    let agents = lane_agents(&[registration], &wired.runtime, &DeviceCaps::default())
        .expect("the loading family boots");
    assert_eq!(agents.len(), 1);
    assert_eq!(agents[0].id(), LOADING);
}

// ─── Under the scheduler ────────────────────────────────────────────────────

/// **A filed asset is decoded off the frame, and lands in the reserve at a
/// frame start.** The family's agent under the real scheduler: a request
/// filed before the first frame is taken, decoded by the runtime's decoder on
/// another thread than the one running the frames, and its result reaches
/// the reserve within a bounded number of frames — the asset channel left
/// empty.
#[test]
fn a_filed_asset_is_decoded_off_the_frame_into_the_reserve() {
    let wired = wired(None);
    let mut scheduler = scheduler_of(loading_registration());
    let mut world = World::new();
    wired.assets.send(asset("meshes/hero.obj"));

    let decoded: Vec<Decoded> =
        frames_until_landed(&mut scheduler, &mut world, &wired.runtime, &wired.reserve);

    assert_eq!(decoded.len(), 1, "one result: {decoded:?}");
    assert_eq!(decoded[0].uuid, AssetUUID::new_v5("meshes/hero.obj"));
    assert_eq!(
        decoded[0]
            .value
            .as_ref()
            .ok()
            .and_then(|value| value.downcast_ref::<AssetUUID>())
            .copied(),
        Some(AssetUUID::new_v5("meshes/hero.obj"))
    );
    assert!(wired.assets.is_empty(), "the request was taken");
    assert!(
        wired.decoding.threads.all_off(std::thread::current().id()),
        "decoded off the thread running the frames: {:?}",
        wired.decoding.threads.all()
    );
}

/// **A filed script is compiled off the frame, and lands in the reserve at
/// a frame start**: its reload, with its declarations beside it, within a
/// bounded number of frames — the script channel left empty.
#[test]
fn a_filed_script_is_compiled_off_the_frame_into_the_reserve() {
    let wired = wired(None);
    let mut scheduler = scheduler_of(loading_registration());
    let mut world = World::new();
    wired.scripts.send(script("assets", &["ai/guard.erg"]));

    let reloads: Vec<ScriptReload> =
        frames_until_landed(&mut scheduler, &mut world, &wired.runtime, &wired.reserve);

    assert_eq!(
        reloads,
        vec![ScriptReload {
            module: "ai/guard.erg".to_owned(),
            program: Some(Program::default()),
        }]
    );
    assert_eq!(
        wired
            .reserve
            .take::<ScriptDeclarations>()
            .into_iter()
            .map(|set| set.module)
            .collect::<Vec<_>>(),
        vec!["ai/guard.erg".to_owned()],
        "the declarations landed beside the reload"
    );
    assert!(wired.scripts.is_empty(), "the request was taken");
    assert!(
        wired.compiler.threads.all_off(std::thread::current().id()),
        "compiled off the thread running the frames: {:?}",
        wired.compiler.threads.all()
    );
}

/// **A scene load is prepared off the frame thread**, and reaches the
/// reserve within a bounded number of frames — the scene channel left
/// empty.
#[test]
fn a_scene_load_is_prepared_off_the_frame_thread() {
    let wired = wired(None);
    let mut scheduler = scheduler_of(loading_registration());
    let mut world = World::new();
    wired.scenes.send(scene(1));

    let prepared: Vec<PreparedLoad> =
        frames_until_landed(&mut scheduler, &mut world, &wired.runtime, &wired.reserve);

    let tickets: Vec<LoadTicket> = prepared.iter().map(|load| load.ticket).collect();
    assert_eq!(tickets, vec![LoadTicket(1)]);
    assert!(wired.scenes.is_empty(), "the request was taken");
    assert!(
        wired.loading.threads.all_off(std::thread::current().id()),
        "prepared off the thread running the frames: {:?}",
        wired.loading.threads.all()
    );
}

/// **The `loading` family runs while the scripting family is off**: an
/// editor editing a scene runs no script, yet the scene it opens is
/// prepared, its assets decode, and a save is compiled and waits in the
/// reserve for the scripts to come back.
#[test]
fn loading_runs_while_the_scripting_family_is_off() {
    let activation = Arc::new(Activation::new([LOADING, khora_core::script::SCRIPT]));
    activation
        .deactivate(khora_core::script::SCRIPT)
        .expect("a known family");
    activation.settle();
    let wired = wired(Some(Arc::clone(&activation)));
    let mut scheduler = scheduler_of(loading_registration());
    let mut world = World::new();
    wired.scenes.send(scene(2));
    wired.scripts.send(script("assets", &["ai/guard.erg"]));
    wired.assets.send(asset("textures/albedo.png"));

    let deadline = Instant::now() + SETTLE;
    let (mut prepared, mut reloads, mut decoded) = (0, 0, 0);
    while prepared == 0 || reloads == 0 || decoded == 0 {
        scheduler.begin_frame(&wired.runtime);
        prepared += wired.reserve.take::<PreparedLoad>().len();
        reloads += wired.reserve.take::<ScriptReload>().len();
        decoded += wired.reserve.take::<Decoded>().len();
        scheduler.run_frame(&mut world, Arc::clone(&wired.runtime));
        assert!(
            Instant::now() < deadline,
            "timed out: {prepared} prepared, {reloads} reloads, {decoded} decoded"
        );
        std::thread::sleep(Duration::from_millis(5));
    }

    assert!(
        !activation.is_active(khora_core::script::SCRIPT),
        "the premise: the scripts stayed off"
    );
    assert_eq!(
        (prepared, reloads, decoded),
        (1, 1, 1),
        "each kind loaded while the scripts were off"
    );
    assert!(wired.all_taken(), "every request was taken");
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
    // The family's other kinds, idle: its lanes read every loader.
    install_idle_indexing(&mut runtime);
    runtime
        .resources
        .insert(Channel::<CompileRequest>::bounded(8, WhenFull::DropOldest));
    runtime
        .resources
        .insert(Channel::<DecodeRequest>::bounded(8, WhenFull::DropOldest));
    runtime
        .resources
        .insert(Arc::new(EchoCompiler::default()) as Arc<dyn ScriptCompiler>);
    runtime
        .resources
        .insert(Arc::new(EchoDecoding::default()) as Arc<dyn AssetDecoding>);
    let runtime = Arc::new(runtime);
    let hub = runtime
        .resources
        .get::<Arc<SceneLoadHub>>()
        .cloned()
        .expect("the runtime holds the scene load hub");
    let mut scheduler = scheduler_of(loading_registration());
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

// ─── How GORNA chooses ──────────────────────────────────────────────────────

/// An envelope of `time` on the host, no memory.
fn host_envelope(time: Duration) -> Envelope {
    Envelope {
        time: PerDevice {
            cpu: time,
            ..PerDevice::default()
        },
        memory: PerDevice::default(),
    }
}

/// The row `choose` picks within an envelope of `time` on the host, from
/// the two lanes' quotes — each lent that time, as the agent quotes them —
/// over one scene (20 ms), one root of two modules (4 ms) and two assets
/// (4 ms each) waiting: 32 ms of work. The menu is the rush lane's level,
/// then the paced lane's, rush the default.
fn chosen_within(time: Duration) -> Choice {
    let wired = wired(None);
    wired.scenes.send(scene(1));
    wired.scripts.send(script("game", &["a.erg", "b.erg"]));
    wired.assets.send(asset("a.obj"));
    wired.assets.send(asset("b.png"));
    let lanes: [Box<dyn AnyLane>; 2] = [
        Box::new(Erased::new(RushLane)),
        Box::new(Erased::new(PacedLane)),
    ];
    let bus = LaneBus::new();
    let mut quotes: Vec<Option<Cost>> = Vec::new();
    let mut menu = Vec::new();
    let mut lane_of_row = Vec::new();
    for (index, lane) in lanes.iter().enumerate() {
        let mut source =
            InputSource::new(FrameId(0), &bus, &wired.runtime).with_budget(Budget { time });
        let costs = lane.quote(&mut source).expect("the lane is quoted");
        for (level, cost) in lane.levels().iter().zip(costs) {
            menu.push(*level);
            quotes.push(Some(cost));
            lane_of_row.push(index);
        }
    }
    assert!(!wired.all_taken(), "the quotes took nothing");
    assert_eq!(menu.len(), 2, "one level each: {menu:?}");
    choose(
        &host_envelope(time),
        &quotes,
        &menu,
        &lane_of_row,
        0,
        None,
        None,
    )
}

/// **The agent runs rush when its envelope covers the work.** Lent 50 ms
/// for 32 ms of work, rush fits and serves the whole of it: it is chosen.
#[test]
fn the_agent_runs_rush_when_its_envelope_covers_the_work() {
    assert_eq!(
        chosen_within(Duration::from_millis(50)),
        Choice {
            row: 0,
            over_budget: false,
        },
        "rush"
    );
}

/// **The agent runs paced when its envelope does not cover the work.** Lent
/// 25 ms for 32 ms of work, rush does not fit; paced fits, serving a part of
/// it: paced is chosen, within the envelope. Lent 5 ms, nothing fits — paced
/// takes at least one 20 ms scene — and paced, the cheaper, is the floor.
#[test]
fn the_agent_runs_paced_when_its_envelope_does_not_cover_the_work() {
    assert_eq!(
        chosen_within(Duration::from_millis(25)),
        Choice {
            row: 1,
            over_budget: false,
        },
        "paced, within the envelope"
    );
    assert_eq!(
        chosen_within(Duration::from_millis(5)),
        Choice {
            row: 1,
            over_budget: true,
        },
        "paced, the floor"
    );
}

/// One frame of the `loading` family's agent as the scheduler runs a
/// background family: the frame start, the event pass waking the agent for
/// what its place gathered, the preparation it left run and handed back, then
/// one run of the installed layer, checked out and in. The row that ran.
fn background_frame(
    agent: &mut khora_agents::lane_agent::LaneAgent,
    frame: u64,
    runtime: &Arc<Runtime>,
) -> Option<StrategyId> {
    use khora_core::gpu::GpuSetup;
    use khora_core::lane::OutputDeck;
    use khora_core::EngineContext;

    fn lock(
        place: &Mutex<khora_core::agent::Place>,
    ) -> std::sync::MutexGuard<'_, khora_core::agent::Place> {
        // A test thread that panicked under the lock still reads the place.
        place
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }
    let place = agent.place();
    let mut bus = LaneBus::new();
    bus.set_frame(FrameId(frame));
    let mut deck = OutputDeck::new();
    lock(&place).start_frame(&mut GpuSetup::headless());
    let why = lock(&place).take_wakes();
    if !why.is_empty() {
        let permit = agent.contention();
        let mut context =
            EngineContext::for_agent(Arc::clone(runtime), &bus, &mut deck, &permit, Some(LOADING));
        let mut held = lock(&place);
        agent.wake(&why, &mut held, &mut context);
    }
    let preparation = lock(&place).take_preparation();
    if let Some(preparation) = preparation {
        let permit = preparation.contention();
        let mut scratch = OutputDeck::new();
        let prepared = {
            let mut context = EngineContext::for_agent(
                Arc::clone(runtime),
                &bus,
                &mut scratch,
                &permit,
                Some(LOADING),
            );
            preparation.run(&mut context)
        };
        lock(&place).prepared(prepared);
    }
    let mut out = lock(&place).check_out()?;
    let row = out.row();
    let permit = out.contention();
    let run = {
        let mut context =
            EngineContext::for_agent(Arc::clone(runtime), &bus, &mut deck, &permit, Some(LOADING));
        out.run(&mut context)
    };
    let _ = out.staged();
    let _ = lock(&place).check_in(out, run);
    Some(row)
}

/// **The agent moves to paced when the work filed outgrows its envelope.**
/// Lent 25 ms with nothing filed, the agent quotes both lanes on empty
/// mailboxes and runs rush. Then every frame three scene loads are filed —
/// 60 ms of work, more than twice the envelope. Each rush run loads them
/// all; its run must price what it loaded, so the agent finds rush no longer
/// fits and moves to paced, which fits — as `choose` does on fresh quotes.
/// Priced on what a run leaves in its fields, rush always costs nothing, and
/// the agent never leaves it.
#[test]
fn the_agent_moves_to_paced_when_the_work_filed_outgrows_its_envelope() {
    use khora_core::agent::gorna::protocol::Epoch;

    let wired = wired(None);
    let mut agent = lane_agent(loading_registration(), &DeviceCaps::default());
    agent.apply(
        Epoch(1),
        Some(&host_envelope(Duration::from_millis(25))),
        None,
        None,
    );
    // Nothing filed: the allocation is answered on empty mailboxes.
    let _ = background_frame(&mut agent, 1, &wired.runtime);

    let mut rows = Vec::new();
    for frame in 2..=8u64 {
        for i in 0..3 {
            wired.scenes.send(scene(frame * 10 + i));
        }
        rows.push(background_frame(&mut agent, frame, &wired.runtime));
    }

    assert!(
        rows.contains(&Some(StrategyId::Balanced)),
        "60 ms filed each frame against 25 ms lent: paced should run, but the rows were {rows:?}"
    );
}

/// **Under loads its envelope covers, the agent settles on rush.** Lent
/// 25 ms, one asset — 4 ms of work — is filed every other frame: rush fits
/// and serves the whole of it, so it runs, and keeps running. Each rush run
/// is priced on what it took, 4 ms; on the frame after, the paced lane is
/// quoted on the mailbox that run emptied, at nothing. Chosen on those two
/// prices — the same share, paced the cheaper — the agent moves to paced;
/// paced's next run costs 4 ms against a quote of nothing, drifts, and the
/// agent moves back: a switch of row on every load filed.
#[test]
fn the_agent_settles_on_rush_under_loads_its_envelope_covers() {
    use khora_core::agent::gorna::protocol::Epoch;

    let wired = wired(None);
    let mut agent = lane_agent(loading_registration(), &DeviceCaps::default());
    agent.apply(
        Epoch(1),
        Some(&host_envelope(Duration::from_millis(25))),
        None,
        None,
    );
    // Nothing filed: the allocation is answered on empty mailboxes.
    let _ = background_frame(&mut agent, 1, &wired.runtime);

    let mut rows = Vec::new();
    for frame in 2..=16u64 {
        if frame % 2 == 0 {
            wired.assets.send(asset(&format!("textures/{frame}.png")));
        }
        rows.push(background_frame(&mut agent, frame, &wired.runtime));
    }

    // A few frames to settle, then one row.
    assert!(
        rows[4..]
            .iter()
            .all(|row| *row == Some(StrategyId::HighPerformance)),
        "4 ms filed every other frame against 25 ms lent: rush should run and stay, but the rows were {rows:?}"
    );
}
