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

//! A project's behaviors reach the `World` as components (Ergon spec 09):
//! each behavior's own component, authored — a scene keeps it — and its
//! state companion, written by the engine and kept only by a game save, so
//! registered `Runtime` and resumable.

use khora_core::script::ScriptValue;
use khora_data::ecs::{
    ComponentProvenance, ComponentVTable, ScriptRole, SemanticDomain, Transform, World,
};
use khora_io::script::declarations::{
    apply_declarations, DeclarationReport, DeclaredComponents, ScriptDeclarations,
};
use khora_io::script::mirror::PreludeLoader;
use khora_script::vm::Program;
use khora_script::{compile_module, Diagnostic, MemoryLoader};

const GUARD: &str = "behavior Guard {
    float speed = 1.5;
    int hp = 10;
    void Update(float dt) { hp += 1; }
}";

/// The state companion's fields, in order — the same for every behavior.
const COMPANION_FIELDS: [&str; 9] = [
    "state",
    "state_fields",
    "timers",
    "pending",
    "spawned",
    "fault",
    "resume_failed",
    "owed",
    "fixed_owed",
];

/// `module` compiled alone, the engine's mirrors available.
fn compile(module: &str, source: &str) -> Program {
    let inner = MemoryLoader::new().with(module, source);
    let result = compile_module(&PreludeLoader::new(inner), module);
    let errors: Vec<&Diagnostic> = result.diagnostics.iter().filter(|d| d.is_error()).collect();
    assert!(errors.is_empty(), "`{module}` should compile: {errors:?}");
    result.program.expect("a program with no errors")
}

/// What the engine sends for `module` once it compiled `source`.
fn declarations(module: &str, source: &str) -> ScriptDeclarations {
    let program = compile(module, source);
    ScriptDeclarations {
        module: module.to_owned(),
        components: program
            .declared
            .into_iter()
            .filter(|declared| declared.module == module)
            .collect(),
    }
}

fn registered(batch: &[ScriptDeclarations]) -> (World, DeclaredComponents, DeclarationReport) {
    let mut world = World::new();
    let mut state = DeclaredComponents::default();
    let report = apply_declarations(&mut world, &mut state, batch);
    (world, state, report)
}

fn vtable<'w>(world: &'w World, name: &str) -> &'w ComponentVTable {
    let key = world
        .components()
        .key_named(name)
        .unwrap_or_else(|| panic!("`{name}` is registered"));
    world.components().vtable(key).expect("a vtable")
}

fn field_names(vtable: &ComponentVTable) -> Vec<String> {
    vtable
        .columns
        .packed()
        .expect("a declared component's columns are packed")
        .fields()
        .iter()
        .map(|field| field.name.clone())
        .collect()
}

/// **A behavior registers two components**: its own, authored and not
/// resumable, with its fields; and its state companion, `Runtime` and
/// resumable, in `Script`, with the companion's fixed fields.
#[test]
fn a_behavior_registers_authored_and_its_state_runtime_and_resumable() {
    let (world, state, report) = registered(&[declarations("ai/guard.erg", GUARD)]);

    assert_eq!(
        report.registered,
        vec!["Guard".to_owned(), "Guard#state".to_owned()]
    );

    let guard = vtable(&world, "Guard");
    assert_eq!(guard.provenance, ComponentProvenance::Authored);
    assert!(!guard.resumable, "a scene keeps the behavior's fields");
    assert_eq!(guard.domain, SemanticDomain::Script);
    assert_eq!(field_names(guard), vec!["speed", "hp"]);

    let companion = vtable(&world, "Guard#state");
    assert_eq!(companion.provenance, ComponentProvenance::Runtime);
    assert!(
        companion.resumable,
        "a game save keeps a behavior's state so play resumes"
    );
    assert_eq!(companion.domain, SemanticDomain::Script);
    assert_eq!(field_names(companion), COMPANION_FIELDS);

    assert_eq!(state.owner_of("Guard"), Some("ai/guard.erg"));
    assert_eq!(state.owner_of("Guard#state"), Some("ai/guard.erg"));
}

