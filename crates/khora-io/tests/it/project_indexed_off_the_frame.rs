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

//! **A project's index filed by a host is built off the frame and lands at a
//! frame's boundary.** `IndexHub::file` answers at once with a ticket; the
//! `loading` family's work is played here by hand, as the scheduler runs it
//! at the background: the requests filed are taken, indexed through the
//! runtime's `AssetIndexing` and staged under their lease into the engine's
//! reserve. The `asset_index_intake` DataSystem then takes them in at the
//! boundary, after `asset_intake`: an `Open` swaps a new asset service into
//! the runtime's own, a `Reindex` reindexes it; each outcome is filed under
//! its ticket.
//!
//! An index filed for a project another one replaced since lands nothing,
//! and its ticket says so. An index build that panics fails its own ticket
//! and only it.

use std::path::Path;
use std::sync::{Arc, Mutex, MutexGuard};

use khora_core::asset::font::Font;
use khora_core::asset::{
    AssetIndexing, AssetUUID, DecodeRequest, IndexKind, IndexRequest, IndexTicket, IndexedAssets,
};
use khora_core::event::Channel;
use khora_core::lane::{OutputDeck, Reserve, Staging};
use khora_core::Runtime;
use khora_data::ecs::{Role, SystemContext, Tick, TickPhase, World};
use khora_data::AssetStore;
use khora_io::asset::{
    install_decoding, install_indexing, AssetIdRegistry, AssetIndex, AssetService, IndexHub,
    IndexOutcome, Requested, SubAssetEntry, SubAssetListerRegistration,
};
use khora_telemetry::MetricsRegistry;

use super::assets_requested_off_the_frame::data_system;

/// How many frames [`Engine::settle`] runs before it calls the index stuck.
const SETTLE_FRAMES: usize = 8;

// ─── A lister that panics ───────────────────────────────────────────────────

/// The extension of a file whose sub-asset lister panics, as a broken
/// container reader may: indexing a project holding one runs it.
const PANICKING_EXTENSION: &str = "khorapanics";

fn panicking_lister(_bytes: &[u8]) -> anyhow::Result<Vec<SubAssetEntry>> {
    panic!("the sub-asset lister of a broken file panicked");
}

inventory::submit! {
    SubAssetListerRegistration {
        type_name: PANICKING_EXTENSION,
        list: panicking_lister,
    }
}

// ─── The harness ────────────────────────────────────────────────────────────

/// A project on disk: `files`, each a forward-slash path under
/// `<root>/assets/` and its bytes.
fn project(files: &[(&str, &[u8])]) -> tempfile::TempDir {
    let dir = tempfile::tempdir().expect("a temporary project");
    std::fs::create_dir_all(dir.path().join("assets")).expect("the assets folder");
    for (rel, bytes) in files {
        write_asset(dir.path(), rel, bytes);
    }
    dir
}

fn write_asset(root: &Path, rel: &str, bytes: &[u8]) {
    let path = root.join("assets").join(rel);
    if let Some(folder) = path.parent() {
        std::fs::create_dir_all(folder).expect("the asset's folder");
    }
    std::fs::write(path, bytes).expect("the asset is written");
}

/// The asset service a host opens over the project at `root`.
fn opened(root: &Path) -> AssetService {
    AssetService::open_loose_files(
        root,
        &AssetIdRegistry::load(root),
        Arc::new(MetricsRegistry::new()),
    )
    .expect("an asset service over the project")
}

fn uuid(rel: &str) -> AssetUUID {
    AssetUUID::new_v5(rel)
}

/// A runtime wired as the engine wires project indexes: the reserve the
/// `loading` family stages into and what `install_indexing` installs — and,
/// when it serves one, the asset service, the store and what
/// `install_decoding` installs.
struct Engine {
    world: World,
    runtime: Runtime,
    reserve: Reserve,
}

impl Engine {
    /// An engine serving no asset service.
    fn new() -> Self {
        let reserve = Reserve::new();
        let mut runtime = Runtime::new();
        runtime.resources.insert(reserve.clone());
        install_indexing(&mut runtime);
        Self {
            world: World::new(),
            runtime,
            reserve,
        }
    }

