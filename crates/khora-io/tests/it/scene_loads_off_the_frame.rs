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

//! **A scene load filed by a host is prepared off the frame and lands at a
//! frame's boundary.** `SceneLoadHub::file` answers at once with a ticket;
//! the `scene_io` family's work is played here by hand, as the scheduler
//! runs it at the background: the requests filed are taken, prepared
//! through the runtime's `SceneLoading` and staged under their lease into
//! the engine's reserve. The `scene_intake` DataSystem then commits them at
//! the boundary, in the order they were filed, and files each outcome under
//! its ticket.
//!
//! A load whose world changed its components while it was in flight is
//! abandoned when the reserve receives it, filed again, and prepared
//! against the world as it is now. A load that cannot be prepared changes
//! nothing and gives back every id it held.

use std::collections::{BTreeSet, HashSet};
use std::sync::Arc;

use khora_core::ecs::entity::EntityId;
use khora_core::event::Channel;
use khora_core::lane::{OutputDeck, Reserve, Staging};
use khora_core::scene::{SceneFile, SerializationGoal};
use khora_core::script::ScriptValue;
use khora_core::Runtime;
use khora_data::ecs::{
    Camera, ComponentProvenance, FieldKind, GlobalTransform, Name, PackedField, PackedLayout,
    RuntimeComponentDecl, SemanticDomain, SystemContext, Tick, Transform, World,
};
use khora_data::scene::loading::{
    LoadKind, LoadTicket, PreparedLoad, SceneLoadRequest, SceneLoading, SceneSource,
};
use khora_data::scene::NoPrefabs;
use khora_io::scene_loads::{install_scene_loading, LoadOutcome, SceneLoadHub};
use khora_io::serialization::SerializationService;

use super::assets_requested_off_the_frame::data_system;

/// How many frames [`Engine::settle`] runs before it calls the loads stuck.
const SETTLE_FRAMES: usize = 8;

// ─── The harness ────────────────────────────────────────────────────────────

/// A world, and a runtime wired as the engine wires the scene loads: the
/// reserve the `scene_io` family stages into, and what
/// `install_scene_loading` installs.
struct Engine {
    world: World,
    runtime: Runtime,
    reserve: Reserve,
}

impl Engine {
    fn new() -> Self {
        let reserve = Reserve::new();
        let mut runtime = Runtime::new();
        runtime.resources.insert(reserve.clone());
        install_scene_loading(&mut runtime);
        Self {
            world: World::new(),
            runtime,
            reserve,
        }
    }

    /// The runtime's scene load hub, where a host files its loads.
    fn hub(&self) -> Arc<SceneLoadHub> {
        self.runtime
            .resources
            .get::<Arc<SceneLoadHub>>()
            .cloned()
            .expect("`install_scene_loading` installs the hub as `Arc<SceneLoadHub>`")
    }

    /// The channel the `scene_io` family takes its requests from.
    fn requests(&self) -> Channel<SceneLoadRequest> {
        self.runtime
            .resources
            .get::<Channel<SceneLoadRequest>>()
            .cloned()
            .expect("`install_scene_loading` installs the request channel")
    }

    /// Files `kind` into the world, with no prefabs, as a host does.
    fn file(&mut self, kind: LoadKind) -> LoadTicket {
        let hub = self.hub();
        hub.file(&mut self.world, kind, Arc::new(NoPrefabs))
    }

    /// One run of the `scene_io` family: the requests filed, taken,
    /// prepared through the runtime's `SceneLoading`, each staged under its
    /// lease — not yet received. Returns the staging and how many requests
    /// it took.
    fn prepare_filed(&self) -> (Staging, usize) {
        let filed = self.requests().drain();
        let loading = self
            .runtime
            .resources
            .get::<Arc<dyn SceneLoading>>()
            .cloned()
            .expect("`install_scene_loading` installs the `SceneLoading`");
        let taken = filed.len();
        let mut staging = Staging::new();
        for request in filed {
            let (prepared, lease) = loading.prepare_leased(request);
            match lease {
                Some(lease) => staging.stage_while(vec![prepared], lease),
                None => staging.stage(vec![prepared]),
            }
        }
        (staging, taken)
    }

