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

//! **An asset asked for and not cached is decoded off the frame, and lands at
//! a frame's boundary.** `AssetService::request` answers at once — the
//! handle, `Pending` or `Failed` — and files one `DecodeRequest` per asset
//! while it is pending. The `loading` family's work is played here by hand, as
//! the scheduler runs it at the background: the requests filed are taken,
//! decoded through the service's `AssetDecoding` and staged under their
//! lease into the engine's reserve. The `asset_intake` DataSystem then takes
//! them in at the boundary: into the service's cache and bookkeeping, and
//! into the `AssetStore`.
//!
//! An asset that changes while its decode is in flight — invalidated,
//! reindexed — moves to a new generation: the decode for the old one is
//! abandoned when the reserve receives it, never installed.

use std::any::TypeId;
use std::path::Path;
use std::sync::{Arc, Mutex, MutexGuard};

use khora_core::asset::font::Font;
use khora_core::asset::{AssetDecoding, AssetUUID, DecodeRequest, Decoded};
use khora_core::lane::{OutputDeck, Reserve, Staging};
use khora_core::renderer::api::gpu_scene::Mesh;
use khora_core::Runtime;
use khora_data::ecs::{DataSystemRegistration, Role, SystemContext, Tick, TickPhase, World};
use khora_data::AssetStore;
use khora_io::asset::{AssetService, FileLoader, IndexBuilder, MeshDispatcher, Requested};
use khora_telemetry::MetricsRegistry;

// ─── The harness ────────────────────────────────────────────────────────────

/// The registered data system called `name`.
pub(crate) fn data_system(name: &str) -> &'static DataSystemRegistration {
    inventory::iter::<DataSystemRegistration>
        .into_iter()
        .find(|system| system.name == name)
        .unwrap_or_else(|| panic!("`{name}` is not registered"))
}

fn run_system(name: &str, world: &mut World, runtime: &Runtime) {
    (data_system(name).run)(
        world,
        runtime,
        &mut OutputDeck::new(),
        SystemContext {
            since: Tick(0),
            resumed: false,
            scope: None,
        },
    );
}

/// A project's assets on disk, indexed and served by a real `AssetService`
/// (the inventory decoders, and meshes through the `MeshDispatcher`), wired
/// into a runtime as the engine wires it: the service, the `AssetStore`, the
/// reserve the `loading` family stages into.
pub(crate) struct Project {
    dir: tempfile::TempDir,
    pub(crate) service: Arc<Mutex<AssetService>>,
    pub(crate) store: AssetStore,
    pub(crate) reserve: Reserve,
    pub(crate) runtime: Runtime,
}

impl Project {
    pub(crate) fn new(files: &[(&str, &[u8])]) -> Self {
        let dir = tempfile::tempdir().expect("a temporary project");
        for (rel, bytes) in files {
            write_file(dir.path(), rel, bytes);
        }
        let index_bytes = IndexBuilder::new(dir.path())
            .build_index_bytes()
            .expect("the project indexes");
        let mut service = AssetService::new(
            &index_bytes,
            Box::new(FileLoader::new(dir.path())),
            Arc::new(MetricsRegistry::new()),
            None,
        )
        .expect("an asset service over the project");
        service.register_inventory_decoders();
        service.register_decoder::<Mesh>("mesh", MeshDispatcher::default());
        let service = Arc::new(Mutex::new(service));
        let store = AssetStore::new();
        let reserve = Reserve::new();
        let mut runtime = Runtime::default();
        runtime.resources.insert(store.clone());
        runtime.resources.insert(reserve.clone());
        runtime.services.insert(Arc::clone(&service));
        Self {
            dir,
            service,
            store,
            reserve,
            runtime,
        }
    }

    /// The folder the service reads the project's files from.
    pub(crate) fn root(&self) -> &Path {
        self.dir.path()
    }

    pub(crate) fn write(&self, rel: &str, bytes: &[u8]) {
        write_file(self.dir.path(), rel, bytes);
    }

    pub(crate) fn remove(&self, rel: &str) {
        std::fs::remove_file(self.dir.path().join(rel)).expect("the file is removed");
    }

    /// The project indexed as it is on disk now.
    pub(crate) fn index_bytes(&self) -> Vec<u8> {
        IndexBuilder::new(self.dir.path())
            .build_index_bytes()
            .expect("the project indexes")
    }

