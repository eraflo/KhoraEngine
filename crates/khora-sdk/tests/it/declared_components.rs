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

//! A game attaches a component its scripts declare — a behavior, or a plain
//! script `component` — by name, through `GameWorld::add_declared_component`:
//! the fields it names take their values, the others the declared defaults.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use khora_sdk::khora_core::ecs::entity::EntityId;
use khora_sdk::khora_core::script::ScriptValue;
use khora_sdk::khora_data::ecs::RowError;
use khora_sdk::prelude::ecs::Transform;
use khora_sdk::prelude::math::Vec3;
use khora_sdk::{
    AgentProvider, DccService, DeclaredComponentError, EngineApp, EngineCore, GameWorld,
    InputEvent, PhaseProvider, Runtime, WindowConfig,
};

use khora_io::script::declarations::{apply_declarations, declaration_channel, DeclaredComponents};
use khora_io::script::hot_reload::{load_all, reload_channel};

const MODULE: &str = "guard.erg";

/// A behavior and a plain component, declared by one module.
const GUARD: &str = "component Loot { int value = 1; }
                     behavior Guard {
                         float speed = 1.5;
                         int hp = 10;
                         void Update(float dt) { hp += 1; }
                     }";

/// A fresh directory holding `scripts/guard.erg`.
fn scripts_with_guard() -> PathBuf {
    let root = std::env::temp_dir().join(format!(
        "khora-sdk-declared-components-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |since| since.as_nanos())
    ));
    let scripts = root.join("scripts");
    std::fs::create_dir_all(&scripts).expect("a scratch script directory");
    std::fs::write(scripts.join(MODULE), GUARD).expect("the script is written");
    root
}

/// A game world knowing what `guard.erg` declares, as the engine registers it
/// from the initial load.
fn world_declaring_guard() -> GameWorld {
    let root = scripts_with_guard();
    let (pending, declarations) = (reload_channel(), declaration_channel());
    assert_eq!(
        load_all(&root.join("scripts"), &pending, &declarations),
        1,
        "`{MODULE}` compiles"
    );
    let mut world = GameWorld::new();
    let report = apply_declarations(
        world.inner_world_mut(),
        &mut DeclaredComponents::default(),
        &declarations.drain(),
    );
    assert!(
        report.registered.contains(&"Guard".to_owned())
            && report.registered.contains(&"Loot".to_owned()),
        "registered: {report:?}"
    );
    assert!(report.refused.is_empty(), "nothing refused: {report:?}");
    let _ = std::fs::remove_dir_all(&root);
    world
}

/// `entity`'s row of the declared component `name`, its fields by name.
fn row(world: &GameWorld, entity: EntityId, name: &str) -> Option<Vec<(String, ScriptValue)>> {
    let world = world.inner_world();
    let key = world.components().key_named(name)?;
    world.declared_row(entity, key)
}

fn fields(pairs: &[(&str, ScriptValue)]) -> Vec<(String, ScriptValue)> {
    pairs
        .iter()
        .map(|(name, value)| ((*name).to_owned(), value.clone()))
        .collect()
}

/// **A behavior attaches by name**: the field named takes its value, the one
/// left unnamed its declared default.
#[test]
fn a_declared_behavior_attaches_with_its_named_fields_and_defaults_for_the_rest() {
    let mut world = world_declaring_guard();
    let entity = world.spawn(Transform::identity());

    world
        .add_declared_component(entity, "Guard", &[("hp", ScriptValue::Int(3))])
        .expect("`Guard` attaches");

    assert_eq!(
        row(&world, entity, "Guard"),
        Some(fields(&[
            ("speed", ScriptValue::Float(1.5)),
            ("hp", ScriptValue::Int(3)),
        ]))
    );
    assert!(
        world.get_transform(entity).is_some(),
        "the entity keeps what it held"
    );
}

/// **A script's plain component attaches the same way**, every field at its
/// default when none is named.
#[test]
fn a_declared_component_attaches_at_its_defaults_when_no_field_is_named() {
    let mut world = world_declaring_guard();
    let entity = world.spawn(Transform::identity());

    world
        .add_declared_component(entity, "Loot", &[])
        .expect("`Loot` attaches");

    assert_eq!(
        row(&world, entity, "Loot"),
        Some(fields(&[("value", ScriptValue::Int(1))]))
    );
}

