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

//! Whether an instance has announced itself, as a save records it.
//!
//! Every entity the flow hands the lane arrives with what the world holds of
//! it: the designer's fields, and — for one restored from a game save — what
//! the game made of it. What the fields hold cannot say which of the two it
//! is: a designer's values look like a history, and a behavior with nothing to
//! hold writes nothing even once it has run. So the save records whether
//! `OnSpawn` has started, and the lane reads that record — an entity that
//! starts fresh announces itself exactly once, and one restored from a save
//! never again.

use khora_core::ecs::entity::EntityId;
use khora_core::script::{EventQueue, PendingBody, ScriptSnapshot, ScriptValue};
use khora_core::Runtime;
use khora_data::ecs::{Script, ScriptState, Transform, World};
use khora_data::flow::{
    Flow, ScriptArrival, ScriptFlow, ScriptInstance, ScriptProgram, ScriptView,
};
use khora_lanes::script_lane::{run_behaviors, ScriptRunReport, ScriptRuntime};
use khora_script::arena::Persisted;
use khora_script::native::Host;
use khora_script::vm::{Program, Value};
use khora_script::{check, compile, ergon_fn, lex, parse};

use super::saves::every_encoding;

/// A native costing far more than the small slices below, so a body can be cut
/// at a known point: everything before it runs, it and everything after do not.
#[ergon_fn(name = "SpawnRecordWall", cost = 50)]
fn spawn_record_wall() -> f32 {
    0.0
}

const MODULE: &str = "spawn_record.erg";

fn build(source: &str) -> Program {
    let lexed = lex(source);
    assert!(lexed.diagnostics.is_empty(), "{:?}", lexed.diagnostics);
    let parsed = parse(lexed.tokens);
    assert!(parsed.diagnostics.is_empty(), "{:?}", parsed.diagnostics);
    let checked = check(&parsed.module);
    assert!(checked.diagnostics.is_empty(), "{:?}", checked.diagnostics);
    let compiled = compile(&parsed.module);
    assert!(
        compiled.diagnostics.is_empty(),
        "{:?}",
        compiled.diagnostics
    );
    compiled.program
}

fn runtime_of(source: &str) -> ScriptRuntime {
    let mut runtime = ScriptRuntime::new();
    runtime.add_program(MODULE, build(source));
    runtime
}

fn subject() -> EntityId {
    EntityId {
        index: 0,
        generation: 1,
    }
}

/// A view of the one subject, hand-built — carrying `arrival` as the flow
/// would on the frame it first appears.
fn view_of(behavior: &str, delta: f32, arrival: Option<ScriptArrival>) -> ScriptView {
    ScriptView {
        delta_seconds: delta,
        input: Default::default(),
        resumed: false,
        programs: vec![ScriptProgram {
            module: MODULE.to_owned(),
            behavior: behavior.to_owned(),
        }],
        instances: vec![ScriptInstance {
            entity: subject(),
            program: 0,
            arrival,
            translation: khora_core::math::Vec3::ZERO,
            rotation: khora_core::math::Quaternion::IDENTITY,
            scale: khora_core::math::Vec3::ONE,
        }],
    }
}

fn int_of(runtime: &ScriptRuntime, entity: EntityId, behavior: &str, slot: usize) -> Option<i64> {
    match runtime.peek(entity, behavior)?.fields.get(slot)? {
        Persisted::Scalar(Value::Int(value)) => Some(*value),
        _ => None,
    }
}

/// One frame: runs the view and returns what the lane reported, with the
/// events the frame raised as `(target, name)`.
fn frame(
    runtime: &mut ScriptRuntime,
    host: &mut Host,
    view: &ScriptView,
    fuel: u64,
) -> (ScriptRunReport, Vec<(EntityId, String)>) {
    let report = run_behaviors(view, &EventQueue::new(), runtime, host, fuel);
    let raised = host
        .take_events()
        .as_slice()
        .iter()
        .map(|event| (event.target, event.name.clone()))
        .collect();
    (report, raised)
}