/// **A component a script declares stays authored and not resumable** beside
/// a behavior of the same module.
#[test]
fn a_component_beside_a_behavior_registers_authored() {
    let (world, _, report) = registered(&[declarations(
        "ai/guard.erg",
        &format!("component Loot {{ int value = 1; }}\n{GUARD}"),
    )]);

    assert_eq!(
        report.registered,
        vec![
            "Guard".to_owned(),
            "Guard#state".to_owned(),
            "Loot".to_owned()
        ]
    );
    let loot = vtable(&world, "Loot");
    assert_eq!(loot.provenance, ComponentProvenance::Authored);
    assert!(!loot.resumable);
}

/// **`[Domain(Physics)]` places the behavior's fields, not its state**: a row
/// of `Guard` is a Physics row, a row of `Guard#state` a Script one.
#[test]
fn a_behavior_domain_places_its_fields_and_never_its_state() {
    let (mut world, _, _) = registered(&[declarations(
        "ai/hover.erg",
        "[Domain(Physics)] [Critical]
         behavior Hover { float height = 0.4; }",
    )]);

    assert_eq!(vtable(&world, "Hover").domain, SemanticDomain::Physics);
    assert_eq!(vtable(&world, "Hover#state").domain, SemanticDomain::Script);

    let entity = world.spawn(Transform::identity());
    let hover = world.components().key_named("Hover").expect("registered");
    let state = world
        .components()
        .key_named("Hover#state")
        .expect("registered");
    world
        .add_runtime_component(entity, hover, &ScriptValue::Unit)
        .expect("`Hover` attaches");
    world
        .add_runtime_component(entity, state, &ScriptValue::Unit)
        .expect("`Hover#state` attaches");
    assert_eq!(
        world
            .row(entity, hover)
            .expect("the entity holds Hover")
            .field(0),
        Some(ScriptValue::Float(0.4)),
        "a new row starts at the declared default"
    );
}

/// **Two modules that never import each other, one behavior name**: each
/// compiles alone, so the registration refuses the second — the first
/// module's behavior keeps its fields.
#[test]
fn a_behavior_name_another_module_owns_is_refused_at_registration() {
    let (world, state, report) = registered(&[
        declarations("a.erg", "behavior Guard { int a = 1; }"),
        declarations("b.erg", "behavior Guard { float b = 2.0; }"),
    ]);

    assert!(
        report.refused.contains(&"Guard".to_owned()),
        "the second module's `Guard` is refused: {report:?}"
    );
    assert_eq!(state.owner_of("Guard"), Some("a.erg"));
    assert_eq!(field_names(vtable(&world, "Guard")), vec!["a"]);
}

/// **A behavior named like another module's component** — modules that never
/// import each other — is refused at registration, the component kept.
#[test]
fn a_behavior_named_like_another_modules_component_is_refused_at_registration() {
    let (world, state, report) = registered(&[
        declarations("a.erg", "component Guard { int n = 0; }"),
        declarations("b.erg", GUARD),
    ]);

    assert!(
        report.refused.contains(&"Guard".to_owned()),
        "the behavior is refused: {report:?}"
    );
    assert_eq!(state.owner_of("Guard"), Some("a.erg"));
    let guard = vtable(&world, "Guard");
    assert_eq!(field_names(guard), vec!["n"]);
    assert_eq!(guard.provenance, ComponentProvenance::Authored);
}

/// **Every field a behavior may hold registers**: whatever its type — an
/// engine value, an entity, a struct, an optional, an array, a map — with or
/// without a default (a type with no neutral value is held as an optional),
/// the behavior's component registers with every field, and its companion
/// beside it.
#[test]
fn every_field_a_behavior_holds_registers() {
    let (world, _, report) = registered(&[declarations(
        "ai/guard.erg",
        "struct Point { float x = 0.0; }
         struct Route { Point from = Point { x: 1.0 }; Point[] stops; }
         behavior Guard {
             bool alert;
             int hp;
             float speed;
             string title;
             Entity? target;
             Vec2 aim;
             Vec3 home;
             Vec4 mask;
             Quat facing;
             Color tint;
             Duration pause;
             Angle turn = 45deg;
             Point at;
             Route route = Route { };
             Point? waypoint;
             Entity[] seen;
             Map<string, int>? tally;
             void Update(float dt) { }
         }",
    )]);

    assert_eq!(
        report.registered,
        vec!["Guard".to_owned(), "Guard#state".to_owned()],
        "the behavior and its companion register"
    );
    assert_eq!(
        field_names(vtable(&world, "Guard")),
        vec![
            "alert", "hp", "speed", "title", "target", "aim", "home", "mask", "facing", "tint",
            "pause", "turn", "at", "route", "waypoint", "seen", "tally"
        ]
    );
}