/// **A name no script declares** is not declared.
#[test]
fn a_name_no_script_declares_is_not_declared() {
    let mut world = world_declaring_guard();
    let entity = world.spawn(Transform::identity());

    assert_eq!(
        world.add_declared_component(entity, "Sentry", &[]),
        Err(DeclaredComponentError::NotDeclared("Sentry".to_owned()))
    );
}

/// **A Rust component's name is not declared**: it has no declared fields —
/// `add_component` attaches it.
#[test]
fn a_rust_components_name_is_not_declared() {
    let mut world = world_declaring_guard();
    let entity = world.spawn(Transform::identity());

    assert_eq!(
        world.add_declared_component(entity, "Transform", &[]),
        Err(DeclaredComponentError::NotDeclared("Transform".to_owned()))
    );
}

/// **A field that refuses its value attaches nothing**: neither that field
/// nor the others named beside it.
#[test]
fn a_refused_field_value_attaches_nothing() {
    let mut world = world_declaring_guard();
    let entity = world.spawn(Transform::identity());

    let attached = world.add_declared_component(
        entity,
        "Guard",
        &[
            ("speed", ScriptValue::Float(2.0)),
            ("hp", ScriptValue::Str("many".to_owned())),
        ],
    );

    assert!(
        matches!(
            attached,
            Err(DeclaredComponentError::Refused(RowError::Field(ref error))) if error.field == "hp"
        ),
        "{attached:?}"
    );
    assert_eq!(row(&world, entity, "Guard"), None, "nothing attached");
}

/// **A field the component does not have is refused**, and nothing attached.
#[test]
fn a_field_the_component_does_not_have_is_refused() {
    let mut world = world_declaring_guard();
    let entity = world.spawn(Transform::identity());

    assert_eq!(
        world.add_declared_component(entity, "Guard", &[("armour", ScriptValue::Int(1))]),
        Err(DeclaredComponentError::Refused(RowError::UnknownField {
            field: "armour".to_owned()
        }))
    );
    assert_eq!(row(&world, entity, "Guard"), None, "nothing attached");
}

/// **A dead entity is refused.**
#[test]
fn attaching_to_a_dead_entity_is_refused() {
    let mut world = world_declaring_guard();
    let entity = world.spawn(Transform::identity());
    assert!(world.despawn(entity));

    assert_eq!(
        world.add_declared_component(entity, "Guard", &[]),
        Err(DeclaredComponentError::Refused(RowError::NoSuchEntity(
            entity
        )))
    );
}

/// **A component already held is refused**, the row it holds untouched.
#[test]
fn attaching_a_component_already_held_is_refused_and_keeps_the_row() {
    let mut world = world_declaring_guard();
    let entity = world.spawn(Transform::identity());
    world
        .add_declared_component(entity, "Guard", &[("hp", ScriptValue::Int(3))])
        .expect("`Guard` attaches");

    assert_eq!(
        world.add_declared_component(entity, "Guard", &[("hp", ScriptValue::Int(7))]),
        Err(DeclaredComponentError::Refused(RowError::AlreadyAttached))
    );
    assert_eq!(
        row(&world, entity, "Guard"),
        Some(fields(&[
            ("speed", ScriptValue::Float(1.5)),
            ("hp", ScriptValue::Int(3)),
        ]))
    );
}

/// **A behavior's state companion is the engine's to write**: a game cannot
/// attach `Guard#state` by name, as if it were a component a script declares.
#[test]
fn a_behaviors_state_companion_cannot_be_attached_by_name() {
    let mut world = world_declaring_guard();
    let entity = world.spawn(Transform::identity());

    assert_eq!(
        world.add_declared_component(entity, "Guard#state", &[]),
        Err(DeclaredComponentError::NotDeclared(
            "Guard#state".to_owned()
        ))
    );
    assert_eq!(row(&world, entity, "Guard#state"), None, "nothing attached");
}

