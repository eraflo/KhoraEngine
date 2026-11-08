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

//! **The `loading` family's lanes hand the requests they take to the
//! runtime's loaders, and stage each result under the lease it holds to.**
//! Project indexes go to the `AssetIndexing`, scenes to the `SceneLoading`,
//! script roots to the `ScriptCompiler`, assets to the `AssetDecoding`. The
//! rush lane loads everything filed; the paced lane only the units its budget
//! covers — the indexes first, since the scenes and the assets read through
//! the index, then the scenes, then the script roots, then the assets, each
//! kind in filing order, always at least one — and what it leaves goes back
//! to its channel.
//!
//! Each lane is run by hand as the scheduler runs a background lane, through
//! its erased form: its inputs fetched from a runtime holding the four
//! request channels and four fake loaders, recorded, its outputs committed
//! into a staging a reserve then receives, and what it left in its fields
//! given back. It reads no `World`, no bus, and locks nothing.

use std::any::{Any, TypeId};
use std::collections::{BTreeSet, HashMap};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use khora_core::asset::{
    AssetDecoding, AssetIndexing, AssetUUID, DecodeRequest, Decoded, IndexKind, IndexRequest,
    IndexTicket, IndexedAssets,
};
use khora_core::event::{Channel, Cursor, WhenFull};
use khora_core::gpu::GpuSetup;
use khora_core::lane::cost::Cost;
use khora_core::lane::erased::{AnyLane, Erased, RunOutcome};
use khora_core::lane::io::{Budget, FrameId, InputSource, OutputSink};
use khora_core::lane::{LaneBus, Lease, OutputDeck, Reserve, Staging};
use khora_core::Runtime;
use khora_data::ecs::World;
use khora_data::scene::loading::{
    LoadKind, LoadTicket, PreparedLoad, Refile, SceneLoadRequest, SceneLoading, SceneSource,
};
use khora_data::scene::NoPrefabs;
use khora_lanes::loading_lane::{PacedLane, RushLane};
use khora_script::reload::ScriptReload;
use khora_script::toolchain::{CompileRequest, Compiled, ScriptCompiler, ScriptDeclarations};
use khora_script::vm::Program;

/// What a scene load is priced at before any is measured.
const SCENE: Duration = Duration::from_millis(20);
/// What compiling one module of a root is priced at.
const MODULE: Duration = Duration::from_millis(2);
/// What decoding one asset is priced at.
const ASSET: Duration = Duration::from_millis(4);
/// What indexing one project is priced at.
const INDEX: Duration = Duration::from_millis(50);

// ─── Fake loaders ───────────────────────────────────────────────────────────

/// A decoder that knows a few assets, each decoding to a number, and fails
/// any other. Each result is leased on a flag of its asset the test may
/// lower, standing for the asset's generation moving on. It remembers the
/// requests, in the order it was asked.
#[derive(Default)]
struct FakeDecoding {
    known: HashMap<AssetUUID, u32>,
    current: HashMap<AssetUUID, Arc<AtomicBool>>,
    asked: Mutex<Vec<DecodeRequest>>,
}

impl FakeDecoding {
    fn knowing(assets: &[(AssetUUID, u32)]) -> Arc<Self> {
        Arc::new(Self {
            known: assets.iter().copied().collect(),
            current: assets
                .iter()
                .map(|(uuid, _)| (*uuid, Arc::new(AtomicBool::new(true))))
                .collect(),
            asked: Mutex::new(Vec::new()),
        })
    }

    fn asked(&self) -> Vec<DecodeRequest> {
        self.asked
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone()
    }

    /// The asset `uuid` changed since it was asked for: what was decoded for
    /// it no longer counts.
    fn change(&self, uuid: AssetUUID) {
        self.current[&uuid].store(false, Ordering::SeqCst);
    }
}

impl AssetDecoding for FakeDecoding {
    fn decode(&self, uuid: &AssetUUID, type_id: TypeId) -> Decoded {
        Decoded {
            uuid: *uuid,
            type_id,
            value: self
                .known
                .get(uuid)
                .map(|value| Box::new(*value) as Box<dyn Any + Send>)
                .ok_or_else(|| format!("asset {uuid} is not known")),
            external_files: Vec::new(),
        }
    }

    fn decode_leased(&self, request: &DecodeRequest) -> (Decoded, Option<Lease>) {
        self.asked
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .push(request.clone());
        let lease = self.current.get(&request.uuid).map(|current| {
            let current = Arc::clone(current);
            Lease::new(move || current.load(Ordering::SeqCst))
        });
        (self.decode(&request.uuid, request.type_id), lease)
    }
}

/// A compiler that answers every request with `answer` — or, without one,
/// with a program and an empty declaration set for each module it names —
/// and remembers the requests, in the order it was asked.
#[derive(Default)]
struct FakeCompiler {
    answer: Option<Compiled>,
    asked: Mutex<Vec<CompileRequest>>,
}

impl FakeCompiler {
    fn echoing() -> Arc<Self> {
        Arc::new(Self::default())
    }

    fn answering(answer: Compiled) -> Arc<Self> {
        Arc::new(Self {
            answer: Some(answer),
            asked: Mutex::new(Vec::new()),
        })
    }

    fn asked(&self) -> Vec<CompileRequest> {
        self.asked
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone()
    }
}

impl ScriptCompiler for FakeCompiler {
    fn compile(&self, request: &CompileRequest) -> Compiled {
        self.asked
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .push(request.clone());
        match &self.answer {
            Some(answer) => answer.clone(),
            None => Compiled {
                reloads: request.modules.iter().map(|m| reload(m)).collect(),
                declarations: request.modules.iter().map(|m| declarations(m)).collect(),
            },
        }
    }
}

/// An indexer that answers every request with the bytes of its project's
/// root, leased on a flag of that root the test may lower — standing for
/// another project opened since. It remembers the tickets it was asked, in
/// order.
#[derive(Default)]
struct FakeIndexing {
    open: Mutex<HashMap<PathBuf, Arc<AtomicBool>>>,
    asked: Mutex<Vec<IndexTicket>>,
}

impl FakeIndexing {
    fn new() -> Arc<Self> {
        Arc::new(Self::default())
    }

    fn asked(&self) -> Vec<IndexTicket> {
        self.asked
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone()
    }

    fn flag(&self, root: &std::path::Path) -> Arc<AtomicBool> {
        let mut open = self
            .open
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        Arc::clone(
            open.entry(root.to_path_buf())
                .or_insert_with(|| Arc::new(AtomicBool::new(true))),
        )
    }