fn count(raised: &[(EntityId, String)], name: &str) -> usize {
    raised.iter().filter(|(_, known)| known == name).count()
}

fn count_for(raised: &[(EntityId, String)], entity: EntityId, name: &str) -> usize {
    raised
        .iter()
        .filter(|(target, known)| *target == entity && known == name)
        .count()
}

/// What the lane wrote for the instance this frame.
fn recorded(report: &ScriptRunReport) -> ScriptSnapshot {
    report
        .state
        .first()
        .map(|update| update.snapshot.clone())
        .expect("the instance did work, so the lane wrote its state")
}

/// A world holding scripted entities, projected through the real flow — the
/// road every arrival takes from the world to the lane.
struct Scene {
    world: World,
    flow: ScriptFlow,
    entity: EntityId,
}

impl Scene {
    fn holding(bundle: impl khora_data::ecs::ComponentBundle) -> Self {
        let mut world = World::new();
        let entity = world.spawn(bundle);
        Self {
            world,
            flow: ScriptFlow::default(),
            entity,
        }
    }

    /// This frame's view, as the flow projects it.
    fn view(&mut self, delta: f32) -> ScriptView {
        let runtime = Runtime::default();
        let selection = self.flow.select(&self.world, &runtime);
        let mut view = self.flow.project(&self.world, &selection, &runtime);
        view.delta_seconds = delta;
        view
    }

    /// Runs `frames` frames at full fuel and returns every event they raised.
    fn run(
        &mut self,
        runtime: &mut ScriptRuntime,
        host: &mut Host,
        frames: usize,
    ) -> Vec<(EntityId, String)> {
        let mut raised = Vec::new();
        for _ in 0..frames {
            let view = self.view(0.0);
            raised.extend(frame(runtime, host, &view, u64::MAX).1);
        }
        raised
    }
}

/// An entity restored from a game save: its `Script`, and the `ScriptState`
/// the lane wrote back for `behavior`.
fn restored(behavior: &str, observed: ScriptSnapshot) -> Scene {
    Scene::holding((
        Transform::identity(),
        Script::new(MODULE, behavior),
        ScriptState {
            behavior: behavior.to_owned(),
            snapshot: observed,
        },
    ))
}

// ─── Behaviors ──────────────────────────────────────────────────────────────

/// Announces itself by counting and by raising an event.
const GREETER: &str = r#"behavior Greeter {
                            int health = 100;
                            int greeted = 0;
                            void OnSpawn() {
                                greeted += 1;
                                Raise(this, "Greeted");
                            }
                        }"#;

/// Nothing to hold — no field, no state, no countdown — so what it writes back
/// once it has run is empty but for its lifecycle. Its announcement is
/// observable only as an event.
const CHIME: &str = r#"behavior Chime {
                          void OnSpawn() { Raise(this, "Chimed"); }
                      }"#;

/// Announces itself, then walks every frame — so the lane writes its state
/// every frame.
const PACER: &str = r#"behavior Pacer {
                          int steps = 0;
                          void OnSpawn() { Raise(this, "Paced"); }
                          void Update(float dt) { steps += 1; }
                      }"#;

/// An announcement a small slice cuts at the wall: `first` and `Before` happen
/// before the cut, `second` and `After` after it.
const HERALD: &str = r#"behavior Herald {
                           int first = 0;
                           int second = 0;
                           void OnSpawn() {
                               first += 1;
                               Raise(this, "Before");
                               SpawnRecordWall();
                               second += 1;
                               Raise(this, "After");
                           }
                       }"#;

/// Enough for the initialiser and the part of `HERALD`'s `OnSpawn` before the
/// wall, not for the wall.
const CUT_BEFORE_THE_WALL: u64 = 30;

/// The two slots of `HERALD`, after the frames have run.
fn herald(runtime: &ScriptRuntime, entity: EntityId) -> (Option<i64>, Option<i64>) {
    (
        int_of(runtime, entity, "Herald", 0),
        int_of(runtime, entity, "Herald", 1),
    )
}