/// **A behavior no script declares any more is not declared**: its module
/// recompiled without it, it stays registered with its rows, orphaned — but a
/// game attaching it by name would attach a behavior nothing runs.
#[test]
fn an_orphaned_behavior_is_not_declared() {
    let root = scripts_with_guard();
    let scripts = root.join("scripts");
    let (pending, declarations) = (reload_channel(), declaration_channel());
    let mut state = DeclaredComponents::default();
    let mut world = GameWorld::new();
    assert_eq!(load_all(&scripts, &pending, &declarations), 1);
    apply_declarations(world.inner_world_mut(), &mut state, &declarations.drain());

    std::fs::write(scripts.join(MODULE), "component Loot { int value = 1; }")
        .expect("the script is rewritten");
    assert_eq!(load_all(&scripts, &pending, &declarations), 1);
    let report = apply_declarations(world.inner_world_mut(), &mut state, &declarations.drain());
    assert!(
        report.orphaned.contains(&"Guard".to_owned()),
        "`Guard` is orphaned: {report:?}"
    );
    let _ = std::fs::remove_dir_all(&root);
    let entity = world.spawn(Transform::identity());

    assert_eq!(
        world.add_declared_component(entity, "Guard", &[]),
        Err(DeclaredComponentError::NotDeclared("Guard".to_owned()))
    );
}

// ─── Attached in setup, run once played ─────────────────────────────────────

/// A game whose setup spawns one entity at `at` and attaches to it, in
/// order, each declared component `attach` names, with its fields.
struct Attaching {
    at: Vec3,
    attach: Vec<(&'static str, Vec<(&'static str, ScriptValue)>)>,
    /// The entity, and what each attach gave.
    attached: Option<(EntityId, Vec<Result<(), DeclaredComponentError>>)>,
}

impl AgentProvider for Attaching {
    fn register_agents(&self, _dcc: &DccService, _runtime: &mut Runtime) {}
}
impl PhaseProvider for Attaching {}
impl EngineApp for Attaching {
    fn window_config() -> WindowConfig {
        WindowConfig::default()
    }
    fn new() -> Self {
        Attaching {
            at: Vec3::ZERO,
            attach: Vec::new(),
            attached: None,
        }
    }
    fn setup(&mut self, world: &mut GameWorld, _runtime: &Runtime) {
        let entity = world.spawn(Transform::from_translation(self.at));
        let results = self
            .attach
            .iter()
            .map(|(name, fields)| world.add_declared_component(entity, name, fields))
            .collect();
        self.attached = Some((entity, results));
    }
    fn update(&mut self, _world: &mut GameWorld, _inputs: &[InputEvent]) {}
}

/// A fresh assets root holding `scripts/<module>` with `source`.
fn assets_with(module: &str, source: &str) -> PathBuf {
    let root = std::env::temp_dir().join(format!(
        "khora-sdk-declared-in-setup-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |since| since.as_nanos())
    ));
    let scripts = root.join("scripts");
    std::fs::create_dir_all(&scripts).expect("a scratch script directory");
    std::fs::write(scripts.join(module), source).expect("the script is written");
    root
}

/// A headless game, its scripts mounted from `root`, its setup run.
fn game(root: &Path, app: Attaching) -> EngineCore<Attaching> {
    let mut runtime = Runtime::new();
    khora_sdk::scripts::mount(&mut runtime, root);
    let mut engine = EngineCore::<Attaching>::new();
    engine.bootstrap(app, runtime);
    engine
}

/// The entity setup spawned; panics unless every attach succeeded.
fn attached_entity(engine: &mut EngineCore<Attaching>) -> EntityId {
    match engine.app_mut().and_then(|app| app.attached.clone()) {
        Some((entity, results)) if results.iter().all(Result::is_ok) => entity,
        other => panic!("setup attaches what the mounted scripts declare, got {other:?}"),
    }
}

/// Field `field` of `entity`'s `component` row.
fn field_of(
    engine: &mut EngineCore<Attaching>,
    entity: EntityId,
    component: &str,
    field: &str,
) -> Option<ScriptValue> {
    let world = engine.game_world_mut()?.inner_world();
    let key = world.components().key_named(component)?;
    world
        .declared_row(entity, key)?
        .into_iter()
        .find_map(|(name, value)| (name == field).then_some(value))
}