/// **A behavior the registration refuses brings no state companion**: when
/// another module's component owns the name, the behavior is not registered,
/// and neither is a `Guard#state` that would belong to no behavior.
#[test]
fn a_behavior_refused_at_registration_registers_no_state_companion() {
    let (world, state, report) = registered(&[
        declarations("a.erg", "component Guard { int n = 0; }"),
        declarations("b.erg", GUARD),
    ]);

    assert!(report.refused.contains(&"Guard".to_owned()), "{report:?}");
    assert!(
        world.components().key_named("Guard#state").is_none(),
        "no companion for a refused behavior: {report:?}, owned by {:?}",
        state.owner_of("Guard#state")
    );
}

/// `module`'s behaviors registered in a fresh world, and one entity holding
/// `behavior` and its companion as a new row starts them: the world and the
/// entity's identity.
fn holding(module: &str, source: &str, behavior: &str) -> (World, khora_core::ecs::PersistentId) {
    let (mut world, _, report) = registered(&[declarations(module, source)]);
    let companion = format!("{behavior}#state");
    assert!(
        report.registered.contains(&behavior.to_owned()) && report.registered.contains(&companion),
        "both register: {report:?}"
    );
    let entity = world.spawn(Transform::identity());
    for name in [behavior, companion.as_str()] {
        let key = world.components().key_named(name).expect("registered");
        world
            .add_runtime_component(entity, key, &ScriptValue::Unit)
            .unwrap_or_else(|e| panic!("`{name}` attaches: {e:?}"));
    }
    let id = world.mark_authored(entity).expect("authored");
    (world, id)
}

/// A scene and a game save of `world`, each loaded back into a fresh world
/// through every record goal: what failed, by goal.
fn round_trips(world: &World) -> Vec<String> {
    use khora_core::asset::AssetUUID;
    use khora_core::scene::{SceneFile, SerializationGoal};
    use khora_io::serialization::SerializationService;
    let on_disk = |file: &SceneFile| SceneFile::from_bytes(&file.to_bytes()).expect("parses");
    let service = SerializationService::new();
    let mut failed = Vec::new();
    for goal in [
        SerializationGoal::HumanReadableDebug,
        SerializationGoal::EditorInterchange,
        SerializationGoal::PortableBinary,
    ] {
        let scene = match service.save_world(world, goal) {
            Ok(file) => file,
            Err(e) => {
                failed.push(format!("{goal:?}: the scene saves: {e:?}"));
                continue;
            }
        };
        if let Err(e) = service.load_world(&on_disk(&scene), &mut World::new()) {
            failed.push(format!("{goal:?}: the scene loads: {e:?}"));
        }
        match service.save_game(world, AssetUUID::new(), &scene, goal) {
            Ok(save) => {
                if let Err(e) =
                    service.load_game(&mut World::new(), &on_disk(&save), &on_disk(&scene))
                {
                    failed.push(format!("{goal:?}: the game loads: {e:?}"));
                }
            }
            Err(e) => failed.push(format!("{goal:?}: the game saves: {e:?}")),
        }
    }
    failed
}

/// **A behavior holding a map is saved and loaded back**: a scene and a game
/// save of an entity holding one (optional, as a map has no neutral value)
/// load into a fresh world, under every record goal — the declaration a file
/// keeps of it reads back.
#[test]
fn a_behavior_holding_a_map_saves_and_loads_back() {
    let (world, _) = holding(
        "ai/guard.erg",
        "behavior Guard { Map<string, int>? tally; int hp = 1; }",
        "Guard",
    );
    let failed = round_trips(&world);
    assert!(failed.is_empty(), "{failed:#?}");
}