    pub(crate) fn svc(&self) -> MutexGuard<'_, AssetService> {
        self.service.lock().expect("the service's lock")
    }

    /// The requests the service filed and nothing took yet, taken — what
    /// the `loading` family's lanes take.
    pub(crate) fn filed(&self) -> Vec<DecodeRequest> {
        let requests = self.svc().decode_requests();
        requests.drain()
    }

    /// `requests` decoded through the service's `AssetDecoding`, each staged
    /// under its lease — what one run of the decode lane stages.
    pub(crate) fn decode(&self, requests: &[DecodeRequest]) -> Staging {
        let decoding: Arc<dyn AssetDecoding> = AssetService::decoding(&self.service);
        let mut staging = Staging::new();
        for request in requests {
            let (decoded, lease) = decoding.decode_leased(request);
            match lease {
                Some(lease) => staging.stage_while(vec![decoded], lease),
                None => staging.stage(vec![decoded]),
            }
        }
        staging
    }

    /// `staging` received by the engine's reserve, as a frame start does.
    pub(crate) fn land(&self, staging: Staging) {
        self.reserve.receive(staging);
    }

    /// One whole run of the `loading` family: what was filed, decoded and
    /// received. Returns how many requests it took.
    pub(crate) fn background(&self) -> usize {
        let filed = self.filed();
        let staging = self.decode(&filed);
        self.land(staging);
        filed.len()
    }

    /// The boundary's `asset_intake`.
    pub(crate) fn intake(&self, world: &mut World) {
        run_system("asset_intake", world, &self.runtime);
    }

    /// The `asset_resolver`, as `PreExtract` runs it.
    pub(crate) fn resolve(&self, world: &mut World) {
        run_system("asset_resolver", world, &self.runtime);
    }

    /// One frame of the asset systems: the boundary's intake, then the
    /// resolver. (The intake's registration is pinned by its own test; a
    /// frame without it still runs the resolver, so a resolver test fails on
    /// what the resolver does.)
    pub(crate) fn frame(&self, world: &mut World) {
        let intake = inventory::iter::<DataSystemRegistration>
            .into_iter()
            .any(|system| system.name == "asset_intake");
        if intake {
            self.intake(world);
        }
        self.resolve(world);
    }

    /// Whether the `AssetStore` holds `uuid` as an `A`.
    pub(crate) fn stored<A: khora_core::asset::Asset>(&self, uuid: &AssetUUID) -> bool {
        self.store
            .store::<A>()
            .read()
            .expect("the store's lock")
            .contains(uuid)
    }
}

pub(crate) fn write_file(root: &Path, rel: &str, bytes: &[u8]) {
    let path = root.join(rel);
    std::fs::create_dir_all(path.parent().expect("a parent folder")).expect("the folders");
    std::fs::write(path, bytes).expect("the file is written");
}

/// The bytes of a font file; the font decoder keeps them as they are.
const FONT_BYTES: &[u8] = b"font file bytes";
const FONT: &str = "fonts/body.ttf";

fn font() -> AssetUUID {
    AssetUUID::new_v5(FONT)
}

/// What a request answered, named, for assertions.
fn answer<A: khora_core::asset::Asset>(requested: &Requested<A>) -> &'static str {
    match requested {
        Requested::Ready(_) => "Ready",
        Requested::Pending => "Pending",
        Requested::Failed => "Failed",
    }
}

fn font_data(requested: Requested<Font>) -> Option<Vec<u8>> {
    match requested {
        Requested::Ready(handle) => Some(handle.data.clone()),
        _ => None,
    }
}

// ─── Requesting ─────────────────────────────────────────────────────────────

/// **A request for an asset not cached files one decode and answers
/// `Pending`.** The request names the asset and the type asked for.
#[test]
fn a_request_for_an_uncached_asset_files_one_decode_and_answers_pending() {
    let project = Project::new(&[(FONT, FONT_BYTES)]);

    let answered = project.svc().request::<Font>(&font());

    assert_eq!(answer(&answered), "Pending");
    let filed = project.filed();
    assert_eq!(filed.len(), 1, "one request filed: {filed:?}");
    assert_eq!(filed[0].uuid, font());
    assert_eq!(filed[0].type_id, TypeId::of::<Font>());
}

/// **Asking again while the decode is pending files nothing more**: a
/// resolver asking every frame costs one decode.
#[test]
fn asking_again_while_pending_files_nothing_more() {
    let project = Project::new(&[(FONT, FONT_BYTES)]);

    for _ in 0..5 {
        let answered = project.svc().request::<Font>(&font());
        assert_eq!(answer(&answered), "Pending");
    }

    assert_eq!(project.filed().len(), 1, "one request for five asks");
}

/// **A request for an asset already cached answers its handle at once and
/// files nothing** — the handle a synchronous load cached.
#[test]
fn a_request_for_a_cached_asset_answers_its_handle_and_files_nothing() {
    let project = Project::new(&[(FONT, FONT_BYTES)]);
    let loaded = project.svc().load::<Font>(&font()).expect("the font loads");

    let answered = project.svc().request::<Font>(&font());

    match answered {
        Requested::Ready(handle) => assert!(handle == loaded, "the cached handle"),
        other => panic!("a cached asset is ready, not {}", answer(&other)),
    }
    assert!(project.filed().is_empty(), "nothing to decode");
}

// ─── The intake ─────────────────────────────────────────────────────────────