    /// An engine serving `service`, as the engine holds one: the decode hub
    /// attached to it.
    fn serving(service: AssetService) -> Self {
        let reserve = Reserve::new();
        let mut runtime = Runtime::new();
        runtime.resources.insert(reserve.clone());
        runtime.resources.insert(AssetStore::new());
        runtime.services.insert(Arc::new(Mutex::new(service)));
        install_decoding(&mut runtime);
        install_indexing(&mut runtime);
        Self {
            world: World::new(),
            runtime,
            reserve,
        }
    }

    /// The runtime's index hub, where a host files its indexes.
    fn hub(&self) -> Arc<IndexHub> {
        self.runtime
            .resources
            .get::<Arc<IndexHub>>()
            .cloned()
            .expect("`install_indexing` installs the hub as `Arc<IndexHub>`")
    }

    /// The channel the `loading` family takes the index requests from.
    fn requests(&self) -> Channel<IndexRequest> {
        self.runtime
            .resources
            .get::<Channel<IndexRequest>>()
            .cloned()
            .expect("`install_indexing` installs the index request channel")
    }

    /// The runtime's indexer, which the `loading` family's lanes read.
    fn indexing(&self) -> Arc<dyn AssetIndexing> {
        self.runtime
            .resources
            .get::<Arc<dyn AssetIndexing>>()
            .cloned()
            .expect("`install_indexing` installs the `AssetIndexing`")
    }

    /// The channel the `loading` family takes the decode requests from.
    fn decode_requests(&self) -> Channel<DecodeRequest> {
        self.runtime
            .resources
            .get::<Channel<DecodeRequest>>()
            .cloned()
            .expect("`install_decoding` installs the decode request channel")
    }

    /// The runtime's asset service.
    fn service(&self) -> Arc<Mutex<AssetService>> {
        self.runtime
            .services
            .get::<Arc<Mutex<AssetService>>>()
            .cloned()
            .expect("the runtime holds an asset service")
    }

    /// One run of the `loading` family over the indexes: the requests
    /// filed, taken, indexed through the runtime's `AssetIndexing`, each
    /// staged under its lease — not yet received. Returns the staging and
    /// how many requests it took.
    fn index_filed(&self) -> (Staging, usize) {
        let filed = self.requests().drain();
        let indexing = self.indexing();
        let taken = filed.len();
        let mut staging = Staging::new();
        for request in filed {
            let (indexed, lease) = indexing.index_leased(&request);
            match lease {
                Some(lease) => staging.stage_while(vec![indexed], lease),
                None => staging.stage(vec![indexed]),
            }
        }
        (staging, taken)
    }

    /// `staging` received by the engine's reserve, as a frame start does.
    fn land(&self, staging: Staging) {
        self.reserve.receive(staging);
    }

    /// The boundary's `asset_intake`, then its `asset_index_intake`.
    fn boundary(&mut self) {
        for name in ["asset_intake", "asset_index_intake"] {
            (data_system(name).run)(
                &mut self.world,
                &self.runtime,
                &mut OutputDeck::new(),
                SystemContext {
                    since: Tick(0),
                    resumed: false,
                    scope: None,
                },
            );
        }
    }

    /// Frames until the index filed as `ticket` landed: each frame takes in,
    /// at its boundary, what the family staged, then the family indexes
    /// what was filed. Its outcome; panics when it has not landed after
    /// [`SETTLE_FRAMES`] frames.
    fn settle(&mut self, ticket: IndexTicket) -> IndexOutcome {
        let hub = self.hub();
        for _ in 0..SETTLE_FRAMES {
            self.boundary();
            if let Some(outcome) = hub.take_outcome(ticket) {
                return outcome;
            }
            let (staging, _) = self.index_filed();
            self.land(staging);
        }
        panic!(
            "index {} did not land after {SETTLE_FRAMES} frames",
            ticket.0
        );
    }
}

fn locked(service: &Arc<Mutex<AssetService>>) -> MutexGuard<'_, AssetService> {
    service.lock().expect("the service's lock")
}

// ─── Off the frame ──────────────────────────────────────────────────────────