/// **A behavior holding an optional entity with no default is saved and
/// loaded back**: a new row starts it at `null`, which a scene and a game save
/// carry like any field.
#[test]
fn a_behavior_holding_an_optional_entity_with_no_default_saves_and_loads_back() {
    let (world, _) = holding(
        "ai/guard.erg",
        "behavior Guard { Entity? target; int hp = 1; }",
        "Guard",
    );
    let failed = round_trips(&world);
    assert!(failed.is_empty(), "{failed:#?}");
}

/// **A behavior with a float default beyond a float still registers**, or is
/// refused when it compiles: never half registered, its companion alone.
#[test]
fn a_behavior_with_an_infinite_default_is_never_half_registered() {
    let inner = MemoryLoader::new().with("ai/far.erg", "behavior Far { float reach = 1e39; }");
    let result = compile_module(&PreludeLoader::new(inner), "ai/far.erg");
    let Some(program) = result.program else {
        return;
    };
    let (world, _, report) = registered(&[ScriptDeclarations {
        module: "ai/far.erg".to_owned(),
        components: program.declared,
    }]);
    assert_eq!(
        (
            world.components().key_named("Far").is_some(),
            world.components().key_named("Far#state").is_some()
        ),
        (true, true),
        "the behavior and its companion register together: {report:?}"
    );
}

/// **Stop drops a behavior's state**: the Play snapshot, restored through
/// the service as the editor's Stop does, gives the entity back its behavior
/// and no companion — the next Play starts it afresh.
#[test]
fn stop_restores_the_behavior_and_drops_its_state() {
    use khora_core::scene::{SceneFile, SerializationGoal};
    use khora_io::serialization::SerializationService;
    let (mut world, id) = holding("ai/guard.erg", GUARD, "Guard");
    let service = SerializationService::new();
    let file = service
        .save_world(&world, SerializationGoal::FastestLoad)
        .expect("the snapshot saves");
    let file = SceneFile::from_bytes(&file.to_bytes()).expect("parses");
    service
        .replace_world(&file, &mut world)
        .unwrap_or_else(|e| panic!("the restore failed: {e:?}"));

    let entity = world.entity_with_id(id).expect("the guard is back");
    let key = |name: &str| world.components().key_named(name).expect("registered");
    assert!(
        world.row(entity, key("Guard")).is_some(),
        "the behavior is back"
    );
    assert!(
        world.row(entity, key("Guard#state")).is_none(),
        "its state is not"
    );
}

// ─── The part each plays for scripts ────────────────────────────────────────

fn role(world: &World, name: &str) -> Option<ScriptRole> {
    vtable(world, name).script.clone()
}

/// **The registry knows what a behavior is**: the behavior's component names
/// the module whose program runs it, its companion names the behavior it
/// belongs to, and a component declared beside them plays no part.
#[test]
fn a_behavior_and_its_companion_register_with_their_script_roles() {
    let (world, _, report) = registered(&[declarations(
        "ai/guard.erg",
        &format!("component Loot {{ int value = 1; }}\n{GUARD}"),
    )]);
    assert!(report.refused.is_empty(), "{report:?}");

    assert_eq!(
        role(&world, "Guard"),
        Some(ScriptRole::Behavior {
            module: "ai/guard.erg".to_owned(),
            order: 0,
        })
    );
    assert_eq!(
        role(&world, "Guard#state"),
        Some(ScriptRole::BehaviorState {
            behavior: "Guard".to_owned()
        })
    );
    assert_eq!(role(&world, "Loot"), None, "a component is no behavior");
    assert_eq!(role(&world, "Transform"), None, "nor is a Rust one");
}

/// **A refused behavior lends its role to nothing**: the component another
/// module owns under its name stays a component, while a behavior of a third
/// module registers as one.
#[test]
fn a_refused_behavior_gives_no_role_to_the_component_it_collides_with() {
    let (world, _, report) = registered(&[
        declarations("a.erg", "component Guard { int n = 0; }"),
        declarations("b.erg", GUARD),
        declarations("c.erg", "behavior Sentry { int seen = 0; }"),
    ]);
    assert!(report.refused.contains(&"Guard".to_owned()), "{report:?}");

    assert_eq!(role(&world, "Guard"), None, "`a.erg`'s component");
    assert_eq!(
        role(&world, "Sentry"),
        Some(ScriptRole::Behavior {
            module: "c.erg".to_owned(),
            order: 0,
        })
    );
}