/// Ticks until `done` holds, or panics after a few seconds with `what`.
fn tick_until(
    engine: &mut EngineCore<Attaching>,
    what: &str,
    mut done: impl FnMut(&mut EngineCore<Attaching>) -> bool,
) {
    let deadline = Instant::now() + Duration::from_secs(5);
    while Instant::now() < deadline {
        engine.tick();
        if done(engine) {
            return;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    panic!("timed out waiting for {what}");
}

/// **A behavior attached in setup runs every frame once played**, with the
/// value setup gave the field it named — not the declared default — and its
/// writes land in its row frame after frame.
#[test]
fn a_behavior_attached_in_setup_runs_every_frame_with_the_field_setup_named() {
    let root = assets_with(
        "mover.erg",
        "behavior Mover {
             float speed = 1.5;
             int ticks = 0;
             float travelled = 0.0;
             void Update(float dt) { ticks += 1; travelled += speed; }
         }",
    );
    let mut engine = game(
        &root,
        Attaching {
            at: Vec3::ZERO,
            attach: vec![("Mover", vec![("speed", ScriptValue::Float(2.0))])],
            attached: None,
        },
    );
    let entity = attached_entity(&mut engine);

    tick_until(
        &mut engine,
        "`Mover` to run three frames",
        |engine| matches!(field_of(engine, entity, "Mover", "ticks"), Some(ScriptValue::Int(n)) if n >= 3),
    );

    let ticks = match field_of(&mut engine, entity, "Mover", "ticks") {
        Some(ScriptValue::Int(n)) => n,
        other => panic!("ticks: {other:?}"),
    };
    assert_eq!(
        field_of(&mut engine, entity, "Mover", "speed"),
        Some(ScriptValue::Float(2.0)),
        "the value setup named"
    );
    assert_eq!(
        field_of(&mut engine, entity, "Mover", "travelled"),
        Some(ScriptValue::Float(2.0 * ticks as f32)),
        "every frame ran at the speed setup named"
    );
    let _ = std::fs::remove_dir_all(&root);
}

/// **A field setup leaves unnamed takes what `OnSpawn` computes**, starting
/// from its registered default, while the field setup named keeps its value.
#[test]
fn a_field_setup_leaves_unnamed_takes_what_on_spawn_computes() {
    let root = assets_with(
        "homing.erg",
        "behavior Homing {
             float speed = 1.5;
             Vec3 home;
             int ticks = 0;
             void OnSpawn() { home = Position(); }
             void Update(float dt) { ticks += 1; }
         }",
    );
    let at = Vec3::new(3.0, 4.0, 5.0);
    let mut engine = game(
        &root,
        Attaching {
            at,
            attach: vec![("Homing", vec![("speed", ScriptValue::Float(2.0))])],
            attached: None,
        },
    );
    let entity = attached_entity(&mut engine);

    tick_until(
        &mut engine,
        "`Homing` to run",
        |engine| matches!(field_of(engine, entity, "Homing", "ticks"), Some(ScriptValue::Int(n)) if n >= 1),
    );

    assert_eq!(
        field_of(&mut engine, entity, "Homing", "home"),
        Some(ScriptValue::Vec3(at)),
        "`OnSpawn` ran where setup put the entity"
    );
    assert_eq!(
        field_of(&mut engine, entity, "Homing", "speed"),
        Some(ScriptValue::Float(2.0))
    );
    let _ = std::fs::remove_dir_all(&root);
}

/// **An edit after startup still reaches the world**: the bootstrap drained
/// the initial load, and the frame boundary registers what the hot reload
/// sends later — the behavior attached in setup takes its new field, keeping
/// the value setup named.
#[test]
fn a_behavior_attached_in_setup_takes_a_field_its_module_adds_after_startup() {
    let root = assets_with(
        "mover.erg",
        "behavior Mover {
             float speed = 1.5;
             int ticks = 0;
             void Update(float dt) { ticks += 1; }
         }",
    );
    let mut engine = game(
        &root,
        Attaching {
            at: Vec3::ZERO,
            attach: vec![("Mover", vec![("speed", ScriptValue::Float(2.0))])],
            attached: None,
        },
    );
    let entity = attached_entity(&mut engine);
    tick_until(
        &mut engine,
        "`Mover` to run",
        |engine| matches!(field_of(engine, entity, "Mover", "ticks"), Some(ScriptValue::Int(n)) if n >= 1),
    );

    std::fs::write(
        root.join("scripts").join("mover.erg"),
        "behavior Mover {
             float speed = 1.5;
             int ticks = 0;
             int extra = 7;
             void Update(float dt) { ticks += 1; }
         }",
    )
    .expect("the script is edited");

    tick_until(&mut engine, "`Mover` to take its new field", |engine| {
        field_of(engine, entity, "Mover", "extra") == Some(ScriptValue::Int(7))
    });
    assert_eq!(
        field_of(&mut engine, entity, "Mover", "speed"),
        Some(ScriptValue::Float(2.0))
    );
    let _ = std::fs::remove_dir_all(&root);
}