/// A running `HERALD` whose `OnSpawn` a small slice cut before the wall — and
/// what the lane recorded of it that frame.
fn herald_cut_part_way() -> ScriptSnapshot {
    let mut runtime = runtime_of(HERALD);
    let mut host = Host::new();
    let (report, raised) = frame(
        &mut runtime,
        &mut host,
        &view_of("Herald", 0.0, None),
        CUT_BEFORE_THE_WALL,
    );
    assert_eq!(
        herald(&runtime, subject()),
        (Some(1), Some(0)),
        "the slice ended inside OnSpawn, before the wall"
    );
    assert_eq!(count(&raised, "Before"), 1, "what precedes the cut ran");
    assert_eq!(count(&raised, "After"), 0, "what follows it did not");
    recorded(&report)
}

/// What a running behavior recorded after one full frame.
fn saved_after_one_frame(source: &str, behavior: &str, announcement: &str) -> ScriptSnapshot {
    let mut running = runtime_of(source);
    let mut host = Host::new();
    let (report, raised) = frame(
        &mut running,
        &mut host,
        &view_of(behavior, 0.0, None),
        u64::MAX,
    );
    assert_eq!(
        count(&raised, announcement),
        1,
        "the running game announced"
    );
    let saved = recorded(&report);
    assert!(saved.lifecycle.spawned, "and recorded it: {saved:?}");
    saved
}

// ─── An entity that starts fresh ────────────────────────────────────────────

/// **A designer's values are not a history.** An entity placed in a scene
/// arrives with the fields its designer typed and nothing observed: it runs
/// `OnSpawn` once — keeping the designer's values — and never again.
#[test]
fn a_scene_authored_entity_runs_on_spawn_once_and_keeps_the_designers_values() {
    let mut runtime = runtime_of(GREETER);
    let mut host = Host::new();
    let mut greeted = 0;
    let mut arrival = Some(ScriptArrival {
        fields: vec![("health".to_owned(), ScriptValue::Int(40))],
        observed: None,
    });
    for _ in 0..3 {
        let (_, raised) = frame(
            &mut runtime,
            &mut host,
            &view_of("Greeter", 0.0, arrival.take()),
            u64::MAX,
        );
        greeted += count(&raised, "Greeted");
    }

    assert_eq!(greeted, 1, "OnSpawn raised exactly once");
    assert_eq!(
        int_of(&runtime, subject(), "Greeter", 1),
        Some(1),
        "OnSpawn ran exactly once"
    );
    assert_eq!(
        int_of(&runtime, subject(), "Greeter", 0),
        Some(40),
        "the designer's value, not the declared default"
    );
}

/// **The same, on the road a scene really takes.** An entity placed in the
/// editor with a designer's field goes through the flow to the lane and
/// announces itself exactly once.
#[test]
fn an_entity_placed_in_a_scene_runs_on_spawn_once_through_the_flow() {
    let mut scene = Scene::holding((
        Transform::identity(),
        Script::new(MODULE, "Greeter").with_field("health", ScriptValue::Int(40)),
    ));
    let mut runtime = runtime_of(GREETER);
    let mut host = Host::new();

    let raised = scene.run(&mut runtime, &mut host, 3);

    assert_eq!(count(&raised, "Greeted"), 1, "OnSpawn raised exactly once");
    assert_eq!(int_of(&runtime, scene.entity, "Greeter", 1), Some(1));
    assert_eq!(
        int_of(&runtime, scene.entity, "Greeter", 0),
        Some(40),
        "the designer's value survives the announcement"
    );
}

/// An entity placed in a scene with no designer values at all, running a
/// behavior with nothing to hold, still announces itself.
#[test]
fn a_bare_entity_placed_in_a_scene_runs_on_spawn_through_the_flow() {
    let mut scene = Scene::holding((Transform::identity(), Script::new(MODULE, "Chime")));
    let mut runtime = runtime_of(CHIME);
    let mut host = Host::new();

    let raised = scene.run(&mut runtime, &mut host, 3);

    assert_eq!(count(&raised, "Chimed"), 1, "OnSpawn raised exactly once");
}