    /// `staging` received by the engine's reserve, as a frame start does.
    fn land(&self, staging: Staging) {
        self.reserve.receive(staging);
    }

    /// The boundary's `scene_intake`.
    fn boundary(&mut self) {
        (data_system("scene_intake").run)(
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

    /// Frames until every load filed has landed: each frame commits, at its
    /// boundary, what the family staged, then the family prepares what was
    /// filed, then the frame ends. Panics when loads are still filed or
    /// waiting after [`SETTLE_FRAMES`] frames.
    fn settle(&mut self) {
        for _ in 0..SETTLE_FRAMES {
            self.boundary();
            let (staging, taken) = self.prepare_filed();
            self.land(staging);
            self.world.release_unspent();
            if taken == 0 && self.reserve.is_empty::<PreparedLoad>() {
                return;
            }
        }
        panic!("scene loads are still in flight after {SETTLE_FRAMES} frames");
    }

    /// The names of the world's entities.
    fn names(&self) -> BTreeSet<String> {
        self.world
            .iter_entities()
            .filter_map(|entity| self.world.get::<Name>(entity))
            .map(|name| name.as_str().to_owned())
            .collect()
    }

    fn named(&self, name: &str) -> Option<EntityId> {
        self.world.iter_entities().find(|entity| {
            self.world
                .get::<Name>(*entity)
                .is_some_and(|n| n.as_str() == name)
        })
    }

    /// Spawns an authored entity called `name`.
    fn spawn_named(&mut self, name: &str) -> EntityId {
        let entity = self
            .world
            .spawn((Transform::identity(), Name::new(name.to_owned())));
        self.world.mark_authored(entity).expect("authored");
        entity
    }
}

/// A scene file of authored entities, one per name.
fn scene_of(names: &[&str]) -> SceneFile {
    let mut src = World::new();
    for name in names {
        let entity = src.spawn((Transform::identity(), Name::new((*name).to_owned())));
        src.mark_authored(entity).expect("authored");
    }
    save(&src, SerializationGoal::EditorInterchange)
}

fn save(world: &World, goal: SerializationGoal) -> SceneFile {
    SerializationService::new()
        .save_world(world, goal)
        .unwrap_or_else(|error| panic!("the scene saves: {error}"))
}

fn bytes(file: &SceneFile) -> SceneSource {
    SceneSource::Bytes(Arc::from(file.to_bytes()))
}

/// A scene whose camera's near plane sits at its eye — a value the camera
/// cannot use, which a load reports — beside a hero.
fn scene_with_a_report() -> SceneFile {
    let mut src = World::new();
    let camera = src.spawn((
        Transform::identity(),
        GlobalTransform::identity(),
        Camera {
            z_near: 0.0,
            ..Camera::default_perspective()
        },
        Name::new("Main Camera"),
    ));
    let hero = src.spawn((Transform::identity(), Name::new("Hero")));
    for entity in [camera, hero] {
        src.mark_authored(entity).expect("authored");
    }
    save(&src, SerializationGoal::EditorInterchange)
}

fn names(list: &[&str]) -> BTreeSet<String> {
    list.iter().map(|name| (*name).to_owned()).collect()
}

/// The value of `entity`'s declared `component` field `name`.
fn field_of(world: &World, entity: EntityId, component: &str, name: &str) -> Option<ScriptValue> {
    let key = world.components().key_named(component)?;
    let slot = world
        .components()
        .vtable(key)?
        .columns
        .packed()?
        .slot_of(name)?;
    world.row(entity, key)?.field(slot)
}

fn int_field(name: &str, default: i64) -> PackedField {
    PackedField {
        name: name.to_owned(),
        kind: FieldKind::Int,
        default: ScriptValue::Int(default),
    }
}

fn declare_health(world: &mut World, fields: Vec<PackedField>) {
    world
        .register_runtime_component(RuntimeComponentDecl {
            name: "Health".to_owned(),
            domain: SemanticDomain::Script,
            provenance: ComponentProvenance::Authored,
            resumable: false,
            layout: PackedLayout::new(fields).expect("a valid layout"),
        })
        .expect("`Health` registers");
}

fn assert_ok(outcome: Option<LoadOutcome>, what: &str) -> khora_data::scene::record::LoadReport {
    match outcome {
        Some(Ok(report)) => report,
        Some(Err(error)) => panic!("{what}: the load failed: {error}"),
        None => panic!("{what}: no outcome under its ticket"),
    }
}

// ─── Landing ────────────────────────────────────────────────────────────────

/// **A filed replace lands at the boundary, with its report.** Prepared
/// and received, the load has changed nothing before the boundary — its
/// ticket pending, no outcome yet. At the boundary the scene replaces the
/// world, and its outcome is the report the synchronous replace gives,
/// once: taken, it is gone.
#[test]
fn a_filed_replace_lands_at_the_boundary_with_its_report() {
    let file = scene_with_a_report();
    let expected = SerializationService::new()
        .replace_world(&file, &mut World::new())
        .unwrap_or_else(|error| panic!("the synchronous replace: {error}"));
    assert!(
        !expected.is_clean(),
        "the premise: the scene reports its camera"
    );
    let mut engine = Engine::new();
    engine.spawn_named("Old");
    let hub = engine.hub();

    let ticket = engine.file(LoadKind::Replace(bytes(&file)));
    let (staging, taken) = engine.prepare_filed();
    engine.land(staging);

    assert_eq!(taken, 1, "one request filed");
    assert_eq!(
        engine.names(),
        names(&["Old"]),
        "nothing lands before the boundary"
    );
    assert!(hub.is_pending(ticket), "in flight until the boundary");
    assert!(
        hub.take_outcome(ticket).is_none(),
        "no outcome before the boundary"
    );

    engine.boundary();

    assert_eq!(engine.names(), names(&["Main Camera", "Hero"]));
    assert!(!hub.is_pending(ticket), "landed");
    let report = assert_ok(hub.take_outcome(ticket), "the replace");
    assert_eq!(report, expected, "the report the synchronous replace gives");
    assert!(
        hub.take_outcome(ticket).is_none(),
        "an outcome is taken once"
    );
}

/// **A filed add keeps what was there**: the scene lands beside the world's
/// entities, which stay as they were.
#[test]
fn a_filed_add_keeps_what_was_there() {
    let mut engine = Engine::new();
    let old = engine.spawn_named("Old");
    let hub = engine.hub();

    let ticket = engine.file(LoadKind::Add(bytes(&scene_of(&["Hero", "Door"]))));
    engine.settle();

    assert_ok(hub.take_outcome(ticket), "the add");
    assert_eq!(engine.names(), names(&["Old", "Hero", "Door"]));
    assert_eq!(
        engine.named("Old"),
        Some(old),
        "the entity that was there stays"
    );
}

/// **A load that fails to prepare leaves the world as it was.** Bytes that
/// are no scene: the outcome is an error, every entity is where it was, and
/// the slots the world had freed — held for the load as it was filed — are
/// handed to the next spawns.
#[test]
fn a_load_that_fails_to_prepare_leaves_the_world_as_it_was() {
    let mut engine = Engine::new();
    let spawned: Vec<EntityId> = (0..6)
        .map(|i| engine.spawn_named(&format!("Kept {i}")))
        .collect();
    let freed: Vec<EntityId> = spawned.iter().copied().step_by(2).collect();
    for entity in &freed {
        assert!(engine.world.despawn(*entity));
    }
    let before: BTreeSet<String> = engine.names();
    let alive: Vec<EntityId> = engine.world.iter_entities().collect();
    let hub = engine.hub();

    let ticket = engine.file(LoadKind::Replace(SceneSource::Bytes(Arc::from(
        &b"not a scene"[..],
    ))));
    engine.settle();

    match hub.take_outcome(ticket) {
        Some(Err(_)) => {}
        Some(Ok(report)) => panic!("bytes that are no scene loaded: {report:?}"),
        None => panic!("no outcome under the failed load's ticket"),
    }
    assert_eq!(engine.names(), before, "the world is as it was");
    assert_eq!(engine.world.iter_entities().collect::<Vec<_>>(), alive);
    let respawned: HashSet<u32> = (0..freed.len())
        .map(|_| engine.world.spawn(Transform::identity()).index)
        .collect();
    assert_eq!(
        respawned,
        freed
            .iter()
            .map(|entity| entity.index)
            .collect::<HashSet<u32>>(),
        "the failed load gave the freed slots back"
    );
}

/// **A declaration change in flight prepares the load again.** A scene
/// holding a declared `Health` is prepared; before the frame start
/// receives it, the world declares `Health` with one more field. The
/// prepared load is abandoned and its request filed again; it then lands
/// against the world's declaration — the new field at its default — with
/// one outcome under its ticket.
#[test]
fn a_declaration_change_in_flight_prepares_the_load_again() {
    let file = {
        let mut src = World::new();
        declare_health(&mut src, vec![int_field("current", 100)]);
        let hero = src.spawn((Transform::identity(), Name::new("Hero")));
        let key = src.components().key_named("Health").expect("declared");
        src.add_runtime_component(
            hero,
            key,
            &ScriptValue::Struct(vec![("current".to_owned(), ScriptValue::Int(42))]),
        )
        .expect("`Health` attaches");
        src.mark_authored(hero).expect("authored");
        save(&src, SerializationGoal::EditorInterchange)
    };
    let mut engine = Engine::new();
    let hub = engine.hub();

    let ticket = engine.file(LoadKind::Replace(bytes(&file)));
    let (staging, _) = engine.prepare_filed();
    declare_health(
        &mut engine.world,
        vec![int_field("current", 100), int_field("max", 250)],
    );
    engine.land(staging);

    assert!(
        engine.reserve.is_empty::<PreparedLoad>(),
        "a load prepared against the old declaration never lands"
    );
    let refiled = engine.requests();
    assert_eq!(refiled.len(), 1, "the load is filed again");
    assert!(hub.is_pending(ticket), "still in flight");

    engine.settle();

    assert_ok(hub.take_outcome(ticket), "the load prepared again");
    assert!(
        hub.take_outcome(ticket).is_none(),
        "one outcome under the ticket"
    );
    let hero = engine.named("Hero").expect("the hero landed");
    assert_eq!(
        field_of(&engine.world, hero, "Health", "current"),
        Some(ScriptValue::Int(42))
    );
    assert_eq!(
        field_of(&engine.world, hero, "Health", "max"),
        Some(ScriptValue::Int(250)),
        "read onto the world's declaration"
    );
}

/// **Loads land in the order they were filed.** Two replaces, A then B,
/// prepared in one run: the world ends with B, and both outcomes are there.
#[test]
fn loads_land_in_the_order_they_were_filed() {
    let mut engine = Engine::new();
    engine.spawn_named("Old");
    let hub = engine.hub();

    let a = engine.file(LoadKind::Replace(bytes(&scene_of(&["A1", "A2"]))));
    let b = engine.file(LoadKind::Replace(bytes(&scene_of(&["B1"]))));
    engine.settle();

    assert_eq!(engine.names(), names(&["B1"]), "B, filed last, lands last");
    assert_ok(hub.take_outcome(a), "A");
    assert_ok(hub.take_outcome(b), "B");
}

/// **Repeated replaces prepared off the frame keep the store bounded** —
/// the editor's Stop, again and again, each restoring the snapshot taken at
/// Play: each load is held the slots the previous one freed, so twenty
/// cycles of 32 entities stay within about two worlds' worth of slots.
#[test]
fn repeated_replaces_prepared_off_the_frame_keep_the_store_bounded() {
    const N: usize = 32;
    let mut engine = Engine::new();
    for i in 0..N {
        engine.spawn_named(&format!("e{i}"));
    }
    let snapshot = save(&engine.world, SerializationGoal::FastestLoad);
    let hub = engine.hub();

    let mut highest = 0;
    for round in 0..20 {
        let ticket = engine.file(LoadKind::Replace(bytes(&snapshot)));
        engine.settle();
        assert_ok(hub.take_outcome(ticket), &format!("round {round}"));
        assert_eq!(engine.world.entity_count(), N, "round {round}");
        highest = engine
            .world
            .iter_entities()
            .map(|entity| entity.index)
            .fold(highest, u32::max);
    }
    assert!(
        (highest as usize) < 2 * N + 24,
        "twenty replaces of {N} entities reached index {highest}: each grew the store"
    );
}

/// A scene holding a hero with a declared `Health` — a component the world
/// loading it may not declare yet, which its commit then registers.
fn scene_declaring_health() -> SceneFile {
    let mut src = World::new();
    declare_health(&mut src, vec![int_field("current", 100)]);
    let hero = src.spawn((Transform::identity(), Name::new("Hero")));
    let key = src.components().key_named("Health").expect("declared");
    src.add_runtime_component(
        hero,
        key,
        &ScriptValue::Struct(vec![("current".to_owned(), ScriptValue::Int(42))]),
    )
    .expect("`Health` attaches");
    src.mark_authored(hero).expect("authored");
    save(&src, SerializationGoal::EditorInterchange)
}

/// **A load prepared again keeps its place among the loads filed.** Three
/// replaces, filed X, A, B: X and A are prepared in one run, B is filed
/// after it. At the boundary X lands and registers the `Health` its file
/// declares, so A — prepared against the components before — is prepared
/// again. It still lands before B, filed after it: the world ends with B.
#[test]
fn a_load_prepared_again_still_lands_before_the_loads_filed_after_it() {
    let mut engine = Engine::new();
    engine.spawn_named("Old");
    let hub = engine.hub();

    let x = engine.file(LoadKind::Replace(bytes(&scene_declaring_health())));
    let a = engine.file(LoadKind::Replace(bytes(&scene_of(&["A"]))));
    let (staging, taken) = engine.prepare_filed();
    engine.land(staging);
    assert_eq!(taken, 2, "the premise: X and A are prepared in one run");
    let b = engine.file(LoadKind::Replace(bytes(&scene_of(&["B"]))));

    engine.settle();

    assert_ok(hub.take_outcome(x), "X");
    assert_ok(hub.take_outcome(a), "A");
    assert_ok(hub.take_outcome(b), "B");
    assert_eq!(
        engine.names(),
        names(&["B"]),
        "B, filed last, lands last — a load prepared again does not jump behind it"
    );
}

/// **A load prepared again is never lost to a full backlog.** A is
/// prepared; then the backlog fills with loads filed after it; then the
/// world declares a component, so A must be prepared again. Its ticket
/// still ends with one outcome, and is no longer pending.
#[test]
fn a_load_prepared_again_lands_even_when_the_backlog_is_full() {
    let mut engine = Engine::new();
    let hub = engine.hub();

    let a = engine.file(LoadKind::Replace(bytes(&scene_of(&["A"]))));
    let (staging, _) = engine.prepare_filed();
    let later: Vec<LoadTicket> = (0..64)
        .map(|i| engine.file(LoadKind::Add(bytes(&scene_of(&[&format!("L{i}")])))))
        .collect();
    for ticket in &later {
        assert!(hub.is_pending(*ticket), "the premise: the backlog takes 64");
    }
    declare_health(&mut engine.world, vec![int_field("current", 100)]);
    engine.land(staging);

    engine.settle();

    assert!(
        !hub.is_pending(a),
        "the load prepared again is still pending, and nothing will ever land it"
    );
    assert!(
        hub.take_outcome(a).is_some(),
        "no outcome under the load prepared again"
    );
}

/// **Repeated replaces prepared again keep the store bounded.** The
/// editor's Stop, again and again, each restore in flight while the world's
/// declarations change — a script declaring a component while the restore is prepared:
/// each is prepared again, and twenty cycles of 64 entities still stay
/// within about two worlds' worth of slots.
#[test]
fn repeated_replaces_prepared_again_keep_the_store_bounded() {
    const N: usize = 64;
    let mut engine = Engine::new();
    for i in 0..N {
        engine.spawn_named(&format!("e{i}"));
    }
    let snapshot = save(&engine.world, SerializationGoal::FastestLoad);
    let hub = engine.hub();

    let mut highest = 0;
    for round in 0..20 {
        let ticket = engine.file(LoadKind::Replace(bytes(&snapshot)));
        let (staging, _) = engine.prepare_filed();
        // A component declared in flight, another each round.
        engine
            .world
            .register_runtime_component(RuntimeComponentDecl {
                name: format!("Mark{round}"),
                domain: SemanticDomain::Script,
                provenance: ComponentProvenance::Authored,
                resumable: false,
                layout: PackedLayout::new(vec![int_field("on", 1)]).expect("a valid layout"),
            })
            .expect("a new component registers");
        engine.land(staging);
        // The frame that received it ends before the family's next run
        // reserves for it: a background run reserves on a worker, after the
        // frame start that filed it again — and a large file is read and
        // parsed before its first reservation.
        engine.world.release_unspent();
        engine.settle();
        assert_ok(hub.take_outcome(ticket), &format!("round {round}"));
        assert_eq!(engine.world.entity_count(), N, "round {round}");
        highest = engine
            .world
            .iter_entities()
            .map(|entity| entity.index)
            .fold(highest, u32::max);
    }
    assert!(
        (highest as usize) < 2 * N + 24,
        "twenty replaces of {N} entities, each prepared again, reached index {highest}"
    );
}

/// **A replace whose components changed after the frame start never
/// empties the world.** The load is received under a lease that still
/// holds; the world then declares a component before the boundary. The
/// boundary leaves every entity in place and files the load again, which
/// lands at a later boundary — one outcome under its ticket.
#[test]
fn a_replace_turned_stale_after_the_frame_start_leaves_the_world_until_it_lands() {
    let mut engine = Engine::new();
    engine.spawn_named("Old");
    let hub = engine.hub();

    let ticket = engine.file(LoadKind::Replace(bytes(&scene_of(&["A"]))));
    let (staging, _) = engine.prepare_filed();
    engine.land(staging);
    assert_eq!(
        engine.reserve.len::<PreparedLoad>(),
        1,
        "the premise: received under a lease that holds"
    );
    declare_health(&mut engine.world, vec![int_field("current", 1)]);
    engine.boundary();

    assert_eq!(engine.names(), names(&["Old"]), "the world is as it was");
    assert!(hub.is_pending(ticket), "filed again, still in flight");
    assert_eq!(engine.requests().len(), 1, "the load is filed again");

    engine.settle();

    assert_ok(hub.take_outcome(ticket), "the replace prepared again");
    assert!(hub.take_outcome(ticket).is_none(), "one outcome");
    assert_eq!(engine.names(), names(&["A"]));
}

/// A scene's bytes whose read panics — a reader that fails without an
/// error.
struct PanickingFetch;

impl khora_data::scene::loading::SceneFetch for PanickingFetch {
    fn fetch(&self) -> Result<Vec<u8>, String> {
        panic!("the reader panicked");
    }
}

/// **A load whose preparation panics does not hold back the loads filed
/// after it.** Its read panics while the family prepares it: the run still
/// completes, and the load fails alone — an error outcome under its ticket
/// that names the panic, the world untouched. A replace filed afterwards
/// lands, with its outcome.
#[test]
fn a_load_whose_preparation_panics_does_not_hold_back_the_loads_filed_after_it() {
    let mut engine = Engine::new();
    engine.spawn_named("Old");
    let hub = engine.hub();

    let broken = engine.file(LoadKind::Replace(SceneSource::Fetch(Arc::new(
        PanickingFetch,
    ))));
    // The family's run, guarded as the scheduler guards a background run.
    let ran = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| engine.prepare_filed()));
    let (staging, taken) = ran.unwrap_or_else(|_| panic!("the family's run panicked"));
    assert_eq!(taken, 1, "the panicking load was taken");
    engine.land(staging);
    engine.boundary();

    assert!(
        !hub.is_pending(broken),
        "the panicked load is no longer pending"
    );
    match hub.take_outcome(broken) {
        Some(Err(error)) => assert!(
            error.to_string().contains("the reader panicked"),
            "the outcome names the panic: {error}"
        ),
        Some(Ok(report)) => panic!("a load whose read panicked landed: {report:?}"),
        None => panic!("no outcome under the panicked load's ticket"),
    }
    assert_eq!(engine.names(), names(&["Old"]), "the world is as it was");

    let next = engine.file(LoadKind::Replace(bytes(&scene_of(&["Next"]))));
    engine.settle();

    assert_ok(
        hub.take_outcome(next),
        "the load filed after the panicked one",
    );
    assert_eq!(engine.names(), names(&["Next"]));
}