/// **The intake runs at the frame's boundary, whatever families are on.**
#[test]
fn the_asset_intake_runs_at_the_boundary_as_upkeep() {
    let intake = data_system("asset_intake");

    assert_eq!(intake.phase, TickPhase::Boundary);
    assert_eq!(intake.role, Role::Upkeep);
}

/// **A decode that landed is taken in at the boundary: the asset is ready
/// and in the `AssetStore`.** Requested, decoded off the frame and received
/// by the reserve, the font is installed by the intake: the next request
/// answers its handle, with the file's bytes, and files nothing; the store
/// holds it; the reserve is left empty.
#[test]
fn a_decode_taken_in_at_the_boundary_makes_the_asset_ready_and_stored() {
    let project = Project::new(&[(FONT, FONT_BYTES)]);
    let mut world = World::new();
    assert_eq!(answer(&project.svc().request::<Font>(&font())), "Pending");

    assert_eq!(project.background(), 1, "the request was decoded");
    project.intake(&mut world);

    assert_eq!(
        font_data(project.svc().request::<Font>(&font())),
        Some(FONT_BYTES.to_vec()),
        "the decoded font is ready"
    );
    assert!(project.filed().is_empty(), "a ready asset files nothing");
    assert!(project.stored::<Font>(&font()), "the store holds it");
    assert!(
        project.reserve.is_empty::<Decoded>(),
        "the intake took what was staged"
    );
}

/// **Until the boundary takes it in, a decode that landed is not served**:
/// the asset stays pending — and files nothing more.
#[test]
fn a_landed_decode_is_not_served_before_the_intake_takes_it_in() {
    let project = Project::new(&[(FONT, FONT_BYTES)]);
    assert_eq!(answer(&project.svc().request::<Font>(&font())), "Pending");
    project.background();

    assert_eq!(answer(&project.svc().request::<Font>(&font())), "Pending");
    assert!(project.filed().is_empty(), "still one request");
}

/// **An asset the intake installed counts as served**: a service replacing
/// this one announces it, so the engine replaces its copy.
#[test]
fn an_asset_taken_in_by_the_intake_counts_as_served() {
    let project = Project::new(&[(FONT, FONT_BYTES)]);
    let mut world = World::new();
    project.svc().request::<Font>(&font());
    project.background();
    project.intake(&mut world);

    let incoming = AssetService::new(
        &project.index_bytes(),
        Box::new(FileLoader::new(project.dir.path())),
        Arc::new(MetricsRegistry::new()),
        None,
    )
    .expect("a second service");
    incoming.announce_replacing(&project.svc());

    assert!(
        incoming.take_changes().contains(&font()),
        "what the intake installed was served"
    );
}

/// **The intake records the files a container read besides its own**: a
/// glTF decoded off the frame, then its `.bin` changes — the glTF is
/// invalidated with it, announced and dropped, as a synchronous load
/// recorded it.
#[test]
fn the_intake_records_the_files_a_container_read_beside_it() {
    let (gltf, bin) = triangle_gltf_with_buffer("tri.bin");
    let project = Project::new(&[("models/tri.gltf", &gltf), ("models/tri.bin", &bin)]);
    let mut service = project.svc();
    service.register_default_decoders(project.dir.path());
    drop(service);
    let file = AssetUUID::new_v5("models/tri.gltf");
    assert!(
        project
            .svc()
            .reader()
            .decode(&file, TypeId::of::<Mesh>())
            .value
            .is_ok(),
        "the premise: the glTF decodes whole as a mesh"
    );
    let mut world = World::new();
    project.svc().request::<Mesh>(&file);
    project.background();
    project.intake(&mut world);
    assert_eq!(answer(&project.svc().request::<Mesh>(&file)), "Ready");
    project.svc().take_changes();

    project
        .svc()
        .invalidate(&AssetUUID::new_v5("models/tri.bin"));

    assert!(
        project.svc().take_changes().contains(&file),
        "the glTF reading the changed `.bin` is announced"
    );
    assert_eq!(
        answer(&project.svc().request::<Mesh>(&file)),
        "Pending",
        "and dropped: it is decoded again"
    );
}

// ─── Failures ───────────────────────────────────────────────────────────────