    /// Another project replaced the one at `root` since its index was asked
    /// for: what was indexed for it no longer counts.
    fn replace(&self, root: &str) {
        self.flag(std::path::Path::new(root))
            .store(false, Ordering::SeqCst);
    }
}

impl AssetIndexing for FakeIndexing {
    fn index_leased(&self, request: &IndexRequest) -> (IndexedAssets, Option<Lease>) {
        self.asked
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .push(request.ticket);
        let open = self.flag(&request.project_root);
        let indexed = IndexedAssets {
            ticket: request.ticket,
            kind: request.kind,
            project_root: request.project_root.clone(),
            index: Ok(request.project_root.to_string_lossy().as_bytes().to_vec()),
        };
        (
            indexed,
            Some(Lease::new(move || open.load(Ordering::SeqCst))),
        )
    }
}

/// A `SceneLoading` that prepares nothing: each request comes back as a
/// failed load naming its ticket, leased on a flag the test may lower —
/// standing for the world's components changing — and set to be filed again
/// on `requests`. It remembers the tickets it was asked, in order, and
/// reserves one id through each request's hold, if it carries one.
struct FakeLoading {
    requests: Channel<SceneLoadRequest>,
    current: Arc<AtomicBool>,
    asked: Mutex<Vec<LoadTicket>>,
    reserved: Mutex<Vec<khora_core::ecs::entity::EntityId>>,
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