/// **An index request is built off the frame**, by the hub's own
/// `AssetIndexing`. A project holds two files, one of them renamed — the
/// registry under its `.khora/` froze the identity it had. Filed, the index
/// is pending and on the family's channel; indexed, its bytes list both
/// files, the renamed one under its frozen identity, and its lease holds:
/// nothing replaced the project.
#[test]
fn an_index_request_is_built_off_the_frame() {
    let dir = project(&[("textures/a.png", b"a"), ("textures/b.png", b"b")]);
    let frozen = uuid("textures/old_name.png");
    let mut registry = AssetIdRegistry::load(dir.path());
    registry.freeze("textures/b.png", frozen);
    registry.save().expect("the registry is saved");
    let engine = Engine::new();
    let hub = engine.hub();

    let ticket = hub.file(dir.path(), IndexKind::Reindex);

    assert!(hub.is_pending(ticket), "in flight once filed");
    assert!(
        hub.take_outcome(ticket).is_none(),
        "no outcome before it lands"
    );
    let filed = engine.requests().drain();
    assert_eq!(
        filed,
        vec![IndexRequest {
            ticket,
            project_root: dir.path().to_path_buf(),
            kind: IndexKind::Reindex,
        }],
        "one request on the family's channel"
    );

    let (indexed, lease) = engine.indexing().index_leased(&filed[0]);

    assert_eq!(indexed.ticket, ticket);
    assert_eq!(indexed.kind, IndexKind::Reindex);
    assert_eq!(indexed.project_root, dir.path());
    let bytes = indexed.index.expect("the project indexes");
    let index = AssetIndex::new(&bytes).expect("the bytes are an asset index");
    assert_eq!(index.asset_count(), 2, "both files, and nothing else");
    assert!(
        index.get_metadata(&uuid("textures/a.png")).is_some(),
        "the file never renamed is named by its path"
    );
    assert!(
        index.get_metadata(&frozen).is_some(),
        "the renamed file keeps the identity the registry froze"
    );
    assert!(
        index.get_metadata(&uuid("textures/b.png")).is_none(),
        "the renamed file is not named by its new path"
    );
    assert!(
        lease.is_some_and(|lease| lease.holds()),
        "nothing replaced the project: its index holds"
    );
}

/// **An index filed for a project replaced since lands nothing.** A reindex
/// of A is filed, then B is opened: A's index is built, but its lease no
/// longer holds once B was filed — the frame start abandons it, and its
/// ticket lands an error at once, saying it was superseded. B's index
/// lands in the reserve, its ticket still waiting for the boundary.
#[test]
fn a_project_replaced_since_filing_lands_nothing() {
    let a = project(&[("a.png", b"a")]);
    let b = project(&[("b.png", b"b")]);
    let engine = Engine::new();
    let hub = engine.hub();

    let reindex_a = hub.file(a.path(), IndexKind::Reindex);
    let open_b = hub.file(b.path(), IndexKind::Open);
    let (staging, taken) = engine.index_filed();
    engine.land(staging);

    assert_eq!(taken, 2, "both were filed");
    let landed: Vec<IndexTicket> = engine
        .reserve
        .take::<IndexedAssets>()
        .iter()
        .map(|indexed| indexed.ticket)
        .collect();
    assert_eq!(landed, vec![open_b], "only B's index reaches the reserve");
    match hub.take_outcome(reindex_a) {
        Some(Err(message)) => assert!(
            message.contains("superseded"),
            "A's ticket says it was superseded: {message}"
        ),
        other => panic!("A's ticket lands an error at once, not {other:?}"),
    }
    assert!(
        !hub.is_pending(reindex_a),
        "A's ticket is no longer in flight"
    );
    assert!(hub.is_pending(open_b), "B's waits for the boundary");
}

/// **A reindex filed after an open leaves the open standing**: only an
/// `Open` replaces the project. A is opened, then reindexed: both indexes
/// hold, and both reach the reserve, in the order they were filed.
#[test]
fn a_reindex_filed_after_an_open_leaves_the_open_standing() {
    let a = project(&[("a.png", b"a")]);
    let engine = Engine::new();
    let hub = engine.hub();

    let open = hub.file(a.path(), IndexKind::Open);
    let reindex = hub.file(a.path(), IndexKind::Reindex);
    let (staging, _) = engine.index_filed();
    engine.land(staging);

    let landed: Vec<IndexTicket> = engine
        .reserve
        .take::<IndexedAssets>()
        .iter()
        .map(|indexed| indexed.ticket)
        .collect();
    assert_eq!(landed, vec![open, reindex], "both land, in filing order");
    assert!(hub.is_pending(open) && hub.is_pending(reindex));
}