/// **A decode that failed is recorded: the asset answers `Failed` and files
/// nothing, until an asset changes.** The font is indexed, then its file
/// disappears: its decode fails off the frame, the intake records it —
/// `has_failed` — and asking again costs nothing. An invalidation forgets
/// the failure: the next request is filed again.
#[test]
fn a_failed_decode_answers_failed_until_an_asset_changes() {
    let project = Project::new(&[(FONT, FONT_BYTES)]);
    project.remove(FONT);
    let mut world = World::new();
    project.svc().request::<Font>(&font());
    project.background();
    project.intake(&mut world);

    assert!(project.svc().has_failed(&font()), "the failure is recorded");
    for _ in 0..3 {
        assert_eq!(answer(&project.svc().request::<Font>(&font())), "Failed");
    }
    assert!(project.filed().is_empty(), "a failed asset files nothing");
    assert!(
        !project.stored::<Font>(&font()),
        "nothing reaches the store"
    );

    project
        .svc()
        .invalidate(&AssetUUID::new_v5("fonts/other.ttf"));

    assert!(!project.svc().has_failed(&font()), "a change forgets it");
    assert_eq!(answer(&project.svc().request::<Font>(&font())), "Pending");
    assert_eq!(project.filed().len(), 1, "it is asked for again");
}

/// **A new index forgets a failure too.**
#[test]
fn a_failed_decode_is_forgotten_by_a_new_index() {
    let project = Project::new(&[(FONT, FONT_BYTES)]);
    project.remove(FONT);
    let mut world = World::new();
    project.svc().request::<Font>(&font());
    project.background();
    project.intake(&mut world);
    assert_eq!(answer(&project.svc().request::<Font>(&font())), "Failed");

    project.write(FONT, FONT_BYTES);
    let index = project.index_bytes();
    project
        .svc()
        .reindex(&index)
        .expect("the index is replaced");

    assert_eq!(answer(&project.svc().request::<Font>(&font())), "Pending");
    project.background();
    project.intake(&mut world);
    assert_eq!(
        font_data(project.svc().request::<Font>(&font())),
        Some(FONT_BYTES.to_vec()),
        "the restored file decodes"
    );
}

/// **A request for an asset no index lists decodes to a failure, never a
/// panic**, and the decode is still staged for the intake to record.
#[test]
fn a_request_for_an_unknown_asset_stages_a_failure() {
    let project = Project::new(&[(FONT, FONT_BYTES)]);
    let unknown = AssetUUID::new_v5("fonts/never_indexed.ttf");
    let mut world = World::new();
    assert_eq!(answer(&project.svc().request::<Font>(&unknown)), "Pending");

    let filed = project.filed();
    let staging = project.decode(&filed);
    project.land(staging);
    assert_eq!(project.reserve.len::<Decoded>(), 1, "the failure is staged");
    project.intake(&mut world);

    assert!(project.svc().has_failed(&unknown));
    assert_eq!(answer(&project.svc().request::<Font>(&unknown)), "Failed");
}

// ─── Generations ────────────────────────────────────────────────────────────

/// **A file invalidated while its decode is in flight is never installed
/// from its old bytes.** The decode read the font before it changed; the
/// invalidation moves the asset to a new generation: the reserve abandons
/// the old decode as it receives it. Asked again, the font is filed anew,
/// for a later generation, and decodes from the bytes on disk now.
#[test]
fn an_asset_invalidated_while_its_decode_is_in_flight_is_never_installed_stale() {
    let project = Project::new(&[(FONT, b"old font")]);
    let mut world = World::new();
    project.svc().request::<Font>(&font());
    let first = project.filed();
    let in_flight = project.decode(&first);

    project.write(FONT, b"new font");
    project.svc().invalidate(&font());
    project.land(in_flight);

    assert!(
        project.reserve.is_empty::<Decoded>(),
        "the stale decode is abandoned at receipt"
    );
    project.intake(&mut world);
    assert_eq!(
        answer(&project.svc().request::<Font>(&font())),
        "Pending",
        "nothing stale is served; the asset is asked for again"
    );
    let again = project.filed();
    assert_eq!(again.len(), 1, "filed anew: {again:?}");
    assert_ne!(
        again[0].generation, first[0].generation,
        "for the asset's new generation"
    );
    let staging = project.decode(&again);
    project.land(staging);
    project.intake(&mut world);
    assert_eq!(
        font_data(project.svc().request::<Font>(&font())),
        Some(b"new font".to_vec())
    );
}

/// **A reindex while a decode is in flight abandons it too**: what the old
/// index named may not be what the new one names.
#[test]
fn a_reindex_while_a_decode_is_in_flight_abandons_it() {
    let project = Project::new(&[(FONT, b"old font")]);
    let mut world = World::new();
    project.svc().request::<Font>(&font());
    let in_flight = project.decode(&project.filed());

    project.write(FONT, b"new font");
    let index = project.index_bytes();
    project
        .svc()
        .reindex(&index)
        .expect("the index is replaced");
    project.land(in_flight);

    assert!(
        project.reserve.is_empty::<Decoded>(),
        "the decode of the old index is abandoned at receipt"
    );
    project.intake(&mut world);
    assert_eq!(answer(&project.svc().request::<Font>(&font())), "Pending");
    project.background();
    project.intake(&mut world);
    assert_eq!(
        font_data(project.svc().request::<Font>(&font())),
        Some(b"new font".to_vec())
    );
}