    fn reserved(&self) -> Vec<khora_core::ecs::entity::EntityId> {
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

// ─── Requests ───────────────────────────────────────────────────────────────

fn scene(ticket: u64) -> SceneLoadRequest {
    SceneLoadRequest {
        ticket: LoadTicket(ticket),
        kind: LoadKind::Replace(SceneSource::Bytes(Arc::from(&b"scene"[..]))),
        prefabs: Arc::new(NoPrefabs),
        hold: None,
    }
}

fn script(root: &str, modules: &[&str]) -> CompileRequest {
    CompileRequest {
        root: PathBuf::from(root),
        modules: modules.iter().map(|module| (*module).to_owned()).collect(),
    }
}

fn index(ticket: u64, root: &str) -> IndexRequest {
    IndexRequest {
        ticket: IndexTicket(ticket),
        project_root: PathBuf::from(root),
        kind: IndexKind::Reindex,
    }
}

fn uuid(path: &str) -> AssetUUID {
    AssetUUID::new_v5(path)
}

fn asset(path: &str) -> DecodeRequest {
    DecodeRequest {
        uuid: uuid(path),
        type_id: TypeId::of::<u32>(),
        generation: 0,
    }
}

fn reload(module: &str) -> ScriptReload {
    ScriptReload {
        module: module.to_owned(),
        program: Some(Program::default()),
    }
}

fn declarations(module: &str) -> ScriptDeclarations {
    ScriptDeclarations {
        module: module.to_owned(),
        components: Vec::new(),
    }
}

// ─── The runtime ────────────────────────────────────────────────────────────

/// A runtime as the engine wires the `loading` family: the four request
/// channels and the four loaders — and handles on each, for the test.
struct Filed {
    runtime: Runtime,
    indexes: Channel<IndexRequest>,
    scenes: Channel<SceneLoadRequest>,
    scripts: Channel<CompileRequest>,
    assets: Channel<DecodeRequest>,
    loading: Arc<FakeLoading>,
    compiler: Arc<FakeCompiler>,
    decoding: Arc<FakeDecoding>,
    indexing: Arc<FakeIndexing>,
}

/// Installs in `runtime` an index request channel and `indexing`: the
/// family's lanes read them whatever kind of load a test files.
fn install_indexing(runtime: &mut Runtime, indexing: &Arc<FakeIndexing>) -> Channel<IndexRequest> {
    let indexes = Channel::<IndexRequest>::bounded(64, WhenFull::Reject);
    runtime.resources.insert(indexes.clone());
    runtime
        .resources
        .insert(Arc::clone(indexing) as Arc<dyn AssetIndexing>);
    indexes
}

impl Filed {
    /// Nothing filed yet; the decoder knows `known`, the compiler is
    /// `compiler`.
    fn new(known: &[(AssetUUID, u32)], compiler: Arc<FakeCompiler>) -> Self {
        let scenes = Channel::<SceneLoadRequest>::bounded(64, WhenFull::Reject);
        let scripts = Channel::<CompileRequest>::bounded(64, WhenFull::Reject);
        let assets = Channel::<DecodeRequest>::bounded(64, WhenFull::Reject);
        let loading = FakeLoading::new(&scenes);
        let decoding = FakeDecoding::knowing(known);
        let indexing = FakeIndexing::new();
        let mut runtime = Runtime::new();
        let indexes = install_indexing(&mut runtime, &indexing);
        runtime.resources.insert(scenes.clone());
        runtime.resources.insert(scripts.clone());
        runtime.resources.insert(assets.clone());
        runtime
            .resources
            .insert(Arc::clone(&loading) as Arc<dyn SceneLoading>);
        runtime
            .resources
            .insert(Arc::clone(&compiler) as Arc<dyn ScriptCompiler>);
        runtime
            .resources
            .insert(Arc::clone(&decoding) as Arc<dyn AssetDecoding>);
        Self {
            runtime,
            indexes,
            scenes,
            scripts,
            assets,
            loading,
            compiler,
            decoding,
            indexing,
        }
    }

    /// An echoing compiler, and a decoder knowing every asset of `paths`
    /// (each decoding to its position in the list).
    fn knowing(paths: &[&str]) -> Self {
        let known: Vec<(AssetUUID, u32)> = paths
            .iter()
            .enumerate()
            .map(|(i, path)| (uuid(path), i as u32))
            .collect();
        Self::new(&known, FakeCompiler::echoing())
    }

    fn file_indexes(&self, requests: &[IndexRequest]) -> &Self {
        for request in requests {
            self.indexes.send(request.clone());
        }
        self
    }

    fn file_scenes(&self, tickets: &[u64]) -> &Self {
        for ticket in tickets {
            self.scenes.send(scene(*ticket));
        }
        self
    }

    fn file_scripts(&self, requests: &[CompileRequest]) -> &Self {
        for request in requests {
            self.scripts.send(request.clone());
        }
        self
    }

    fn file_assets(&self, paths: &[&str]) -> &Self {
        for path in paths {
            self.assets.send(asset(path));
        }
        self
    }

    /// The tickets of the scene loads still waiting, in order.
    fn waiting_scenes(&self) -> Vec<LoadTicket> {
        self.scenes
            .read(&mut Cursor::default())
            .iter()
            .map(|request| request.ticket)
            .collect()
    }

    fn waiting_indexes(&self) -> Vec<IndexTicket> {
        self.indexes
            .read(&mut Cursor::default())
            .iter()
            .map(|request| request.ticket)
            .collect()
    }

    fn waiting_scripts(&self) -> Vec<CompileRequest> {
        self.scripts.read(&mut Cursor::default())
    }

    fn waiting_assets(&self) -> Vec<DecodeRequest> {
        self.assets.read(&mut Cursor::default())
    }

    fn all_taken(&self) -> bool {
        self.indexes.is_empty()
            && self.scenes.is_empty()
            && self.scripts.is_empty()
            && self.assets.is_empty()
    }
}

/// One run of `lane` over `filed`, as the scheduler runs it at the
/// background, lent `budget` when given: its inputs fetched, the lane
/// recorded, its outputs committed into the staging returned — not yet
/// received by a reserve — and what it left given back.
fn run(lane: &mut dyn AnyLane, filed: &Filed, budget: Option<Duration>) -> Staging {
    let bus = LaneBus::new();
    let mut source = InputSource::new(FrameId(0), &bus, &filed.runtime);
    if let Some(time) = budget {
        source = source.with_budget(Budget { time });
    }
    let mut deck = OutputDeck::new();
    let mut staging = Staging::new();
    {
        let mut sink = OutputSink::new(&mut deck).with_staging(&mut staging);
        let ran = lane.run(0, &mut source, &mut sink, &mut GpuSetup::headless());
        assert!(
            matches!(ran.outcome, RunOutcome::Recorded(_)),
            "the run records: {:?}",
            ran.outcome
        );
    }
    staging
}

/// The cost of `lane`'s one level on what `filed` holds, lent `budget`
/// when given.
fn quote(lane: &dyn AnyLane, filed: &Filed, budget: Option<Duration>) -> Cost {
    let bus = LaneBus::new();
    let mut source = InputSource::new(FrameId(0), &bus, &filed.runtime);
    if let Some(time) = budget {
        source = source.with_budget(Budget { time });
    }
    let costs = lane.quote(&mut source).expect("the lane is quoted");
    assert_eq!(costs.len(), 1, "one level: {costs:?}");
    costs[0]
}

/// What `stagings` land in a fresh reserve, as frame starts receive them.
fn landed(stagings: impl IntoIterator<Item = Staging>) -> Reserve {
    let reserve = Reserve::new();
    for staging in stagings {
        reserve.receive(staging);
    }
    reserve
}

fn rush() -> Erased<RushLane> {
    Erased::new(RushLane)
}

fn paced() -> Erased<PacedLane> {
    Erased::new(PacedLane)
}

fn tickets(reserve: &Reserve) -> Vec<LoadTicket> {
    reserve
        .take::<PreparedLoad>()
        .iter()
        .map(|load| load.ticket)
        .collect()
}

fn decoded_uuids(reserve: &Reserve) -> Vec<AssetUUID> {
    reserve
        .take::<Decoded>()
        .iter()
        .map(|decoded| decoded.uuid)
        .collect()
}

fn reloaded_modules(reserve: &Reserve) -> BTreeSet<String> {
    reserve
        .take::<ScriptReload>()
        .into_iter()
        .map(|reload| reload.module)
        .collect()
}

fn value_of(decoded: &Decoded) -> Option<u32> {
    decoded
        .value
        .as_ref()
        .ok()
        .and_then(|value| value.downcast_ref::<u32>())
        .copied()
}

fn millis(cost: &Cost) -> Duration {
    cost.time.cpu
}

// ─── Rush: what each kind does, moved from the three lanes ─────────────────

/// **Rush decodes every asset it takes and stages one result each.** Two
/// assets wait: the decoder is asked for both, in the order they were filed,
/// each result reaches the reserve with its value and the type it was asked
/// for, and the asset channel is left empty.
#[test]
fn rush_decodes_every_asset_it_takes_and_stages_one_result_each() {
    let (mesh, texture) = (uuid("meshes/a.obj"), uuid("t.png"));
    let filed = Filed::new(&[(mesh, 7), (texture, 9)], FakeCompiler::echoing());
    filed.file_assets(&["meshes/a.obj", "t.png"]);

    let reserve = landed([run(&mut rush(), &filed, None)]);

    assert_eq!(
        filed.decoding.asked(),
        vec![asset("meshes/a.obj"), asset("t.png")]
    );
    let decoded = reserve.take::<Decoded>();
    let got: Vec<(AssetUUID, Option<u32>)> = decoded
        .iter()
        .map(|decoded| (decoded.uuid, value_of(decoded)))
        .collect();
    assert_eq!(got, vec![(mesh, Some(7)), (texture, Some(9))]);
    assert!(
        decoded.iter().all(|d| d.type_id == TypeId::of::<u32>()),
        "each result carries the type it was asked for"
    );
    assert!(filed.assets.is_empty(), "the requests were taken");
}

/// **A failed decode is staged, never dropped nor panicked on.** A request
/// names an asset the decoder does not know: its failure reaches the
/// reserve, so the intake can record it — beside the request that decoded.
#[test]
fn rush_stages_a_failed_decode_beside_the_ones_that_decoded() {
    let known = uuid("meshes/a.obj");
    let unknown = uuid("meshes/never_indexed.obj");
    let filed = Filed::new(&[(known, 1)], FakeCompiler::echoing());
    filed.file_assets(&["meshes/never_indexed.obj", "meshes/a.obj"]);

    let reserve = landed([run(&mut rush(), &filed, None)]);

    let decoded = reserve.take::<Decoded>();
    assert_eq!(decoded.len(), 2, "one result per request: {decoded:?}");
    let failed = decoded
        .iter()
        .find(|decoded| decoded.uuid == unknown)
        .expect("the failure is staged");
    assert!(failed.value.is_err(), "it is staged as a failure");
    assert!(decoded
        .iter()
        .any(|decoded| decoded.uuid == known && value_of(decoded) == Some(1)));
}

/// **Each decode is staged under its own lease.** Two assets are decoded in
/// one run; one changes before the frame start receives the run: its result
/// is abandoned there, and only its — the other lands.
#[test]
fn a_decode_whose_asset_changed_while_it_waited_is_abandoned_and_only_it() {
    let (changed, kept) = (uuid("a.obj"), uuid("b.obj"));
    let filed = Filed::new(&[(changed, 1), (kept, 2)], FakeCompiler::echoing());
    filed.file_assets(&["a.obj", "b.obj"]);

    let staging = run(&mut rush(), &filed, None);
    filed.decoding.change(changed);
    let reserve = landed([staging]);

    assert_eq!(
        decoded_uuids(&reserve),
        vec![kept],
        "the changed asset's decode is never installed"
    );
}

/// **Rush stages the reloads and the declarations the compiler returns.**
/// One script request waits; the fake compiler answers with a reload and a
/// set of declarations: both reach the reserve, and the script channel is
/// left empty.
#[test]
fn rush_stages_what_the_compiler_returns() {
    let filed = Filed::new(
        &[],
        FakeCompiler::answering(Compiled {
            reloads: vec![reload("ai/guard.erg")],
            declarations: vec![declarations("ai/guard.erg")],
        }),
    );
    filed.file_scripts(&[script("assets", &["ai/guard.erg"])]);

    let reserve = landed([run(&mut rush(), &filed, None)]);

    assert_eq!(
        filed.compiler.asked(),
        vec![script("assets", &["ai/guard.erg"])]
    );
    assert_eq!(
        reserve.take::<ScriptReload>(),
        vec![reload("ai/guard.erg")],
        "the reload is staged"
    );
    assert_eq!(
        reserve.take::<ScriptDeclarations>(),
        vec![declarations("ai/guard.erg")],
        "the declarations are staged"
    );
    assert!(filed.scripts.is_empty(), "the request was taken");
}

/// **The requests of one root are compiled once, their modules merged.**
/// Two requests of `game` wait beside one of `editor`: the compiler is asked
/// once per root, `game` with both its modules, in the order the roots first
/// asked.
#[test]
fn rush_compiles_the_requests_of_one_root_once_with_their_modules_merged() {
    let filed = Filed::knowing(&[]);
    filed.file_scripts(&[
        script("game", &["a.erg"]),
        script("editor", &["x.erg"]),
        script("game", &["b.erg"]),
    ]);

    let _ = run(&mut rush(), &filed, None);

    assert_eq!(
        filed.compiler.asked(),
        vec![
            script("game", &["a.erg", "b.erg"]),
            script("editor", &["x.erg"]),
        ]
    );
}

/// **What every compile of a run returns is staged.** Each root's answer
/// reaches the reserve: none is lost to the next compile of the run.
#[test]
fn what_every_compile_of_a_run_returns_is_staged() {
    let filed = Filed::new(
        &[],
        FakeCompiler::answering(Compiled {
            reloads: vec![reload("shared.erg")],
            declarations: Vec::new(),
        }),
    );
    filed.file_scripts(&[script("game", &["a.erg"]), script("editor", &["x.erg"])]);

    let reserve = landed([run(&mut rush(), &filed, None)]);

    assert_eq!(filed.compiler.asked().len(), 2, "one compile per root");
    // The reserve keeps one reload per module: the second compile's reload
    // of `shared.erg` replaced the first's, which never reached a boundary.
    assert_eq!(reserve.take::<ScriptReload>(), vec![reload("shared.erg")]);
}

/// **Rush prepares every scene load it takes and stages one load each.** Two
/// loads wait: the runtime's `SceneLoading` is asked for both, in the order
/// they were filed, each prepared load reaches the reserve under its own
/// ticket and kind, and the scene channel is left empty.
#[test]
fn rush_prepares_every_scene_load_it_takes_and_stages_one_each() {
    let filed = Filed::knowing(&[]);
    let mut add = scene(2);
    add.kind = LoadKind::Add(SceneSource::Bytes(Arc::from(&b"more"[..])));
    filed.scenes.send(scene(1));
    filed.scenes.send(add);

    let reserve = landed([run(&mut rush(), &filed, None)]);

    assert_eq!(filed.loading.asked(), vec![LoadTicket(1), LoadTicket(2)]);
    let got: Vec<(LoadTicket, bool)> = reserve
        .take::<PreparedLoad>()
        .iter()
        .map(|load| (load.ticket, load.replaces))
        .collect();
    assert_eq!(got, vec![(LoadTicket(1), true), (LoadTicket(2), false)]);
    assert!(filed.scenes.is_empty(), "the requests were taken");
}

/// **A load whose lease broke while it waited is filed again, without its
/// hold.** A load prepared with a hold is staged; the world's components
/// change before the frame start receives it: it never reaches the reserve,
/// its request is back on the channel under the same ticket and with no
/// hold, and the id its hold reserved is handed out again.
#[test]
fn a_load_whose_lease_broke_is_filed_again_without_its_hold() {
    let world = World::new();
    let filed = Filed::knowing(&[]);
    let mut held = scene(7);
    held.hold = Some(world.hold_reservations());
    filed.scenes.send(held);

    let staging = run(&mut rush(), &filed, None);
    filed.loading.change();
    let reserve = landed([staging]);

    let prepared = reserve.take::<PreparedLoad>();
    assert!(
        prepared.is_empty(),
        "an abandoned load never lands: {prepared:?}"
    );
    let refiled = filed.scenes.drain();
    assert_eq!(refiled.len(), 1, "filed again once: {refiled:?}");
    assert_eq!(refiled[0].ticket, LoadTicket(7));
    assert!(refiled[0].hold.is_none(), "filed again without its hold");
    let reserved = filed.loading.reserved();
    assert_eq!(reserved.len(), 1, "the fake reserved one id");
    assert_eq!(
        world.reserver().reserve(),
        reserved[0],
        "the abandoned load's hold gave its id back"
    );
}

/// **A run with nothing filed loads nothing and stages nothing**, on either
/// lane: no loader is asked.
#[test]
fn a_run_with_nothing_filed_loads_nothing() {
    for (name, lane) in [
        ("rush", &mut rush() as &mut dyn AnyLane),
        ("paced", &mut paced() as &mut dyn AnyLane),
    ] {
        let filed = Filed::knowing(&["a.obj"]);

        let reserve = landed([run(lane, &filed, Some(Duration::from_millis(25)))]);

        assert!(filed.loading.asked().is_empty(), "{name}: no scene asked");
        assert!(filed.compiler.asked().is_empty(), "{name}: no compile");
        assert!(filed.decoding.asked().is_empty(), "{name}: no decode");
        assert!(filed.indexing.asked().is_empty(), "{name}: no index");
        assert!(reserve.is_empty::<IndexedAssets>(), "{name}");
        assert!(reserve.is_empty::<PreparedLoad>(), "{name}");
        assert!(reserve.is_empty::<ScriptReload>(), "{name}");
        assert!(reserve.is_empty::<ScriptDeclarations>(), "{name}");
        assert!(reserve.is_empty::<Decoded>(), "{name}");
    }
}

// ─── Rush ───────────────────────────────────────────────────────────────────

/// **Rush loads everything filed.** Two scenes, three script requests of two
/// roots and two assets wait: each scene is prepared, each root compiled
/// once with its requests merged, each asset decoded — every result staged,
/// and every channel left empty.
#[test]
fn rush_loads_everything_filed() {
    let filed = Filed::knowing(&["a.obj", "b.png"]);
    filed
        .file_scenes(&[1, 2])
        .file_scripts(&[
            script("game", &["a.erg"]),
            script("editor", &["x.erg"]),
            script("game", &["b.erg"]),
        ])
        .file_assets(&["a.obj", "b.png"]);

    let reserve = landed([run(&mut rush(), &filed, Some(Duration::from_millis(1)))]);

    assert_eq!(filed.loading.asked(), vec![LoadTicket(1), LoadTicket(2)]);
    assert_eq!(
        filed.compiler.asked(),
        vec![
            script("game", &["a.erg", "b.erg"]),
            script("editor", &["x.erg"]),
        ],
        "one compile per root, its requests merged"
    );
    assert_eq!(filed.decoding.asked(), vec![asset("a.obj"), asset("b.png")]);
    assert_eq!(tickets(&reserve), vec![LoadTicket(1), LoadTicket(2)]);
    assert_eq!(
        reloaded_modules(&reserve),
        ["a.erg", "b.erg", "x.erg"]
            .into_iter()
            .map(str::to_owned)
            .collect()
    );
    assert_eq!(
        reserve
            .take::<ScriptDeclarations>()
            .into_iter()
            .map(|set| set.module)
            .collect::<BTreeSet<_>>(),
        ["a.erg", "b.erg", "x.erg"]
            .into_iter()
            .map(str::to_owned)
            .collect()
    );
    assert_eq!(decoded_uuids(&reserve), vec![uuid("a.obj"), uuid("b.png")]);
    assert!(filed.all_taken(), "everything filed was taken");
}

/// **Rush quotes all the waiting work, whatever its budget**: 20 ms a scene,
/// 2 ms a module of a root, 4 ms an asset — two scenes, three modules and
/// three assets are 58 ms — and serves all of it (no share). The quote takes
/// nothing.
#[test]
fn rush_quotes_all_the_waiting_work() {
    let filed = Filed::knowing(&[]);
    filed
        .file_scenes(&[1, 2])
        .file_scripts(&[
            script("game", &["a.erg"]),
            script("editor", &["x.erg"]),
            script("game", &["b.erg"]),
        ])
        .file_assets(&["a.obj", "b.png", "c.wav"]);

    let cost = quote(&rush(), &filed, Some(Duration::from_millis(10)));

    assert_eq!(millis(&cost), SCENE * 2 + MODULE * 3 + ASSET * 3);
    assert_eq!(cost.share, None, "rush serves all the waiting work");
    assert_eq!(filed.waiting_scenes().len(), 2, "the quote took nothing");
    assert_eq!(filed.waiting_scripts().len(), 3, "the quote took nothing");
    assert_eq!(filed.waiting_assets().len(), 3, "the quote took nothing");
}

// ─── Paced ──────────────────────────────────────────────────────────────────

/// **Paced takes the scenes, then the scripts, then the assets, within its
/// budget.** Lent 25 ms, with one scene (20 ms), one root of two modules
/// (4 ms) and two assets (4 ms each) waiting: the scene and the root are
/// loaded — 24 ms — and the two assets go back to their channel, in the
/// order they were filed.
#[test]
fn paced_takes_scenes_then_scripts_then_assets_within_its_budget() {
    let filed = Filed::knowing(&["a.obj", "b.png"]);
    filed
        .file_assets(&["a.obj", "b.png"])
        .file_scripts(&[script("game", &["a.erg", "b.erg"])])
        .file_scenes(&[1]);

    let reserve = landed([run(&mut paced(), &filed, Some(Duration::from_millis(25)))]);

    assert_eq!(filed.loading.asked(), vec![LoadTicket(1)], "the scene");
    assert_eq!(
        filed.compiler.asked(),
        vec![script("game", &["a.erg", "b.erg"])],
        "the root"
    );
    assert!(filed.decoding.asked().is_empty(), "no asset decoded");
    assert_eq!(tickets(&reserve), vec![LoadTicket(1)]);
    assert_eq!(reloaded_modules(&reserve).len(), 2);
    assert!(reserve.is_empty::<Decoded>());
    assert!(filed.scenes.is_empty() && filed.scripts.is_empty());
    assert_eq!(
        filed.waiting_assets(),
        vec![asset("a.obj"), asset("b.png")],
        "the two assets went back, in order"
    );
}

/// **Paced takes one unit whatever its budget.** Lent 1 ms — less than any
/// unit costs — it still loads the first scene filed; the second scene and
/// the asset go back.
#[test]
fn paced_takes_one_unit_whatever_its_budget() {
    let filed = Filed::knowing(&["a.obj"]);
    filed.file_scenes(&[1, 2]).file_assets(&["a.obj"]);

    let reserve = landed([run(&mut paced(), &filed, Some(Duration::from_millis(1)))]);

    assert_eq!(filed.loading.asked(), vec![LoadTicket(1)]);
    assert_eq!(tickets(&reserve), vec![LoadTicket(1)]);
    assert_eq!(filed.waiting_scenes(), vec![LoadTicket(2)]);
    assert_eq!(filed.waiting_assets(), vec![asset("a.obj")]);
    assert!(filed.decoding.asked().is_empty());
}

/// **Paced without a budget takes everything**, as rush does.
#[test]
fn paced_without_a_budget_takes_everything() {
    let filed = Filed::knowing(&["a.obj", "b.png"]);
    filed
        .file_scenes(&[1, 2])
        .file_scripts(&[script("game", &["a.erg"]), script("editor", &["x.erg"])])
        .file_assets(&["a.obj", "b.png"]);

    let reserve = landed([run(&mut paced(), &filed, None)]);

    assert_eq!(filed.loading.asked(), vec![LoadTicket(1), LoadTicket(2)]);
    assert_eq!(filed.compiler.asked().len(), 2);
    assert_eq!(filed.decoding.asked().len(), 2);
    assert_eq!(tickets(&reserve), vec![LoadTicket(1), LoadTicket(2)]);
    assert_eq!(decoded_uuids(&reserve), vec![uuid("a.obj"), uuid("b.png")]);
    assert!(filed.all_taken(), "everything filed was taken");
}

/// **Paced never splits a root.** The two requests of `game` are one unit:
/// alone and over its budget, they are loaded together in one compile; after
/// a scene that leaves too little of the budget, they are both left, and go
/// back in the order filed.
#[test]
fn paced_never_splits_a_root() {
    // Loaded together: the root is the first unit, taken whatever it costs.
    let filed = Filed::knowing(&[]);
    filed.file_scripts(&[script("game", &["a.erg"]), script("game", &["b.erg"])]);

    let _ = run(&mut paced(), &filed, Some(Duration::from_millis(3)));

    assert_eq!(
        filed.compiler.asked(),
        vec![script("game", &["a.erg", "b.erg"])],
        "both requests in one compile"
    );
    assert!(filed.scripts.is_empty(), "neither request left");

    // Both left: the scene takes 20 ms of 21, the root would need 4 more.
    let filed = Filed::knowing(&[]);
    filed
        .file_scenes(&[1])
        .file_scripts(&[script("game", &["a.erg"]), script("game", &["b.erg"])]);

    let _ = run(&mut paced(), &filed, Some(Duration::from_millis(21)));

    assert_eq!(filed.loading.asked(), vec![LoadTicket(1)]);
    assert!(
        filed.compiler.asked().is_empty(),
        "the root was not compiled"
    );
    assert_eq!(
        filed.waiting_scripts(),
        vec![script("game", &["a.erg"]), script("game", &["b.erg"])],
        "both requests went back, in order"
    );
}

/// **Paced quotes the share it serves.** One scene, one root of two modules
/// and two assets wait — 32 ms of work. Lent 25 ms, it would take the scene
/// and the root: it quotes 24 ms, serving 24/32 of the work. Lent 1 ms, it
/// would still take the scene: 20 ms, 20/32. Lent 100 ms — or nothing — it
/// would take everything: 32 ms, no share. The quote takes nothing.
#[test]
fn paced_quotes_the_share_it_serves() {
    let filed = Filed::knowing(&[]);
    filed
        .file_scenes(&[1])
        .file_scripts(&[script("game", &["a.erg", "b.erg"])])
        .file_assets(&["a.obj", "b.png"]);
    let total = SCENE + MODULE * 2 + ASSET * 2;
    let close = |share: Option<f32>, expected: f32| {
        share.is_some_and(|share| (share - expected).abs() < 1e-4)
    };

    let within = quote(&paced(), &filed, Some(Duration::from_millis(25)));
    assert_eq!(millis(&within), SCENE + MODULE * 2);
    assert!(
        close(within.share, 24.0 / 32.0),
        "the share it serves: {:?}",
        within.share
    );

    let one = quote(&paced(), &filed, Some(Duration::from_millis(1)));
    assert_eq!(millis(&one), SCENE);
    assert!(
        close(one.share, 20.0 / 32.0),
        "the share it serves: {:?}",
        one.share
    );

    let all = quote(&paced(), &filed, Some(Duration::from_millis(100)));
    assert_eq!(millis(&all), total);
    assert_eq!(all.share, None, "everything fits");

    let unbudgeted = quote(&paced(), &filed, None);
    assert_eq!(millis(&unbudgeted), total);
    assert_eq!(unbudgeted.share, None, "no budget takes everything");

    assert_eq!(filed.waiting_scenes().len(), 1, "the quote took nothing");
    assert_eq!(filed.waiting_scripts().len(), 1, "the quote took nothing");
    assert_eq!(filed.waiting_assets().len(), 2, "the quote took nothing");
}

/// **Either lane loads the same requests.** The same requests filed twice:
/// paced, lent 25 ms, is run until its channels are empty; rush runs once.
/// Both prepare the same scenes in the same order, compile the same roots
/// with the same modules, decode the same assets in the same order, and
/// stage the same results.
#[test]
fn either_lane_loads_the_same_requests() {
    let paths = ["a.obj", "b.png", "c.wav"];
    let file = |filed: &Filed| {
        filed
            .file_scenes(&[1, 2])
            .file_scripts(&[
                script("game", &["a.erg"]),
                script("editor", &["x.erg"]),
                script("game", &["b.erg"]),
            ])
            .file_assets(&paths);
    };

    let rushed = Filed::knowing(&paths);
    file(&rushed);
    let rush_reserve = landed([run(&mut rush(), &rushed, None)]);

    let paced_filed = Filed::knowing(&paths);
    file(&paced_filed);
    let mut lane = paced();
    let mut stagings = Vec::new();
    for _ in 0..16 {
        if paced_filed.all_taken() {
            break;
        }
        stagings.push(run(
            &mut lane,
            &paced_filed,
            Some(Duration::from_millis(25)),
        ));
    }
    assert!(paced_filed.all_taken(), "paced emptied its channels");
    assert!(
        stagings.len() > 1,
        "paced spread the work over several runs"
    );
    let paced_reserve = landed(stagings);

    assert_eq!(paced_filed.loading.asked(), rushed.loading.asked());
    let roots = |filed: &Filed| -> BTreeSet<(PathBuf, BTreeSet<String>)> {
        filed
            .compiler
            .asked()
            .into_iter()
            .map(|request| (request.root, request.modules))
            .collect()
    };
    assert_eq!(roots(&paced_filed), roots(&rushed));
    assert_eq!(
        paced_filed.compiler.asked().len(),
        2,
        "one compile per root"
    );
    assert_eq!(paced_filed.decoding.asked(), rushed.decoding.asked());
    assert_eq!(tickets(&paced_reserve), tickets(&rush_reserve));
    assert_eq!(
        reloaded_modules(&paced_reserve),
        reloaded_modules(&rush_reserve)
    );
    assert_eq!(decoded_uuids(&paced_reserve), decoded_uuids(&rush_reserve));
}

// ─── One kind's panic and the other kinds ───────────────────────────────────

/// A decoder that panics on every decode, as a broken decoder of a malformed
/// file may.
struct PanickingDecoding;

impl AssetDecoding for PanickingDecoding {
    fn decode(&self, uuid: &AssetUUID, _type_id: TypeId) -> Decoded {
        panic!("the decoder of {uuid} panicked");
    }
}

/// **A decoder that panics loses no scene load taken beside it.** One scene
/// load and one asset wait; the decoder panics on the asset. The panic is the
/// asset's alone: the run still records, the scene load prepared beside it is
/// staged — never lost, its ticket pending forever and every later load held
/// back behind it — and the broken asset is staged as a failed decode whose
/// message names the panic, so its request is answered rather than left
/// pending. Both channels are left empty.
#[test]
fn a_decoder_that_panics_loses_no_scene_load_taken_beside_it() {
    let scenes = Channel::<SceneLoadRequest>::bounded(64, WhenFull::Reject);
    let scripts = Channel::<CompileRequest>::bounded(64, WhenFull::Reject);
    let assets = Channel::<DecodeRequest>::bounded(64, WhenFull::Reject);
    let loading = FakeLoading::new(&scenes);
    let mut runtime = Runtime::new();
    let _indexes = install_indexing(&mut runtime, &FakeIndexing::new());
    runtime.resources.insert(scenes.clone());
    runtime.resources.insert(scripts.clone());
    runtime.resources.insert(assets.clone());
    runtime
        .resources
        .insert(Arc::clone(&loading) as Arc<dyn SceneLoading>);
    runtime
        .resources
        .insert(FakeCompiler::echoing() as Arc<dyn ScriptCompiler>);
    runtime
        .resources
        .insert(Arc::new(PanickingDecoding) as Arc<dyn AssetDecoding>);
    scenes.send(scene(1));
    assets.send(asset("broken.png"));

    let mut lane = rush();
    let mut staging = Staging::new();
    let ran = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let bus = LaneBus::new();
        let mut source = InputSource::new(FrameId(0), &bus, &runtime);
        let mut deck = OutputDeck::new();
        let mut sink = OutputSink::new(&mut deck).with_staging(&mut staging);
        lane.run(0, &mut source, &mut sink, &mut GpuSetup::headless())
            .outcome
    }));
    let outcome = ran.expect("the decoder's panic stays within its own unit");
    assert!(
        matches!(outcome, RunOutcome::Recorded(_)),
        "the run records: {outcome:?}"
    );
    assert_eq!(
        loading.asked(),
        vec![LoadTicket(1)],
        "the scene was prepared"
    );

    let reserve = landed([staging]);
    assert_eq!(
        tickets(&reserve),
        vec![LoadTicket(1)],
        "the scene load prepared beside the panic is staged"
    );
    let decoded = reserve.take::<Decoded>();
    assert_eq!(
        decoded.len(),
        1,
        "the broken asset is answered: {decoded:?}"
    );
    assert_eq!(decoded[0].uuid, uuid("broken.png"));
    assert_eq!(decoded[0].type_id, TypeId::of::<u32>());
    match &decoded[0].value {
        Ok(_) => panic!("the broken asset is staged as a failure"),
        Err(message) => assert!(
            message.contains("panicked"),
            "the failure names the panic: {message}"
        ),
    }
    assert!(
        scenes.is_empty() && assets.is_empty(),
        "nothing left waiting: {} scene(s), {} asset(s)",
        scenes.read(&mut Cursor::default()).len(),
        assets.read(&mut Cursor::default()).len()
    );
}