/// **A behavior registered from a file becomes a behavior once its module
/// declares it.** A game save loaded into a fresh world registers `Guard`
/// and its companion from the declarations it carries — which say nothing of
/// scripts — and the registration of the module that declares the behavior
/// adopts both, giving each its role.
#[test]
fn a_behavior_registered_from_a_save_gains_its_role_when_its_module_declares_it() {
    use khora_core::asset::AssetUUID;
    use khora_core::scene::{SceneFile, SerializationGoal};
    use khora_io::serialization::SerializationService;

    let (world, id) = holding("ai/guard.erg", GUARD, "Guard");
    let service = SerializationService::new();
    let on_disk = |file: &SceneFile| SceneFile::from_bytes(&file.to_bytes()).expect("parses");
    let scene = on_disk(
        &service
            .save_world(&world, SerializationGoal::EditorInterchange)
            .expect("the scene saves"),
    );
    let save = on_disk(
        &service
            .save_game(
                &world,
                AssetUUID::new(),
                &scene,
                SerializationGoal::EditorInterchange,
            )
            .expect("the game saves"),
    );

    let mut fresh = World::new();
    service
        .load_game(&mut fresh, &save, &scene)
        .expect("the game loads");
    let guard = fresh.entity_with_id(id).expect("the guard is back");
    assert_eq!(role(&fresh, "Guard"), None, "registered from the file");
    assert_eq!(
        role(&fresh, "Guard#state"),
        None,
        "registered from the file"
    );

    let mut state = DeclaredComponents::default();
    let report = apply_declarations(
        &mut fresh,
        &mut state,
        &[declarations("ai/guard.erg", GUARD)],
    );
    assert!(report.refused.is_empty(), "adopted: {report:?}");

    assert_eq!(
        role(&fresh, "Guard"),
        Some(ScriptRole::Behavior {
            module: "ai/guard.erg".to_owned(),
            order: 0,
        })
    );
    assert_eq!(
        role(&fresh, "Guard#state"),
        Some(ScriptRole::BehaviorState {
            behavior: "Guard".to_owned()
        })
    );
    let key = |name: &str| fresh.components().key_named(name).expect("registered");
    assert!(
        fresh.row(guard, key("Guard")).is_some() && fresh.row(guard, key("Guard#state")).is_some(),
        "the adopted components keep the rows the save loaded"
    );
}

// ─── Registered from the runtime's channel ──────────────────────────────────

/// A runtime holding the declaration channel and the registration's shared
/// memory, as the engine's bootstrap leaves it.
fn registration_runtime() -> khora_core::Runtime {
    use khora_io::script::declarations::declaration_channel;
    use std::sync::{Arc, Mutex};
    let mut runtime = khora_core::Runtime::new();
    runtime.resources.insert(declaration_channel());
    runtime
        .resources
        .insert(Arc::new(Mutex::new(DeclaredComponents::default())));
    runtime
}

/// What `runtime`'s compile path sent and no frame boundary drained yet.
fn send(runtime: &khora_core::Runtime, declarations: ScriptDeclarations) {
    runtime
        .resources
        .get::<khora_io::script::declarations::PendingDeclarations>()
        .expect("the runtime holds the declaration channel")
        .send(declarations);
}

/// The registration memory `runtime` shares.
fn shared_state(
    runtime: &khora_core::Runtime,
) -> std::sync::Arc<std::sync::Mutex<DeclaredComponents>> {
    runtime
        .resources
        .get::<std::sync::Arc<std::sync::Mutex<DeclaredComponents>>>()
        .cloned()
        .expect("the runtime holds the registration state")
}