/// **Invalidating another asset leaves a decode in flight standing**: the
/// lease holds to its own asset's generation, not to every change.
#[test]
fn invalidating_another_asset_leaves_a_decode_in_flight_standing() {
    let project = Project::new(&[(FONT, FONT_BYTES), ("fonts/title.ttf", b"title")]);
    let mut world = World::new();
    project.svc().request::<Font>(&font());
    let in_flight = project.decode(&project.filed());

    project
        .svc()
        .invalidate(&AssetUUID::new_v5("fonts/title.ttf"));
    project.land(in_flight);
    project.intake(&mut world);

    assert_eq!(
        font_data(project.svc().request::<Font>(&font())),
        Some(FONT_BYTES.to_vec())
    );
}

/// **The decoding follows the service's current index**: taken before a
/// reindex, it decodes an asset only the new index lists.
#[test]
fn the_decoding_reads_the_index_the_service_holds_now() {
    let project = Project::new(&[(FONT, FONT_BYTES)]);
    let decoding = AssetService::decoding(&project.service);
    project.write("fonts/added.ttf", b"added");
    let index = project.index_bytes();
    project
        .svc()
        .reindex(&index)
        .expect("the index is replaced");

    let decoded = decoding.decode(&AssetUUID::new_v5("fonts/added.ttf"), TypeId::of::<Font>());

    let data = decoded
        .value
        .expect("the new index's asset decodes")
        .downcast::<Font>()
        .expect("a font")
        .data;
    assert_eq!(data, b"added");
}

/// **The decoding follows the service its mutex holds**: the editor swaps a
/// project's service into the engine's; the decoding, taken before, decodes
/// the new project's assets.
#[test]
fn the_decoding_reads_the_service_swapped_into_its_mutex() {
    let first = Project::new(&[(FONT, FONT_BYTES)]);
    let second = Project::new(&[("fonts/second.ttf", b"second")]);
    let decoding = AssetService::decoding(&first.service);

    std::mem::swap(&mut *first.svc(), &mut *second.svc());

    let decoded = decoding.decode(&AssetUUID::new_v5("fonts/second.ttf"), TypeId::of::<Font>());
    assert!(
        decoded.value.is_ok(),
        "the swapped-in service's asset decodes: {:?}",
        decoded.value.err()
    );
}

/// **A change to another asset while a decode is pending does not decode
/// it twice**: the decode in flight read nothing that changed, so it still
/// lands; asking again files nothing more.
#[test]
fn a_change_to_another_asset_while_a_decode_is_pending_files_nothing_more() {
    let project = Project::new(&[(FONT, FONT_BYTES), ("fonts/title.ttf", b"title")]);
    assert_eq!(answer(&project.svc().request::<Font>(&font())), "Pending");

    project
        .svc()
        .invalidate(&AssetUUID::new_v5("fonts/title.ttf"));
    assert_eq!(answer(&project.svc().request::<Font>(&font())), "Pending");

    assert_eq!(project.filed().len(), 1, "one decode for one asset");
}

// ─── Synchronous loads ──────────────────────────────────────────────────────

/// **A synchronous load answers at once while a request for the asset is
/// pending**, for the host code and the prefabs that need an answer within
/// the frame; the request then answers the loaded handle, and the decode
/// landing after changes nothing a reader sees.
#[test]
fn a_synchronous_load_answers_at_once_while_a_request_is_pending() {
    let project = Project::new(&[(FONT, FONT_BYTES)]);
    let mut world = World::new();
    assert_eq!(answer(&project.svc().request::<Font>(&font())), "Pending");

    let loaded = project
        .svc()
        .load::<Font>(&font())
        .expect("the load answers");

    assert_eq!(loaded.data, FONT_BYTES);
    assert_eq!(
        font_data(project.svc().request::<Font>(&font())),
        Some(FONT_BYTES.to_vec()),
        "the request answers what the load cached"
    );
    project.background();
    project.intake(&mut world);
    assert_eq!(
        font_data(project.svc().request::<Font>(&font())),
        Some(FONT_BYTES.to_vec())
    );
    assert!(project.filed().is_empty(), "nothing more was filed");
}

// ─── Containers changed while a decode is in flight ─────────────────────────

/// The x of the second corner of the mesh `uuid` once its decode settled:
/// what was landed is taken in, and what is still pending is decoded and
/// taken in, at most three rounds.
fn settled_second_corner_x(project: &Project, world: &mut World, uuid: &AssetUUID) -> f32 {
    for _ in 0..3 {
        project.intake(world);
        if let Requested::Ready(mesh) = project.svc().request::<Mesh>(uuid) {
            return mesh.positions[1].x;
        }
        project.background();
    }
    panic!("the mesh {uuid} never settled");
}