/// A compiler that panics on every request of the root `broken`, as a
/// compiler bug on one project's scripts may, and compiles any other root as
/// the echoing fake does.
struct PanickingOnOneRoot;

impl ScriptCompiler for PanickingOnOneRoot {
    fn compile(&self, request: &CompileRequest) -> Compiled {
        if request.root == std::path::Path::new("broken") {
            panic!("the compiler of {} panicked", request.root.display());
        }
        Compiled {
            reloads: request.modules.iter().map(|m| reload(m)).collect(),
            declarations: request.modules.iter().map(|m| declarations(m)).collect(),
        }
    }
}

/// **A compiler that panics on one root fails that root alone.** The root
/// `broken` is filed first, then `game`, then one asset. The compiler panics
/// on `broken`: the run still records; `game` is compiled and its reload and
/// declarations staged; the asset is decoded and staged; `broken` stages
/// nothing — no reload, no declaration, so what its modules ran keeps
/// running — and its request is not given back, which would panic again on
/// every later run. Every channel is left empty.
#[test]
fn a_compiler_that_panics_on_one_root_fails_that_root_alone() {
    let scenes = Channel::<SceneLoadRequest>::bounded(64, WhenFull::Reject);
    let scripts = Channel::<CompileRequest>::bounded(64, WhenFull::Reject);
    let assets = Channel::<DecodeRequest>::bounded(64, WhenFull::Reject);
    let loading = FakeLoading::new(&scenes);
    let decoding = FakeDecoding::knowing(&[(uuid("a.png"), 7)]);
    let mut runtime = Runtime::new();
    let _indexes = install_indexing(&mut runtime, &FakeIndexing::new());
    runtime.resources.insert(scenes.clone());
    runtime.resources.insert(scripts.clone());
    runtime.resources.insert(assets.clone());
    runtime
        .resources
        .insert(Arc::clone(&loading) as Arc<dyn SceneLoading>);
    runtime
        .resources
        .insert(Arc::new(PanickingOnOneRoot) as Arc<dyn ScriptCompiler>);
    runtime
        .resources
        .insert(Arc::clone(&decoding) as Arc<dyn AssetDecoding>);
    scripts.send(script("broken", &["bad.erg"]));
    scripts.send(script("game", &["good.erg"]));
    assets.send(asset("a.png"));

    let mut lane = rush();
    let mut staging = Staging::new();
    let ran = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let bus = LaneBus::new();
        let mut source = InputSource::new(FrameId(0), &bus, &runtime);
        let mut deck = OutputDeck::new();
        let mut sink = OutputSink::new(&mut deck).with_staging(&mut staging);
        lane.run(0, &mut source, &mut sink, &mut GpuSetup::headless())
            .outcome
    }));
    let outcome = ran.expect("the compiler's panic stays within its own root");
    assert!(
        matches!(outcome, RunOutcome::Recorded(_)),
        "the run records: {outcome:?}"
    );

    let reserve = landed([staging]);
    assert_eq!(
        reserve.take::<ScriptReload>(),
        vec![reload("good.erg")],
        "the root compiled beside the panic is staged, and the broken one adds nothing"
    );
    assert_eq!(
        reserve.take::<ScriptDeclarations>(),
        vec![declarations("good.erg")],
        "the broken root withdraws no declaration"
    );
    assert_eq!(
        decoded_uuids(&reserve),
        vec![uuid("a.png")],
        "the asset taken beside the panic is decoded"
    );
    assert!(
        scripts.is_empty() && assets.is_empty(),
        "nothing given back to panic again: {} script request(s), {} asset(s)",
        scripts.read(&mut Cursor::default()).len(),
        assets.read(&mut Cursor::default()).len()
    );
}