/// An entity placed in a scene whose `OnSpawn` the first frame's slice cuts
/// finishes it on a later frame — the part before the cut once, the part
/// after it once.
#[test]
fn a_scene_authored_entity_whose_on_spawn_is_cut_finishes_it_once() {
    let mut scene = Scene::holding((Transform::identity(), Script::new(MODULE, "Herald")));
    let mut runtime = runtime_of(HERALD);
    let mut host = Host::new();

    let view = scene.view(0.0);
    let (_, mut raised) = frame(&mut runtime, &mut host, &view, CUT_BEFORE_THE_WALL);
    assert_eq!(
        herald(&runtime, scene.entity),
        (Some(1), Some(0)),
        "the slice ended inside OnSpawn, before the wall"
    );
    raised.extend(scene.run(&mut runtime, &mut host, 3));

    assert_eq!(
        herald(&runtime, scene.entity),
        (Some(1), Some(1)),
        "OnSpawn ran once, whole"
    );
    assert_eq!(count(&raised, "Before"), 1, "the part before the cut, once");
    assert_eq!(count(&raised, "After"), 1, "the part after it, once");
}

/// **An entity spawned while the game runs is a fresh start too.** It
/// arrives through the flow with nothing observed and announces itself once;
/// the entity already running does not announce itself again.
#[test]
fn a_runtime_spawned_entity_runs_on_spawn() {
    let mut scene = Scene::holding((Transform::identity(), Script::new(MODULE, "Greeter")));
    let mut runtime = runtime_of(GREETER);
    let mut host = Host::new();
    let mut raised = scene.run(&mut runtime, &mut host, 2);

    let newcomer = scene
        .world
        .spawn((Transform::identity(), Script::new(MODULE, "Greeter")));
    raised.extend(scene.run(&mut runtime, &mut host, 3));

    assert_eq!(
        count_for(&raised, scene.entity, "Greeted"),
        1,
        "the first entity, once"
    );
    assert_eq!(
        count_for(&raised, newcomer, "Greeted"),
        1,
        "the spawned one, once"
    );
}

// ─── What the lane records ──────────────────────────────────────────────────

/// Once `OnSpawn` has run, everything the lane writes says so.
#[test]
fn the_lane_records_that_on_spawn_completed() {
    let mut runtime = runtime_of(PACER);
    let mut host = Host::new();

    let mut written = Vec::new();
    for _ in 0..3 {
        let (report, _) = frame(
            &mut runtime,
            &mut host,
            &view_of("Pacer", 0.016, None),
            u64::MAX,
        );
        written.push(recorded(&report));
    }

    for (index, snapshot) in written.iter().enumerate() {
        assert!(
            snapshot.lifecycle.spawned,
            "frame {index} recorded {snapshot:?}"
        );
    }
}

/// While `OnSpawn` is cut part-way, what the lane writes says it has started
/// and holds the rest of it as the body owed — so a load finishes it rather
/// than calling it afresh. Once it finishes, nothing is owed.
#[test]
fn the_lane_records_that_on_spawn_was_cut() {
    let mut runtime = runtime_of(HERALD);
    let mut host = Host::new();

    let (report, _) = frame(
        &mut runtime,
        &mut host,
        &view_of("Herald", 0.0, None),
        CUT_BEFORE_THE_WALL,
    );
    let cut = recorded(&report);
    assert!(cut.lifecycle.spawned, "OnSpawn has started: {cut:?}");
    let pending = cut.pending.as_ref().expect("the cut body is kept");
    assert_eq!(pending.machine.body, PendingBody::Spawn);

    let (report, _) = frame(
        &mut runtime,
        &mut host,
        &view_of("Herald", 0.0, None),
        u64::MAX,
    );
    let finished = recorded(&report);
    assert!(finished.lifecycle.spawned, "finished: {finished:?}");
    assert!(finished.pending.is_none(), "nothing is owed any more");
}

// ─── An entity restored from a save ─────────────────────────────────────────