/// **A part of a glTF whose file changed while the part's decode was in
/// flight is never installed from the file's old bytes.** The part was never
/// served yet, so the container does not know it holds it; the change to
/// the container still moves it to a new generation.
#[test]
fn a_part_of_a_gltf_changed_while_its_decode_is_in_flight_is_never_installed_stale() {
    let (gltf, bin) = triangle_gltf_with_buffer("tri.bin");
    let project = Project::new(&[("models/tri.gltf", &gltf), ("models/tri.bin", &bin)]);
    project.svc().register_default_decoders(project.root());
    let file = AssetUUID::new_v5("models/tri.gltf");
    let part = AssetUUID::sub_asset(file, "mesh0/prim0");
    let mut world = World::new();
    assert_eq!(answer(&project.svc().request::<Mesh>(&part)), "Pending");
    let in_flight = project.decode(&project.filed());

    // The glTF now reads another buffer, its triangle half the size.
    let (gltf, _) = triangle_gltf_with_buffer("tri2.bin");
    project.write("models/tri.gltf", &gltf);
    project.write("models/tri2.bin", &half_triangle_bin());
    project.svc().invalidate(&file);
    project.land(in_flight);

    assert_eq!(
        settled_second_corner_x(&project, &mut world, &part),
        0.5,
        "the part decoded before its glTF changed is never served"
    );
}

/// **A glTF whose buffer beside it changed while the glTF's first decode
/// was in flight is never installed from the old buffer.** No decode of it
/// was taken in yet, so nothing recorded that it reads the buffer; the
/// change to the buffer still reaches the decode in flight.
#[test]
fn a_gltf_whose_buffer_changed_while_its_first_decode_is_in_flight_is_never_installed_stale() {
    let (gltf, bin) = triangle_gltf_with_buffer("tri.bin");
    let project = Project::new(&[("models/tri.gltf", &gltf), ("models/tri.bin", &bin)]);
    project.svc().register_default_decoders(project.root());
    let file = AssetUUID::new_v5("models/tri.gltf");
    let mut world = World::new();
    assert_eq!(answer(&project.svc().request::<Mesh>(&file)), "Pending");
    let in_flight = project.decode(&project.filed());

    project.write("models/tri.bin", &half_triangle_bin());
    project
        .svc()
        .invalidate(&AssetUUID::new_v5("models/tri.bin"));
    project.land(in_flight);

    assert_eq!(
        settled_second_corner_x(&project, &mut world, &file),
        0.5,
        "the glTF decoded from the buffer before it changed is never served"
    );
}

/// **A failure decoded before a change lands after it without sticking.**
/// A change forgets every failure, since a file one asset reads besides its
/// own may be what failed it: a glTF exported before its buffer fails to
/// decode; the buffer is written and announced while that decode is in
/// flight. The glTF is asked for again, and decodes, rather than answering
/// `Failed` until some other file changes.
#[test]
fn a_failure_decoded_before_its_missing_buffer_appeared_does_not_stick() {
    let (gltf, bin) = triangle_gltf_with_buffer("tri.bin");
    let project = Project::new(&[("models/tri.gltf", &gltf)]);
    project.svc().register_default_decoders(project.root());
    let file = AssetUUID::new_v5("models/tri.gltf");
    let mut world = World::new();
    assert_eq!(answer(&project.svc().request::<Mesh>(&file)), "Pending");
    let in_flight = project.decode(&project.filed());

    project.write("models/tri.bin", &bin);
    project
        .svc()
        .invalidate(&AssetUUID::new_v5("models/tri.bin"));
    project.land(in_flight);

    assert_eq!(
        settled_second_corner_x(&project, &mut world, &file),
        1.0,
        "the glTF decodes once its buffer is there"
    );
}

/// `project`'s service opened again over an index whose files `frozen`
/// keep the uuid the registry froze for them, not the one their path gives;
/// every default decoder and container registered.
fn reopen_with_frozen_ids(project: &Project, frozen: &[(&str, AssetUUID)]) {
    let mut registry = khora_io::asset::AssetIdRegistry::load(project.root());
    for (rel, uuid) in frozen {
        registry.freeze(rel, *uuid);
    }
    let index = IndexBuilder::new(project.root())
        .with_registry(&registry)
        .build_index_bytes()
        .expect("the project indexes");
    let mut service = AssetService::new(
        &index,
        Box::new(FileLoader::new(project.root())),
        Arc::new(MetricsRegistry::new()),
        None,
    )
    .expect("an asset service over the project");
    service.register_default_decoders(project.root());
    *project.svc() = service;
}