// ─── Indexes ────────────────────────────────────────────────────────────────

/// The tickets of the project indexes `reserve` holds, in the order staged.
fn indexed_tickets(reserve: &Reserve) -> Vec<IndexTicket> {
    reserve
        .take::<IndexedAssets>()
        .iter()
        .map(|indexed| indexed.ticket)
        .collect()
}

/// **Rush indexes every project it takes and stages one result each, never
/// merging two.** Two reindexes of `game` wait beside one of `editor`: the
/// indexer is asked for each, in the order they were filed — an index does
/// not coalesce, a reindex filed after another lands after it — each result
/// reaches the reserve under its own ticket, with its kind and root, and the
/// index channel is left empty.
#[test]
fn rush_indexes_every_project_it_takes_and_stages_one_result_each() {
    let filed = Filed::knowing(&[]);
    filed.file_indexes(&[index(1, "game"), index(2, "editor"), index(3, "game")]);

    let reserve = landed([run(&mut rush(), &filed, None)]);

    assert_eq!(
        filed.indexing.asked(),
        vec![IndexTicket(1), IndexTicket(2), IndexTicket(3)],
        "each request indexed, in filing order"
    );
    let indexed = reserve.take::<IndexedAssets>();
    let got: Vec<(IndexTicket, IndexKind, PathBuf)> = indexed
        .iter()
        .map(|indexed| (indexed.ticket, indexed.kind, indexed.project_root.clone()))
        .collect();
    assert_eq!(
        got,
        vec![
            (IndexTicket(1), IndexKind::Reindex, PathBuf::from("game")),
            (IndexTicket(2), IndexKind::Reindex, PathBuf::from("editor")),
            (IndexTicket(3), IndexKind::Reindex, PathBuf::from("game")),
        ],
        "one result per request, none merged"
    );
    assert!(
        indexed.iter().all(|indexed| indexed.index.is_ok()),
        "each staged with its bytes: {indexed:?}"
    );
    assert!(filed.indexes.is_empty(), "the requests were taken");
}