/// **Loading a save is not spawning.** An instance restored from what the
/// lane wrote after its `OnSpawn` ran keeps what it had and does not announce
/// itself again — whichever encoding carried the save.
#[test]
fn a_restored_entity_that_had_spawned_does_not_run_on_spawn_again() {
    let saved = saved_after_one_frame(GREETER, "Greeter", "Greeted");

    for (encoding, carry) in every_encoding() {
        let mut scene = restored("Greeter", carry(&saved));
        let mut runtime = runtime_of(GREETER);
        let mut host = Host::new();

        let raised = scene.run(&mut runtime, &mut host, 3);

        assert_eq!(
            count(&raised, "Greeted"),
            0,
            "{encoding}: no second announcement"
        );
        assert_eq!(
            int_of(&runtime, scene.entity, "Greeter", 1),
            Some(1),
            "{encoding}: the count the save held, not incremented"
        );
    }
}

/// **Why the record cannot be derived.** A behavior with nothing to hold
/// writes back a snapshot empty but for its lifecycle even once it has run —
/// exactly what one that never ran holds otherwise. Restored from it, it
/// still does not announce itself again.
#[test]
fn a_restored_stateless_behavior_does_not_run_on_spawn_again() {
    let saved = saved_after_one_frame(CHIME, "Chime", "Chimed");
    assert!(
        saved.fields.is_empty()
            && saved.state.is_none()
            && saved.state_fields.is_empty()
            && saved.timers.is_empty()
            && saved.pending.is_none(),
        "nothing to hold: {saved:?}"
    );

    for (encoding, carry) in every_encoding() {
        let mut scene = restored("Chime", carry(&saved));
        let mut runtime = runtime_of(CHIME);
        let mut host = Host::new();

        let raised = scene.run(&mut runtime, &mut host, 3);

        assert_eq!(
            count(&raised, "Chimed"),
            0,
            "{encoding}: no second announcement"
        );
    }
}

/// **A cut `OnSpawn` loads as a cut `OnSpawn`.** A save taken while it was
/// part-way holds the rest of it; loaded into a fresh runtime, it finishes
/// that body once — the part before the cut, already done in the saved game,
/// is not done again, and the body is not called afresh.
#[test]
fn a_save_with_on_spawn_cut_part_way_finishes_it_once_after_loading() {
    let saved = herald_cut_part_way();
    assert!(
        saved.lifecycle.spawned,
        "the save records that OnSpawn has started: {saved:?}"
    );

    for (encoding, carry) in every_encoding() {
        let loaded = carry(&saved);
        assert_eq!(loaded, saved, "{encoding}: the save is carried whole");

        let mut scene = restored("Herald", loaded);
        let mut runtime = runtime_of(HERALD);
        let mut host = Host::new();

        let raised = scene.run(&mut runtime, &mut host, 3);

        assert_eq!(
            herald(&runtime, scene.entity),
            (Some(1), Some(1)),
            "{encoding}: OnSpawn ran once, whole, across the save"
        );
        assert_eq!(
            count(&raised, "Before"),
            0,
            "{encoding}: the part before the cut ran in the saved game, not again"
        );
        assert_eq!(
            count(&raised, "After"),
            1,
            "{encoding}: the part after it, once"
        );
    }
}

/// **A behavior new to a saved entity has never spawned.** The save recorded
/// what another behavior observed of the entity — that one had spawned. The
/// behavior the entity runs now was never observed, so it starts fresh and
/// announces itself once.
#[test]
fn a_behavior_added_after_the_save_runs_on_spawn() {
    let saved = saved_after_one_frame(CHIME, "Chime", "Chimed");

    for (encoding, carry) in every_encoding() {
        let mut scene = Scene::holding((
            Transform::identity(),
            Script::new(MODULE, "Greeter"),
            ScriptState {
                behavior: "Chime".to_owned(),
                snapshot: carry(&saved),
            },
        ));
        let mut runtime = runtime_of(GREETER);
        let mut host = Host::new();

        let raised = scene.run(&mut runtime, &mut host, 3);

        assert_eq!(
            count(&raised, "Greeted"),
            1,
            "{encoding}: the new behavior announced itself, once"
        );
        assert_eq!(int_of(&runtime, scene.entity, "Greeter", 1), Some(1));
    }
}