/// **What the compile path sent is registered on demand**, before any frame
/// boundary: the behavior and its companion are in the world with their
/// roles, and the runtime's shared memory knows which module declares them.
#[test]
fn pending_declarations_are_registered_with_their_roles() {
    use khora_io::script::declarations::register_pending;
    let runtime = registration_runtime();
    send(&runtime, declarations("ai/guard.erg", GUARD));
    let mut world = World::new();

    let report = register_pending(&mut world, &runtime);

    assert_eq!(
        report.registered,
        vec!["Guard".to_owned(), "Guard#state".to_owned()]
    );
    assert_eq!(field_names(vtable(&world, "Guard")), vec!["speed", "hp"]);
    assert_eq!(
        role(&world, "Guard"),
        Some(ScriptRole::Behavior {
            module: "ai/guard.erg".to_owned(),
            order: 0,
        })
    );
    assert_eq!(
        role(&world, "Guard#state"),
        Some(ScriptRole::BehaviorState {
            behavior: "Guard".to_owned()
        })
    );
    let state = shared_state(&runtime);
    let state = state.lock().expect("the registration state lock");
    assert_eq!(
        state.owner_of("Guard"),
        Some("ai/guard.erg"),
        "the runtime's shared memory, not a private one"
    );
}

/// **The channel is drained**: called again with nothing sent since, the
/// registration registers nothing — what the frame boundary's data system
/// finds after the bootstrap registered the initial load.
#[test]
fn registering_pending_declarations_drains_the_channel() {
    use khora_io::script::declarations::{register_pending, PendingDeclarations};
    let runtime = registration_runtime();
    send(&runtime, declarations("ai/guard.erg", GUARD));
    let mut world = World::new();
    let first = register_pending(&mut world, &runtime);
    assert!(!first.registered.is_empty(), "{first:?}");

    assert!(
        runtime
            .resources
            .get::<PendingDeclarations>()
            .expect("the channel")
            .is_empty(),
        "nothing left for the frame boundary"
    );
    assert_eq!(
        register_pending(&mut world, &runtime),
        DeclarationReport::default()
    );
}

/// **Its memory lasts across calls**: a module that sends a new set after a
/// first registration relays its behavior out — never adopts it as a stranger,
/// which a registration with a fresh memory each call would.
#[test]
fn a_later_registration_remembers_who_declared_what() {
    use khora_io::script::declarations::register_pending;
    let runtime = registration_runtime();
    let mut world = World::new();
    send(&runtime, declarations("ai/guard.erg", GUARD));
    register_pending(&mut world, &runtime);

    send(
        &runtime,
        declarations(
            "ai/guard.erg",
            "behavior Guard { float speed = 1.5; int hp = 10; bool alert = false; }",
        ),
    );
    let report = register_pending(&mut world, &runtime);

    assert_eq!(report.relaid, vec!["Guard".to_owned()], "{report:?}");
    assert!(report.adopted.is_empty(), "{report:?}");
    assert_eq!(
        field_names(vtable(&world, "Guard")),
        vec!["speed", "hp", "alert"]
    );
}

/// **Nothing pending, nothing done.**
#[test]
fn registering_with_nothing_pending_reports_nothing() {
    use khora_io::script::declarations::register_pending;
    let runtime = registration_runtime();
    let mut world = World::new();

    assert_eq!(
        register_pending(&mut world, &runtime),
        DeclarationReport::default()
    );
}

/// **A runtime with no channel** — an application that mounts no script —
/// registers nothing, and does not panic.
#[test]
fn registering_without_a_declaration_channel_reports_nothing() {
    use khora_io::script::declarations::register_pending;
    let runtime = khora_core::Runtime::new();
    let mut world = World::new();

    assert_eq!(
        register_pending(&mut world, &runtime),
        DeclarationReport::default()
    );
}

/// **A runtime with declarations but no registration memory** registers
/// nothing, and does not panic: there is nowhere to remember who owns what.
#[test]
fn registering_without_a_registration_state_reports_nothing() {
    use khora_io::script::declarations::{declaration_channel, register_pending};
    let mut runtime = khora_core::Runtime::new();
    runtime.resources.insert(declaration_channel());
    send(&runtime, declarations("ai/guard.erg", GUARD));
    let mut world = World::new();

    assert_eq!(
        register_pending(&mut world, &runtime),
        DeclarationReport::default()
    );
    assert!(
        world.components().key_named("Guard").is_none(),
        "nothing registered"
    );
}