/// **A refused index request lands at once.** Sixty-four indexes wait to be
/// built; a sixty-fifth is refused by the full channel: `file` still
/// returns a ticket, whose outcome is an error at once — never a ticket
/// pending forever. The ones waiting stay pending.
#[test]
fn a_refused_index_request_lands_at_once() {
    let dir = project(&[]);
    let engine = Engine::new();
    let hub = engine.hub();

    let waiting: Vec<IndexTicket> = (0..64)
        .map(|_| hub.file(dir.path(), IndexKind::Reindex))
        .collect();
    let refused = hub.file(dir.path(), IndexKind::Reindex);

    assert_eq!(engine.requests().len(), 64, "the channel is full");
    assert!(
        !waiting.contains(&refused),
        "the refused request has a ticket of its own"
    );
    assert!(
        waiting.iter().all(|ticket| hub.is_pending(*ticket)),
        "the ones waiting stay pending"
    );
    assert!(!hub.is_pending(refused), "the refused one is not in flight");
    assert!(
        matches!(hub.take_outcome(refused), Some(Err(_))),
        "its outcome is an error at once"
    );
}

/// **An index build that panics fails its own ticket, and only it.** A
/// project holding a file whose lister panics is reindexed, then the
/// project the engine serves — which gained a file. The panic stays within
/// its own index: the indexer returns, the broken ticket lands an error
/// naming the panic, and the sound one lands and indexes the new file.
#[test]
fn a_panicking_index_build_fails_its_ticket_alone() {
    let broken = project(&[(&format!("boom.{PANICKING_EXTENSION}"), b"broken")]);
    let sound = project(&[("textures/a.png", b"a")]);
    let mut engine = Engine::serving(opened(sound.path()));
    write_asset(sound.path(), "textures/c.png", b"c");
    let hub = engine.hub();

    let failing = hub.file(broken.path(), IndexKind::Reindex);
    let landing = hub.file(sound.path(), IndexKind::Reindex);
    let indexed = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let (staging, _) = engine.index_filed();
        engine.land(staging);
    }));
    assert!(indexed.is_ok(), "the lister's panic stays within its index");

    let failed = engine.settle(failing);
    match failed {
        Err(message) => assert!(
            message.contains("panicked"),
            "the broken ticket names the panic: {message}"
        ),
        Ok(()) => panic!("the broken project's ticket fails"),
    }
    assert_eq!(engine.settle(landing), Ok(()), "the sound one lands");
    assert!(
        locked(&engine.service())
            .index()
            .get_metadata(&uuid("textures/c.png"))
            .is_some(),
        "the sound reindex landed in the service"
    );
}

// ─── At the boundary ────────────────────────────────────────────────────────

/// **An open lands a new service in the runtime.** The engine serves A, and
/// a decode A filed is out. B is opened: once its ticket lands `Ok`, the
/// runtime's own service — the one every holder of it reads — serves B's
/// assets and no longer A's; and the decode request A had filed is filed
/// again by the new service, on the channel the `loading` family takes
/// from (the service moving in adopted it).
#[test]
fn an_open_lands_a_new_service_in_the_runtime() {
    let a = project(&[("textures/a.png", b"a"), ("shared.png", b"s")]);
    let b = project(&[("textures/b.png", b"b"), ("shared.png", b"s")]);
    let mut engine = Engine::serving(opened(a.path()));
    let shared = engine.service();
    assert!(
        matches!(
            locked(&shared).request::<Font>(&uuid("shared.png")),
            Requested::Pending
        ),
        "the premise: A files a decode"
    );
    let decodes = engine.decode_requests();
    assert_eq!(
        decodes.drain().len(),
        1,
        "the premise: A filed on the family's channel, and the family took it"
    );

    let ticket = engine.hub().file(b.path(), IndexKind::Open);
    assert_eq!(engine.settle(ticket), Ok(()), "the open lands");

    assert!(
        Arc::ptr_eq(&engine.service(), &shared),
        "swapped into the runtime's own service, not beside it"
    );
    {
        let service = locked(&shared);
        assert!(
            service
                .index()
                .get_metadata(&uuid("textures/b.png"))
                .is_some(),
            "the runtime's service serves B's assets"
        );
        assert!(
            service
                .index()
                .get_metadata(&uuid("textures/a.png"))
                .is_none(),
            "and no longer A's"
        );
    }
    assert!(matches!(
        locked(&shared).request::<Font>(&uuid("shared.png")),
        Requested::Pending
    ));
    let refiled: Vec<AssetUUID> = decodes.drain().iter().map(|request| request.uuid).collect();
    assert_eq!(
        refiled,
        vec![uuid("shared.png")],
        "the earlier request is filed again, on the family's channel"
    );
}