/// **Each index is staged under its own lease.** Two projects are indexed
/// in one run; another project replaces one of them before the frame start
/// receives the run: its index is abandoned there, and only its — the other
/// lands.
#[test]
fn an_index_whose_project_was_replaced_while_it_waited_is_abandoned_and_only_it() {
    let filed = Filed::knowing(&[]);
    filed.file_indexes(&[index(1, "replaced"), index(2, "kept")]);

    let staging = run(&mut rush(), &filed, None);
    filed.indexing.replace("replaced");
    let reserve = landed([staging]);

    assert_eq!(
        indexed_tickets(&reserve),
        vec![IndexTicket(2)],
        "the replaced project's index never lands"
    );
}

/// **An index is priced at 50 ms**, before any is measured. One index and
/// one asset wait: rush quotes 54 ms and serves it all; paced lent 1 ms
/// would take the index alone — 50 ms, 50/54 of the work.
#[test]
fn an_index_unit_is_quoted_at_50_ms() {
    let filed = Filed::knowing(&[]);
    filed
        .file_indexes(&[index(1, "game")])
        .file_assets(&["a.obj"]);

    let all = quote(&rush(), &filed, Some(Duration::from_millis(1)));
    assert_eq!(millis(&all), INDEX + ASSET);
    assert_eq!(all.share, None, "rush serves all the waiting work");

    let one = quote(&paced(), &filed, Some(Duration::from_millis(1)));
    assert_eq!(millis(&one), INDEX, "paced would take the index alone");
    assert!(
        one.share
            .is_some_and(|share| (share - 50.0 / 54.0).abs() < 1e-4),
        "the share it serves: {:?}",
        one.share
    );
    assert_eq!(
        filed.waiting_indexes(),
        vec![IndexTicket(1)],
        "the quote took nothing"
    );
}