// ─── Entities that left while the game was away ─────────────────────────────

/// Says goodbye, so a farewell is visible as an event.
const KEEPER: &str = r#"behavior Keeper {
                           int health = 100;
                           void OnDespawn() { Raise(this, "Gone"); }
                       }"#;

/// A view of `entities`, each running `Keeper`.
fn keepers(entities: &[EntityId], resumed: bool) -> ScriptView {
    let one = view_of("Keeper", 0.0, None);
    ScriptView {
        resumed,
        instances: entities
            .iter()
            .map(|entity| ScriptInstance {
                entity: *entity,
                ..one.instances[0].clone()
            })
            .collect(),
        ..one
    }
}

/// **Nothing was running to say goodbye.** On the first frame back in play,
/// an instance whose entity left while the game was away — the editor
/// deleted it, a tool mode cleaned up — is dropped without its `OnDespawn`.
/// The instances still in the view carry on.
#[test]
fn an_entity_that_left_while_the_game_was_away_is_dropped_without_a_farewell() {
    let stays = subject();
    let left = EntityId {
        index: 1,
        generation: 1,
    };
    let mut runtime = runtime_of(KEEPER);
    let mut host = Host::new();
    frame(
        &mut runtime,
        &mut host,
        &keepers(&[stays, left], false),
        u64::MAX,
    );
    assert_eq!(runtime.instance_count(), 2);

    let (_, mut raised) = frame(&mut runtime, &mut host, &keepers(&[stays], true), u64::MAX);
    raised.extend(frame(&mut runtime, &mut host, &keepers(&[stays], false), u64::MAX).1);

    assert_eq!(count(&raised, "Gone"), 0, "no farewell: {raised:?}");
    assert!(
        runtime.peek(left, "Keeper").is_none(),
        "the instance is dropped"
    );
    assert!(
        runtime.peek(stays, "Keeper").is_some(),
        "the one still in the view carries on"
    );
    assert_eq!(runtime.instance_count(), 1);
}

/// **The same, on the road the frame takes.** A game leaves play, an entity
/// is despawned meanwhile, the game comes back: the flow says the frame is a
/// resumption, and the lane drops the instance without its `OnDespawn` while
/// the other runs on.
#[test]
fn returning_to_play_drops_what_left_without_a_farewell_through_the_flow() {
    use khora_core::agent::{EngineMode, SharedEngineMode};

    let mut scene = Scene::holding((Transform::identity(), Script::new(MODULE, "Keeper")));
    let left = scene
        .world
        .spawn((Transform::identity(), Script::new(MODULE, "Keeper")));
    let mode: SharedEngineMode = std::sync::Arc::new(std::sync::RwLock::new(EngineMode::Playing));
    let mut engine = Runtime::default();
    engine.resources.insert(mode.clone());
    let view = |scene: &mut Scene| {
        let selection = scene.flow.select(&scene.world, &engine);
        scene.flow.project(&scene.world, &selection, &engine)
    };
    let mut runtime = runtime_of(KEEPER);
    let mut host = Host::new();

    let playing = view(&mut scene);
    frame(&mut runtime, &mut host, &playing, u64::MAX);
    assert_eq!(runtime.instance_count(), 2);

    *mode.write().expect("the mode lock") = EngineMode::Custom("tool".to_owned());
    // The scripting agent does not run outside play: the flow projects, the
    // lane is not called.
    view(&mut scene);
    scene.world.despawn(left);
    view(&mut scene);

    *mode.write().expect("the mode lock") = EngineMode::Playing;
    let back = view(&mut scene);
    assert!(back.resumed, "the first frame back is a resumption");
    let (_, raised) = frame(&mut runtime, &mut host, &back, u64::MAX);

    assert_eq!(count(&raised, "Gone"), 0, "no farewell: {raised:?}");
    assert!(
        runtime.peek(left, "Keeper").is_none(),
        "the instance is dropped"
    );
    assert!(
        runtime.peek(scene.entity, "Keeper").is_some(),
        "the other carries on"
    );
}