/// **A reindex lands in the runtime's service.** The engine serves a
/// project; a file is added to it and a reindex filed: once its ticket
/// lands `Ok`, the runtime's same service indexes the new file.
#[test]
fn a_reindex_lands_in_the_runtime_service() {
    let dir = project(&[("textures/a.png", b"a")]);
    let mut engine = Engine::serving(opened(dir.path()));
    let shared = engine.service();
    write_asset(dir.path(), "textures/c.png", b"c");
    assert!(
        locked(&shared)
            .index()
            .get_metadata(&uuid("textures/c.png"))
            .is_none(),
        "the premise: the new file is not indexed yet"
    );

    let ticket = engine.hub().file(dir.path(), IndexKind::Reindex);
    assert_eq!(engine.settle(ticket), Ok(()), "the reindex lands");

    assert!(Arc::ptr_eq(&engine.service(), &shared), "the same service");
    assert!(
        locked(&shared)
            .index()
            .get_metadata(&uuid("textures/c.png"))
            .is_some(),
        "the service indexes the new file"
    );
    assert!(
        locked(&shared)
            .index()
            .get_metadata(&uuid("textures/a.png"))
            .is_some(),
        "and still the old one"
    );
}

/// **An open replaced after its index was received lands nothing.** The
/// engine serves Z. A is opened, indexed, and received by a frame start —
/// nothing had replaced it yet, so its lease held. Within that frame B is
/// opened: A's ticket lands "superseded" at once. At the boundary A's index
/// must not be swapped into the runtime's service, as its ticket says: the
/// service still serves Z until B's index lands.
#[test]
fn an_open_superseded_after_its_index_was_received_lands_nothing() {
    let z = project(&[("z.png", b"z")]);
    let a = project(&[("a.png", b"a")]);
    let b = project(&[("b.png", b"b")]);
    let mut engine = Engine::serving(opened(z.path()));
    let hub = engine.hub();

    let open_a = hub.file(a.path(), IndexKind::Open);
    let (staging, _) = engine.index_filed();
    engine.land(staging);
    let _open_b = hub.file(b.path(), IndexKind::Open);
    match hub.take_outcome(open_a) {
        Some(Err(message)) => assert!(message.contains("superseded"), "{message}"),
        other => panic!("the premise: A's ticket lands superseded at once, not {other:?}"),
    }

    engine.boundary();

    let service = engine.service();
    let service = locked(&service);
    assert!(
        service.index().get_metadata(&uuid("a.png")).is_none(),
        "A's open, superseded, was swapped into the runtime's service"
    );
    assert!(
        service.index().get_metadata(&uuid("z.png")).is_some(),
        "the runtime's service still serves Z until B's index lands"
    );
}

/// **The index intake runs at the boundary, after the asset intake**: the
/// decodes read for the service serving now are installed in it before an
/// open swaps it out. It runs whatever families are on.
#[test]
fn the_intake_runs_after_the_asset_intake() {
    let intake = data_system("asset_index_intake");

    assert_eq!(intake.phase, TickPhase::Boundary);
    assert_eq!(intake.role, Role::Upkeep);
    assert!(
        intake.runs_after.contains(&"asset_intake"),
        "after `asset_intake`: {:?}",
        intake.runs_after
    );
}