/// **Loading takes the indexes first**: the scenes and the assets read
/// through the index, so a project's index goes before them, whatever order
/// they were filed in. An asset, then a scene, then an index are filed;
/// paced, lent 1 ms — less than any unit — takes one unit: the index. The
/// scene and the asset go back.
#[test]
fn loading_takes_indexes_first() {
    let filed = Filed::knowing(&["a.obj"]);
    filed
        .file_assets(&["a.obj"])
        .file_scenes(&[1])
        .file_indexes(&[index(7, "game")]);

    let reserve = landed([run(&mut paced(), &filed, Some(Duration::from_millis(1)))]);

    assert_eq!(
        filed.indexing.asked(),
        vec![IndexTicket(7)],
        "the index is taken first"
    );
    assert!(filed.loading.asked().is_empty(), "no scene prepared");
    assert!(filed.decoding.asked().is_empty(), "no asset decoded");
    assert_eq!(indexed_tickets(&reserve), vec![IndexTicket(7)]);
    assert!(filed.indexes.is_empty(), "the index was taken");
    assert_eq!(
        filed.waiting_scenes(),
        vec![LoadTicket(1)],
        "the scene went back"
    );
    assert_eq!(
        filed.waiting_assets(),
        vec![asset("a.obj")],
        "the asset went back"
    );
}

/// **Paced takes the indexes before the scenes within its budget.** Lent
/// 75 ms, with two indexes (50 ms each) and a scene waiting: the first index
/// is taken, then the second does not fit and stops the taking — the second
/// index and the scene go back, in order.
#[test]
fn paced_stops_at_the_first_index_that_does_not_fit() {
    let filed = Filed::knowing(&[]);
    filed
        .file_scenes(&[1])
        .file_indexes(&[index(1, "game"), index(2, "editor")]);

    let reserve = landed([run(&mut paced(), &filed, Some(Duration::from_millis(75)))]);

    assert_eq!(filed.indexing.asked(), vec![IndexTicket(1)]);
    assert_eq!(indexed_tickets(&reserve), vec![IndexTicket(1)]);
    assert_eq!(filed.waiting_indexes(), vec![IndexTicket(2)]);
    assert_eq!(filed.waiting_scenes(), vec![LoadTicket(1)]);
    assert!(filed.loading.asked().is_empty(), "no scene prepared");
}