/// **A glTF reading a buffer whose uuid the registry froze is never
/// installed stale when the buffer changes during its first decode**: the
/// change is announced under the frozen uuid, the decode names the buffer by
/// its path.
#[test]
fn a_gltf_whose_frozen_id_buffer_changed_while_its_decode_is_in_flight_is_never_installed_stale() {
    let (gltf, bin) = triangle_gltf_with_buffer("tri.bin");
    let project = Project::new(&[("models/tri.gltf", &gltf), ("models/tri.bin", &bin)]);
    let frozen_bin = AssetUUID::new_v5("an id frozen for the buffer");
    reopen_with_frozen_ids(&project, &[("models/tri.bin", frozen_bin)]);
    let file = AssetUUID::new_v5("models/tri.gltf");
    let mut world = World::new();
    assert_eq!(answer(&project.svc().request::<Mesh>(&file)), "Pending");
    let in_flight = project.decode(&project.filed());

    project.write("models/tri.bin", &half_triangle_bin());
    project.svc().invalidate(&frozen_bin);
    project.land(in_flight);

    assert_eq!(settled_second_corner_x(&project, &mut world, &file), 0.5);
}

/// **A part of a glTF whose uuid the registry froze is never installed
/// stale when the glTF changes during the part's decode.**
#[test]
fn a_part_of_a_frozen_id_gltf_changed_while_its_decode_is_in_flight_is_never_installed_stale() {
    let (gltf, bin) = triangle_gltf_with_buffer("tri.bin");
    let project = Project::new(&[("models/tri.gltf", &gltf), ("models/tri.bin", &bin)]);
    let frozen = AssetUUID::new_v5("an id frozen for the gltf");
    reopen_with_frozen_ids(&project, &[("models/tri.gltf", frozen)]);
    let part = AssetUUID::sub_asset(frozen, "mesh0/prim0");
    let mut world = World::new();
    assert_eq!(answer(&project.svc().request::<Mesh>(&part)), "Pending");
    let in_flight = project.decode(&project.filed());

    let (gltf, _) = triangle_gltf_with_buffer("tri2.bin");
    project.write("models/tri.gltf", &gltf);
    project.write("models/tri2.bin", &half_triangle_bin());
    project.svc().invalidate(&frozen);
    project.land(in_flight);

    assert_eq!(settled_second_corner_x(&project, &mut world, &part), 0.5);
}

// ─── A service swapped while a decode is in flight ──────────────────────────

/// **A failure the service swapped out decoded never lands in the one
/// swapped in.** The engine's first service (the editor's, before a project
/// is open) does not hold the font, so its decode fails; the project's
/// service, swapped in meanwhile, holds it and serves it.
#[test]
fn a_failure_decoded_by_a_service_swapped_out_never_lands_in_the_one_swapped_in() {
    let engine = Project::new(&[("fonts/other.ttf", b"other")]);
    let project = Project::new(&[(FONT, b"incoming project")]);
    let mut world = World::new();
    assert_eq!(answer(&engine.svc().request::<Font>(&font())), "Pending");
    let in_flight = engine.decode(&engine.filed());

    // As `ProjectVfs::share_assets` moves the project's service in.
    {
        let mut own = project.svc();
        let mut target = engine.svc();
        std::mem::swap(&mut *own, &mut *target);
        target.announce_replacing(&own);
        target.adopt_decode_requests(&mut own);
    }
    engine.land(in_flight);
    engine.intake(&mut world);

    assert!(
        !engine.svc().has_failed(&font()),
        "the outgoing service's failure is not the incoming one's"
    );
    assert_ne!(answer(&engine.svc().request::<Font>(&font())), "Failed");
}

/// **A decode of a type the incoming service already asks for, made by the
/// service swapped out, never lands in the one swapped in.** The editor
/// moves a project's service into the engine's (`ProjectVfs::share_assets`):
/// the two hold the same uuid for the same path, with other bytes. The
/// decode the outgoing service filed read its own project; the incoming
/// one, which asked for a font of its own before it was shared, serves its
/// own project's bytes, never the other's.
#[test]
fn a_decode_of_a_known_type_made_by_a_service_swapped_out_never_lands_in_the_one_swapped_in() {
    let engine = Project::new(&[(FONT, b"outgoing project")]);
    let project = Project::new(&[(FONT, b"incoming project"), ("fonts/title.ttf", b"title")]);
    let mut world = World::new();
    project
        .svc()
        .request::<Font>(&AssetUUID::new_v5("fonts/title.ttf"));
    assert_eq!(answer(&engine.svc().request::<Font>(&font())), "Pending");
    let in_flight = engine.decode(&engine.filed());

    // As `ProjectVfs::share_assets` moves the project's service in.
    {
        let mut own = project.svc();
        let mut target = engine.svc();
        std::mem::swap(&mut *own, &mut *target);
        target.announce_replacing(&own);
        target.adopt_decode_requests(&mut own);
    }
    engine.land(in_flight);
    engine.intake(&mut world);
    for _ in 0..2 {
        if answer(&engine.svc().request::<Font>(&font())) == "Pending" {
            engine.background();
            engine.intake(&mut world);
        }
    }

    assert_eq!(
        font_data(engine.svc().request::<Font>(&font())),
        Some(b"incoming project".to_vec()),
        "the service swapped in serves its own project's font"
    );
}

/// **A request filed by the outgoing service and decoded by the incoming one
/// holds to the incoming service's changes.** The outgoing service saw more
/// changes than the incoming one; its request, still waiting when the editor
/// swaps a project's service in, is decoded from the incoming project's
/// files. That file then changes and is announced: the decode read the
/// bytes before the change, so it is never served.
#[test]
fn a_request_filed_before_a_swap_and_decoded_after_it_holds_to_the_incoming_changes() {
    let engine = Project::new(&[(FONT, b"outgoing project"), ("fonts/other.ttf", b"o")]);
    let project = Project::new(&[(FONT, b"incoming v1"), ("fonts/title.ttf", b"title")]);
    let mut world = World::new();
    // The outgoing service has seen a few changes; the incoming one, none.
    for _ in 0..4 {
        engine
            .svc()
            .invalidate(&AssetUUID::new_v5("fonts/other.ttf"));
    }
    project
        .svc()
        .request::<Font>(&AssetUUID::new_v5("fonts/title.ttf"));
    project.filed();
    assert_eq!(answer(&engine.svc().request::<Font>(&font())), "Pending");
    let waiting = engine.filed();

    // As `ProjectVfs::share_assets` moves the project's service in.
    {
        let mut own = project.svc();
        let mut target = engine.svc();
        std::mem::swap(&mut *own, &mut *target);
        target.announce_replacing(&own);
        target.adopt_decode_requests(&mut own);
    }
    // The `loading` family takes the waiting request now: it decodes from the
    // incoming project.
    let in_flight = engine.decode(&waiting);
    project.write(FONT, b"incoming v2");
    engine.svc().invalidate(&font());
    engine.land(in_flight);
    engine.intake(&mut world);
    for _ in 0..2 {
        if answer(&engine.svc().request::<Font>(&font())) == "Pending" {
            engine.background();
            engine.intake(&mut world);
        }
    }

    assert_eq!(
        font_data(engine.svc().request::<Font>(&font())),
        Some(b"incoming v2".to_vec()),
        "the font decoded before it changed is never served"
    );
}

/// **A decode landing after a synchronous load of the same asset keeps the
/// handle the load cached**: the asset is not decoded into a second copy,
/// so whoever holds the loaded handle holds the one the engine serves.
#[test]
fn a_decode_landing_after_a_synchronous_load_keeps_the_loaded_handle() {
    let project = Project::new(&[(FONT, FONT_BYTES)]);
    let mut world = World::new();
    assert_eq!(answer(&project.svc().request::<Font>(&font())), "Pending");
    let loaded = project
        .svc()
        .load::<Font>(&font())
        .expect("the load answers");
    let loads = project.svc().load_count();

    project.background();
    project.intake(&mut world);

    match project.svc().request::<Font>(&font()) {
        Requested::Ready(served) => assert!(served == loaded, "the handle the load cached"),
        other => panic!("ready, not {}", answer(&other)),
    }
    assert_eq!(
        project.svc().load_count(),
        loads,
        "no second copy was installed"
    );
}

// ─── Fixtures ───────────────────────────────────────────────────────────────

/// The buffer of [`triangle_gltf_with_buffer`]'s triangle, half the size:
/// its second corner at x = 0.5.
fn half_triangle_bin() -> Vec<u8> {
    let corners: [[f32; 3]; 3] = [[0.0, 0.0, 0.0], [0.5, 0.0, 0.0], [0.0, 0.5, 0.0]];
    corners
        .iter()
        .flatten()
        .flat_map(|value| value.to_le_bytes())
        .collect()
}

/// A glTF holding one triangle whose positions sit in the buffer file
/// `uri` beside it: the glTF's JSON and the buffer's bytes.
fn triangle_gltf_with_buffer(uri: &str) -> (Vec<u8>, Vec<u8>) {
    let corners: [[f32; 3]; 3] = [[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]];
    let bin: Vec<u8> = corners
        .iter()
        .flatten()
        .flat_map(|value| value.to_le_bytes())
        .collect();
    let json = format!(
        r#"{{
            "asset": {{ "version": "2.0" }},
            "buffers": [ {{ "uri": "{uri}", "byteLength": {len} }} ],
            "bufferViews": [ {{ "buffer": 0, "byteOffset": 0, "byteLength": {len} }} ],
            "accessors": [ {{
                "bufferView": 0, "componentType": 5126, "count": 3, "type": "VEC3",
                "min": [0.0, 0.0, 0.0], "max": [1.0, 1.0, 0.0]
            }} ],
            "meshes": [ {{ "primitives": [ {{ "attributes": {{ "POSITION": 0 }} }} ] }} ],
            "nodes": [ {{ "mesh": 0 }} ],
            "scenes": [ {{ "nodes": [0] }} ],
            "scene": 0
        }}"#,
        len = bin.len()
    );
    (json.into_bytes(), bin)
}
