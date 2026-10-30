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

//! A project's own components reach the `World` (Ergon spec 08).
//!
//! Compiled against the mirrors the engine serves, their declarations handed
//! to the frame-boundary registration the way the engine hands them — one
//! `ScriptDeclarations` per declaring module — then written and read by
//! scripts through the applier and the projection, like any component. A
//! reload follows the spec's table: a field added, renamed or retyped, a
//! domain changed, a declaration deleted and declared again.

use std::sync::Arc;

use khora_core::ecs::entity::EntityId;
use khora_core::script::{ComponentName, FrameComponents, ScriptValue, WorldCommand};
use khora_data::ecs::systems::script_commands::apply_all;
use khora_data::ecs::{
    Collider, ComponentKey, ComponentProvenance, SemanticDomain, Transform, World,
};
use khora_io::script::declarations::{
    apply_declarations, declaration_channel, DeclarationReport, DeclaredComponents,
    ScriptDeclarations,
};
use khora_io::script::hot_reload::{load_all, reload_channel};
use khora_io::script::mirror::PreludeLoader;
use khora_script::vm::{Machine, Program, Run, Value};
use khora_script::{compile_module, Diagnostic, Host, MemoryLoader};

const HEALTH: &str = "[Domain(Physics)]
component Health { int current = 100; int max = 100; }";

// ─── Helpers ────────────────────────────────────────────────────────────────

/// `entry` compiled among `modules`, the engine's mirrors available.
fn compile(modules: &[(&str, &str)], entry: &str) -> Program {
    let mut inner = MemoryLoader::new();
    for &(path, source) in modules {
        inner = inner.with(path, source);
    }
    let result = compile_module(&PreludeLoader::new(inner), entry);
    let errors: Vec<&Diagnostic> = result.diagnostics.iter().filter(|d| d.is_error()).collect();
    assert!(errors.is_empty(), "`{entry}` should compile: {errors:?}");
    result.program.expect("a program with no errors")
}

/// What the engine sends for `module` once it compiled `source`: the
/// components that module declares — none, when it declares none.
fn declarations(module: &str, source: &str) -> ScriptDeclarations {
    let program = compile(&[(module, source)], module);
    ScriptDeclarations {
        module: module.to_owned(),
        components: program
            .declared
            .into_iter()
            .filter(|declared| declared.module == module)
            .collect(),
    }
}

/// A world, the registration's own state, and what it did with `batch`.
fn registered(batch: &[ScriptDeclarations]) -> (World, DeclaredComponents, DeclarationReport) {
    let mut world = World::new();
    let mut state = DeclaredComponents::default();
    let report = apply_declarations(&mut world, &mut state, batch);
    (world, state, report)
}

fn key(world: &World, name: &str) -> ComponentKey {
    world
        .components()
        .key_named(name)
        .unwrap_or_else(|| panic!("`{name}` is registered"))
}

/// `entity`'s value of field `field` of component `name`.
fn read(world: &World, entity: EntityId, name: &str, field: &str) -> Option<ScriptValue> {
    let key = world.components().key_named(name)?;
    let slot = world
        .components()
        .vtable(key)?
        .columns
        .packed()?
        .slot_of(field)?;
    world.row(entity, key)?.field(slot)
}

fn fields(pairs: &[(&str, ScriptValue)]) -> ScriptValue {
    ScriptValue::Struct(
        pairs
            .iter()
            .map(|(name, value)| ((*name).to_owned(), value.clone()))
            .collect(),
    )
}

/// Attaches `name` to `entity` with `patch` over its defaults, as a script's
/// `Add` does.
fn attach(world: &mut World, entity: EntityId, name: &str, patch: ScriptValue) {
    let mut buffer = khora_core::script::CommandBuffer::new();
    buffer.push(WorldCommand::AddComponent {
        entity,
        component: ComponentName::new(name),
        value: patch,
    });
    assert_eq!(apply_all(world, &buffer), 1, "`{name}` attaches");
}

/// Runs `member` of `program` as `entity`'s behavior, over what the world
/// projects of `reads`, then applies what it queued. Returns the member's
/// result and how many commands were applied.
fn run(
    world: &mut World,
    program: &Program,
    member: &str,
    entity: EntityId,
    args: &[Value],
) -> (Value, usize) {
    let names: Vec<ComponentName> = program
        .reads
        .iter()
        .map(|name| ComponentName::new(name.as_str()))
        .collect();
    let mut host = Host::new()
        .for_entity(entity)
        .with_reserver(world.reserver());
    host.components = FrameComponents::new(Arc::new(world.project_components(&names)));
    let mut machine = Machine::new(program, member, args).expect("the member exists");
    assert_eq!(machine.run(program, &mut host, u64::MAX), Run::Completed);
    let result = machine.result();
    let queued = host.end_frame();
    world.settle_reservations();
    (result, apply_all(world, &queued))
}

/// Every domain's epoch, in `SemanticDomain::index` order.
fn epochs(world: &World) -> Vec<u64> {
    [
        SemanticDomain::Spatial,
        SemanticDomain::Render,
        SemanticDomain::Audio,
        SemanticDomain::Physics,
        SemanticDomain::Ui,
        SemanticDomain::Script,
    ]
    .iter()
    .map(|domain| world.domain_epoch(*domain))
    .collect()
}

// ─── Registration ───────────────────────────────────────────────────────────

/// **`[Domain(Physics)]` places the data**: the component is registered under
/// its name's key, in the Physics domain, authored (a scene keeps it); a row
/// of it is a Physics row — a write bumps Physics, not Script.
#[test]
fn a_script_component_lives_in_its_declared_domain() {
    let (mut world, _, report) = registered(&[declarations("game/health.erg", HEALTH)]);

    assert_eq!(report.registered, vec!["Health".to_owned()]);
    let health = key(&world, "Health");
    assert_eq!(health, ComponentKey::named("Health"));
    let vtable = world.components().vtable(health).expect("registered");
    assert_eq!(vtable.domain, SemanticDomain::Physics);
    assert_eq!(vtable.provenance, ComponentProvenance::Authored);
    assert!(vtable.rust.is_none(), "no Rust type behind it");

    let entity = world.spawn(Transform::identity());
    attach(&mut world, entity, "Health", ScriptValue::Unit);
    let (physics, script) = (
        world.domain_epoch(SemanticDomain::Physics),
        world.domain_epoch(SemanticDomain::Script),
    );
    let slot = vtable_slot(&world, "Health", "current");
    world
        .row_mut(entity, health)
        .expect("the entity holds Health")
        .set_field(slot, &ScriptValue::Int(3))
        .expect("written");
    assert!(world.domain_epoch(SemanticDomain::Physics) > physics);
    assert_eq!(world.domain_epoch(SemanticDomain::Script), script);
}

fn vtable_slot(world: &World, name: &str, field: &str) -> usize {
    world
        .components()
        .vtable(key(world, name))
        .and_then(|vtable| vtable.columns.packed()?.slot_of(field))
        .unwrap_or_else(|| panic!("`{name}.{field}` exists"))
}

/// Without `[Domain]`, the neutral domain: `Script`.
#[test]
fn without_domain_it_goes_to_script() {
    let (world, _, _) = registered(&[declarations(
        "game/loot.erg",
        "component Loot { string item = \"\"; int value = 0; }",
    )]);

    let vtable = world
        .components()
        .vtable(key(&world, "Loot"))
        .expect("registered");
    assert_eq!(vtable.domain, SemanticDomain::Script);
}

/// **The fields, kinds and defaults are the declaration's**: a new row holds
/// each field's default, the zero where none was written.
#[test]
fn a_registered_component_has_the_declared_fields() {
    let (mut world, _, _) = registered(&[declarations(
        "game/loot.erg",
        "component Loot { string item = \"coin\"; int value; float weight = 0.5; int[] tags; }",
    )]);

    let entity = world.spawn(Transform::identity());
    attach(&mut world, entity, "Loot", ScriptValue::Unit);
    assert_eq!(
        read(&world, entity, "Loot", "item"),
        Some(ScriptValue::Str("coin".to_owned()))
    );
    assert_eq!(
        read(&world, entity, "Loot", "value"),
        Some(ScriptValue::Int(0))
    );
    assert_eq!(
        read(&world, entity, "Loot", "weight"),
        Some(ScriptValue::Float(0.5))
    );
    assert_eq!(
        read(&world, entity, "Loot", "tags"),
        Some(ScriptValue::Array(Vec::new()))
    );
}

/// **No limit on how many a project declares**: 5,000 declarations compile,
/// register, and are writable through the applier.
#[test]
fn a_project_can_declare_many_components() {
    const COUNT: usize = 5_000;
    let source: String = (0..COUNT)
        .map(|i| format!("component Generated{i} {{ int n = {i}; }}\n"))
        .collect();
    let batch = declarations("game/generated.erg", &source);
    assert_eq!(batch.components.len(), COUNT, "every declaration compiled");

    let (mut world, _, report) = registered(&[batch]);
    assert_eq!(report.registered.len(), COUNT);

    let entity = world.spawn(Transform::identity());
    let mut buffer = khora_core::script::CommandBuffer::new();
    for i in 0..COUNT {
        buffer.push(WorldCommand::AddComponent {
            entity,
            component: ComponentName::new(format!("Generated{i}").as_str()),
            value: fields(&[("n", ScriptValue::Int(2 * i as i64))]),
        });
    }
    assert_eq!(
        apply_all(&mut world, &buffer),
        COUNT,
        "every one is written"
    );
    for i in [0, 1, COUNT / 2, COUNT - 1] {
        assert_eq!(
            read(&world, entity, &format!("Generated{i}"), "n"),
            Some(ScriptValue::Int(2 * i as i64)),
            "Generated{i}"
        );
    }
}

/// **A variable-size field moves with its row.** The entity's Physics row
/// migrates — a Rust Physics-domain component is added, then removed — and
/// the text stays.
#[test]
fn a_string_field_survives_a_page_migration() {
    let (mut world, _, _) = registered(&[declarations(
        "game/tag.erg",
        "[Domain(Physics)]\ncomponent Inscription { string label = \"none\"; int n = 0; }",
    )]);
    let neighbour = world.spawn(Transform::identity());
    attach(
        &mut world,
        neighbour,
        "Inscription",
        fields(&[("label", ScriptValue::Str("shield".to_owned()))]),
    );
    let entity = world.spawn(Transform::identity());
    attach(
        &mut world,
        entity,
        "Inscription",
        fields(&[
            (
                "label",
                ScriptValue::Str("sword of a thousand cuts".to_owned()),
            ),
            ("n", ScriptValue::Int(3)),
        ]),
    );

    world
        .add_component(entity, Collider::default())
        .expect("Collider attaches");
    assert_eq!(
        read(&world, entity, "Inscription", "label"),
        Some(ScriptValue::Str("sword of a thousand cuts".to_owned()))
    );
    world
        .remove_component::<Collider>(entity)
        .expect("Collider detaches");
    assert_eq!(
        read(&world, entity, "Inscription", "label"),
        Some(ScriptValue::Str("sword of a thousand cuts".to_owned()))
    );
    assert_eq!(
        read(&world, entity, "Inscription", "n"),
        Some(ScriptValue::Int(3))
    );
    assert_eq!(
        read(&world, neighbour, "Inscription", "label"),
        Some(ScriptValue::Str("shield".to_owned())),
        "the row left behind is intact"
    );
}

// ─── Written and read like any component ────────────────────────────────────

/// `Health` declared in its module, and a behavior using it, compiled as the
/// engine compiles them; `Health` registered.
fn medic(behavior: &str) -> (World, Program) {
    let source =
        format!("import \"engine/components.erg\";\nimport \"game/health.erg\";\n{behavior}");
    let modules = [
        ("game/health.erg", HEALTH),
        ("game/medic.erg", source.as_str()),
    ];
    let program = compile(&modules, "game/medic.erg");
    let (world, _, _) = registered(&[declarations("game/health.erg", HEALTH)]);
    (world, program)
}

/// **`this.Add(Health { })` starts from the declared defaults.**
#[test]
fn add_applies_declared_defaults() {
    let (mut world, program) = medic("behavior Medic { void Go() { this.Add(Health { }); } }");
    let entity = world.spawn(Transform::identity());

    let (_, applied) = run(&mut world, &program, "Medic.Go", entity, &[]);

    assert_eq!(applied, 1);
    assert_eq!(
        read(&world, entity, "Health", "current"),
        Some(ScriptValue::Int(100))
    );
    assert_eq!(
        read(&world, entity, "Health", "max"),
        Some(ScriptValue::Int(100))
    );
}

/// **`this.Set(Health { current: 5 })` is a patch**: `max` is left as it was.
#[test]
fn set_merges_into_a_script_component() {
    let (mut world, program) =
        medic("behavior Medic { void Go() { this.Set(Health { current: 5 }); } }");
    let entity = world.spawn(Transform::identity());
    attach(
        &mut world,
        entity,
        "Health",
        fields(&[
            ("current", ScriptValue::Int(60)),
            ("max", ScriptValue::Int(80)),
        ]),
    );

    let (_, applied) = run(&mut world, &program, "Medic.Go", entity, &[]);

    assert_eq!(applied, 1);
    assert_eq!(
        read(&world, entity, "Health", "current"),
        Some(ScriptValue::Int(5))
    );
    assert_eq!(
        read(&world, entity, "Health", "max"),
        Some(ScriptValue::Int(80)),
        "the field not named is intact"
    );
}

#[test]
fn remove_detaches() {
    let (mut world, program) = medic("behavior Medic { void Go() { this.Remove(Health); } }");
    let entity = world.spawn(Transform::identity());
    attach(&mut world, entity, "Health", ScriptValue::Unit);

    let (_, applied) = run(&mut world, &program, "Medic.Go", entity, &[]);

    assert_eq!(applied, 1);
    assert!(world.row(entity, key(&world, "Health")).is_none());
    assert!(world.contains(entity), "the entity stays");
    assert_eq!(world.get::<Transform>(entity), Some(&Transform::identity()));
}

/// **One `Spawn`, both kinds**: an engine component and a script one on the
/// same new entity, each with its patch over its defaults.
#[test]
fn spawn_mixes_engine_and_script_components() {
    let (mut world, program) = medic(
        "behavior Medic {
             void Go() { Spawn(Vec3(1.0, 2.0, 3.0), Collider { friction: 0.8 }, Health { current: 7 }); }
         }",
    );
    let spawner = world.spawn(Transform::identity());

    let (_, applied) = run(&mut world, &program, "Medic.Go", spawner, &[]);

    assert_eq!(applied, 1);
    let spawned: Vec<EntityId> = world.iter_entities().filter(|e| *e != spawner).collect();
    assert_eq!(spawned.len(), 1, "one entity was spawned");
    let entity = spawned[0];
    assert_eq!(world.get::<Collider>(entity).map(|c| c.friction), Some(0.8));
    assert_eq!(
        read(&world, entity, "Health", "current"),
        Some(ScriptValue::Int(7))
    );
    assert_eq!(
        read(&world, entity, "Health", "max"),
        Some(ScriptValue::Int(100))
    );
}

/// **`e.Get(Health)` reads the row**, through the projection scripts read.
#[test]
fn get_reads_a_script_component() {
    let (mut world, program) = medic(
        "fn int Vitals(Entity e) {
             if (var h = e.Get(Health)) { return h.current * 1000 + h.max; }
             return -1;
         }",
    );
    let entity = world.spawn(Transform::identity());
    attach(
        &mut world,
        entity,
        "Health",
        fields(&[("current", ScriptValue::Int(42))]),
    );
    let bare = world.spawn(Transform::identity());

    let (vitals, _) = run(
        &mut world,
        &program,
        "Vitals",
        entity,
        &[Value::Entity(entity)],
    );
    assert_eq!(vitals, Value::Int(42_100));
    let (absent, _) = run(&mut world, &program, "Vitals", bare, &[Value::Entity(bare)]);
    assert_eq!(absent, Value::Int(-1));
}

// ─── Reload: one test per row of the spec's table ───────────────────────────

/// A world where `Health` was registered from `before` and two entities hold
/// it — one written away from the defaults.
fn reloading(before: &str) -> (World, DeclaredComponents, EntityId, EntityId) {
    let (mut world, state, _) = registered(&[declarations("game/health.erg", before)]);
    let written = world.spawn(Transform::identity());
    attach(
        &mut world,
        written,
        "Health",
        fields(&[("current", ScriptValue::Int(42))]),
    );
    let fresh = world.spawn(Transform::identity());
    attach(&mut world, fresh, "Health", ScriptValue::Unit);
    (world, state, written, fresh)
}

fn reload(world: &mut World, state: &mut DeclaredComponents, after: &str) -> DeclarationReport {
    apply_declarations(world, state, &[declarations("game/health.erg", after)])
}

/// **Adds a field** → existing rows get its default.
#[test]
fn a_reload_adding_a_field_gives_existing_rows_its_default() {
    let (mut world, mut state, written, fresh) = reloading(HEALTH);

    let report = reload(
        &mut world,
        &mut state,
        "[Domain(Physics)]
         component Health { int current = 100; int max = 100; int armor = 5; }",
    );

    assert_eq!(report.relaid, vec!["Health".to_owned()]);
    for entity in [written, fresh] {
        assert_eq!(
            read(&world, entity, "Health", "armor"),
            Some(ScriptValue::Int(5))
        );
    }
    assert_eq!(
        read(&world, written, "Health", "current"),
        Some(ScriptValue::Int(42)),
        "the other fields keep their values"
    );
}

/// **Renames a field** → the old value is dropped; the new field takes its
/// default.
#[test]
fn a_reload_renaming_a_field_drops_the_old_value() {
    let (mut world, mut state, written, _) = reloading(HEALTH);

    reload(
        &mut world,
        &mut state,
        "[Domain(Physics)]
         component Health { int hp = 100; int max = 100; }",
    );

    assert_eq!(read(&world, written, "Health", "current"), None, "gone");
    assert_eq!(
        read(&world, written, "Health", "hp"),
        Some(ScriptValue::Int(100)),
        "the new name takes its default"
    );
    assert_eq!(
        read(&world, written, "Health", "max"),
        Some(ScriptValue::Int(100))
    );
}

/// **Changes a field's type** → as a rename: the value is dropped for the new
/// default.
#[test]
fn a_reload_changing_a_field_type_takes_the_default() {
    let (mut world, mut state, written, _) = reloading(HEALTH);

    reload(
        &mut world,
        &mut state,
        "[Domain(Physics)]
         component Health { string current = \"full\"; int max = 100; }",
    );

    assert_eq!(
        read(&world, written, "Health", "current"),
        Some(ScriptValue::Str("full".to_owned()))
    );
}

/// **…unless it converts losslessly**: `int` → `float` keeps the value.
#[test]
fn a_reload_from_int_to_float_keeps_the_value() {
    let (mut world, mut state, written, fresh) = reloading(HEALTH);

    reload(
        &mut world,
        &mut state,
        "[Domain(Physics)]
         component Health { float current = 1.5; int max = 100; }",
    );

    assert_eq!(
        read(&world, written, "Health", "current"),
        Some(ScriptValue::Float(42.0))
    );
    assert_eq!(
        read(&world, fresh, "Health", "current"),
        Some(ScriptValue::Float(100.0)),
        "a row at the old default keeps that value, converted"
    );
}

/// **Changes `[Domain]`** → every row moves to the new domain's page, values
/// intact, the component keeping its key.
#[test]
fn a_reload_changing_the_domain_moves_the_rows() {
    let (mut world, mut state, written, fresh) = reloading(HEALTH);
    let before = key(&world, "Health");

    let report = reload(
        &mut world,
        &mut state,
        "[Domain(Ui)]
         component Health { int current = 100; int max = 100; }",
    );

    assert_eq!(report.moved, vec!["Health".to_owned()]);
    assert_eq!(key(&world, "Health"), before, "the same component");
    assert_eq!(
        world
            .components()
            .vtable(before)
            .expect("registered")
            .domain,
        SemanticDomain::Ui
    );
    assert_eq!(
        read(&world, written, "Health", "current"),
        Some(ScriptValue::Int(42))
    );
    assert_eq!(
        read(&world, fresh, "Health", "current"),
        Some(ScriptValue::Int(100))
    );
}

/// **Deletes the declaration** → the component stays registered with its
/// data, reported orphaned. Never silently deleted.
#[test]
fn a_deleted_declaration_leaves_the_component_orphaned() {
    let (mut world, mut state, written, _) = reloading(HEALTH);

    let report = reload(&mut world, &mut state, "fn void NoMoreHealth() { }");

    assert_eq!(report.orphaned, vec!["Health".to_owned()]);
    assert!(world.components().key_named("Health").is_some());
    assert_eq!(
        read(&world, written, "Health", "current"),
        Some(ScriptValue::Int(42)),
        "the data stays"
    );

    // The file itself deleted: the same empty set, nothing more to orphan.
    let again = apply_declarations(
        &mut world,
        &mut state,
        &[ScriptDeclarations {
            module: "game/health.erg".to_owned(),
            components: Vec::new(),
        }],
    );
    assert!(again.orphaned.is_empty(), "already orphaned: {again:?}");
    assert_eq!(
        read(&world, written, "Health", "current"),
        Some(ScriptValue::Int(42))
    );
}

/// **Declaring an orphan again adopts it** — from any module — relaid out to
/// the new declaration, its data kept.
#[test]
fn redeclaring_an_orphan_adopts_it() {
    let (mut world, mut state, written, _) = reloading(HEALTH);
    reload(&mut world, &mut state, "fn void NoMoreHealth() { }");

    let report = apply_declarations(
        &mut world,
        &mut state,
        &[declarations(
            "game/vitals.erg",
            "[Domain(Physics)]
             component Health { int current = 100; int max = 100; int armor = 5; }",
        )],
    );

    assert_eq!(report.adopted, vec!["Health".to_owned()]);
    assert!(report.registered.is_empty(), "not registered twice");
    assert_eq!(
        read(&world, written, "Health", "current"),
        Some(ScriptValue::Int(42))
    );
    assert_eq!(
        read(&world, written, "Health", "armor"),
        Some(ScriptValue::Int(5))
    );

    // Its new module owns it: that module dropping it orphans it again.
    let dropped = reload_module(&mut world, &mut state, "game/vitals.erg");
    assert_eq!(dropped.orphaned, vec!["Health".to_owned()]);
}

/// `module` recompiled without any component.
fn reload_module(
    world: &mut World,
    state: &mut DeclaredComponents,
    module: &str,
) -> DeclarationReport {
    apply_declarations(
        world,
        state,
        &[declarations(module, "fn void Nothing() { }")],
    )
}

/// **A component moving to another module in one frame is not orphaned**:
/// the module that dropped it and the one that took it arrive together.
#[test]
fn a_component_moving_between_modules_in_one_batch_stays_declared() {
    let (mut world, mut state, written, _) = reloading(HEALTH);

    for batch in [
        [
            declarations("game/vitals.erg", HEALTH),
            declarations("game/health.erg", "fn void Moved() { }"),
        ],
        [
            declarations("game/health.erg", HEALTH),
            declarations("game/vitals.erg", "fn void Moved() { }"),
        ],
    ] {
        let report = apply_declarations(&mut world, &mut state, &batch);
        assert!(report.orphaned.is_empty(), "{report:?}");
        assert_eq!(
            read(&world, written, "Health", "current"),
            Some(ScriptValue::Int(42))
        );
    }
}

/// **The same declaration again changes nothing** — no relayout, no move, no
/// epoch bumped, so nothing projected from it is invalidated.
#[test]
fn an_unchanged_declaration_bumps_no_epoch() {
    let (mut world, mut state, written, _) = reloading(HEALTH);
    let before = epochs(&world);

    let report = reload(&mut world, &mut state, HEALTH);

    assert_eq!(report, DeclarationReport::default());
    assert_eq!(epochs(&world), before);
    assert_eq!(
        read(&world, written, "Health", "current"),
        Some(ScriptValue::Int(42))
    );
}

// ─── What the engine sends ──────────────────────────────────────────────────

/// **The mount sends each declaring module's components once**, whichever
/// modules import it; a module that declares nothing sends nothing. The two
/// importers write `Health` from plain functions — a behavior is a component
/// of its own, and would be a declaration of theirs.
#[test]
fn load_all_sends_each_declaring_module_once() {
    let dir = tempfile::TempDir::new().expect("a temp dir");
    std::fs::create_dir_all(dir.path().join("game")).expect("makes a subdir");
    std::fs::write(dir.path().join("game/health.erg"), HEALTH).expect("writes");
    for (path, function) in [("game/medic.erg", "Treat"), ("game/nurse.erg", "Tend")] {
        std::fs::write(
            dir.path().join(path),
            format!(
                "import \"game/health.erg\";
                 fn void {function}(Entity e) {{ e.Add(Health {{ }}); }}"
            ),
        )
        .expect("writes");
    }
    std::fs::write(
        dir.path().join("rules.erg"),
        "fn int Double(int n) { return n * 2; }",
    )
    .expect("writes");

    let pending = reload_channel();
    let declared = declaration_channel();
    assert_eq!(
        load_all(dir.path(), &pending, &declared),
        4,
        "every module compiled"
    );

    let sent: Vec<(String, Vec<String>)> = declared
        .drain()
        .into_iter()
        .map(|batch| {
            (
                batch.module,
                batch.components.into_iter().map(|c| c.name).collect(),
            )
        })
        .collect();
    assert_eq!(
        sent,
        vec![("game/health.erg".to_owned(), vec!["Health".to_owned()])],
        "one set, from the module that declares it"
    );
}

// ─── Ownership between modules ──────────────────────────────────────────────

/// **A component one module owns is not re-declared from under it by
/// another.** Two modules that do not import each other each compile alone,
/// so `report_name_clashes` never sees both: the registration is the only
/// place that can tell `game/rival.erg`'s `Health` from `game/health.erg`'s.
/// Taking the rival's layout silently would drop every `current` written.
#[test]
fn a_second_module_declaring_an_owned_name_does_not_relayout_it() {
    let (mut world, mut state, written, _) = reloading(HEALTH);

    apply_declarations(
        &mut world,
        &mut state,
        &[declarations(
            "game/rival.erg",
            "component Health { string label = \"rival\"; }",
        )],
    );

    assert_eq!(
        read(&world, written, "Health", "current"),
        Some(ScriptValue::Int(42)),
        "the owner's data is not dropped for another module's declaration"
    );
    assert_eq!(
        world
            .components()
            .vtable(key(&world, "Health"))
            .expect("registered")
            .domain,
        SemanticDomain::Physics,
        "nor moved to the other module's domain"
    );
    assert_eq!(state.owner_of("Health"), Some("game/health.erg"));
}

// ─── Reload table, combined and edge rows ───────────────────────────────────

/// **A domain change and a field change in one reload**: the rows move and
/// are relaid out — the kept fields keep their values, the new one takes its
/// default, all in the new domain.
#[test]
fn a_reload_changing_domain_and_fields_together_keeps_the_values() {
    let (mut world, mut state, written, fresh) = reloading(HEALTH);

    let report = reload(
        &mut world,
        &mut state,
        "[Domain(Ui)]
         component Health { float current = 1.0; int max = 100; int armor = 5; }",
    );

    assert_eq!(report.moved, vec!["Health".to_owned()]);
    assert_eq!(report.relaid, vec!["Health".to_owned()]);
    assert_eq!(
        read(&world, written, "Health", "current"),
        Some(ScriptValue::Float(42.0))
    );
    assert_eq!(
        read(&world, fresh, "Health", "current"),
        Some(ScriptValue::Float(100.0))
    );
    for entity in [written, fresh] {
        assert_eq!(
            read(&world, entity, "Health", "armor"),
            Some(ScriptValue::Int(5))
        );
    }
}

/// **A moved row goes with its entity**: despawned after the move, it is
/// gone; a new entity in the recycled slot holds no `Health`.
#[test]
fn a_moved_row_is_despawned_with_its_entity() {
    let (mut world, mut state, written, fresh) = reloading(HEALTH);
    reload(
        &mut world,
        &mut state,
        "[Domain(Script)]
         component Health { int current = 100; int max = 100; }",
    );

    assert!(world.despawn(written));
    let recycled = world.spawn(Transform::identity());
    assert!(world.row(recycled, key(&world, "Health")).is_none());
    assert_eq!(
        read(&world, fresh, "Health", "current"),
        Some(ScriptValue::Int(100))
    );
}

/// **`Get` reads a component after its domain moved**, through the
/// projection a script reads.
#[test]
fn get_reads_a_component_whose_domain_moved() {
    let (mut world, program) = medic(
        "fn int Vitals(Entity e) {
             if (var h = e.Get(Health)) { return h.current * 1000 + h.max; }
             return -1;
         }",
    );
    let mut state = DeclaredComponents::default();
    apply_declarations(
        &mut world,
        &mut state,
        &[declarations("game/health.erg", HEALTH)],
    );
    let entity = world.spawn(Transform::identity());
    attach(
        &mut world,
        entity,
        "Health",
        fields(&[("current", ScriptValue::Int(42))]),
    );
    reload(
        &mut world,
        &mut state,
        "[Domain(Render)]
         component Health { int current = 100; int max = 100; }",
    );

    let (vitals, _) = run(
        &mut world,
        &program,
        "Vitals",
        entity,
        &[Value::Entity(entity)],
    );
    assert_eq!(vitals, Value::Int(42_100));
}

/// **A component registered without a script** — the way a file registers
/// one it holds — is adopted by the first module declaring it, relaid out.
#[test]
fn a_component_registered_without_a_script_is_adopted() {
    let mut world = World::new();
    let health = world
        .register_runtime_component(khora_data::ecs::RuntimeComponentDecl {
            name: "Health".to_owned(),
            domain: SemanticDomain::Physics,
            provenance: ComponentProvenance::Authored,
            resumable: false,
            layout: khora_data::ecs::PackedLayout::new(vec![khora_data::ecs::PackedField {
                name: "current".to_owned(),
                kind: khora_data::ecs::FieldKind::Int,
                default: ScriptValue::Int(1),
            }])
            .expect("a layout"),
        })
        .expect("registered");
    let entity = world.spawn(Transform::identity());
    world
        .add_runtime_component(entity, health, &fields(&[("current", ScriptValue::Int(9))]))
        .expect("attached");
    let mut state = DeclaredComponents::default();

    let report = apply_declarations(
        &mut world,
        &mut state,
        &[declarations("game/health.erg", HEALTH)],
    );

    assert_eq!(report.adopted, vec!["Health".to_owned()]);
    assert_eq!(report.relaid, vec!["Health".to_owned()]);
    assert_eq!(
        read(&world, entity, "Health", "current"),
        Some(ScriptValue::Int(9))
    );
    assert_eq!(
        read(&world, entity, "Health", "max"),
        Some(ScriptValue::Int(100))
    );
    assert_eq!(state.owner_of("Health"), Some("game/health.erg"));
}

// ─── Field kinds through the applier and Get ────────────────────────────────

/// **Struct, array and optional fields** go through `Add` and come back
/// through `Get`: the struct default is whole (its own defaults filled), the
/// array keeps its elements, the optional starts at `null`.
#[test]
fn struct_array_and_optional_fields_round_trip_through_add_and_get() {
    let declaring = "struct Point { int x; int y = 7; }
        component Route { Point start = Point { x: 1 }; Point[] stops; string? note; }";
    let user = "import \"game/route.erg\";
        fn int Probe(Entity e) {
            if (var r = e.Get(Route)) {
                var n = r.note ?? \"none\";
                if (n != \"none\") { return -2; }
                return r.start.x * 1000 + r.start.y * 100 + r.stops[0].y * 10 + r.stops[0].x;
            }
            return -1;
        }
        behavior Planner { void Go() { this.Add(Route { stops: [Point { x: 2 }] }); } }";
    let program = compile(
        &[("game/route.erg", declaring), ("game/user.erg", user)],
        "game/user.erg",
    );
    let batch = declarations("game/route.erg", declaring);
    let start = batch.components[0]
        .fields
        .iter()
        .find(|field| field.name == "start")
        .expect("start is declared")
        .default
        .clone();
    assert_eq!(
        start,
        fields(&[("x", ScriptValue::Int(1)), ("y", ScriptValue::Int(7))]),
        "a struct default is completed with the struct's own defaults"
    );
    let (mut world, _, _) = registered(&[batch]);
    let entity = world.spawn(Transform::identity());
    let (_, applied) = run(&mut world, &program, "Planner.Go", entity, &[]);
    assert_eq!(applied, 1);

    let (probe, _) = run(
        &mut world,
        &program,
        "Probe",
        entity,
        &[Value::Entity(entity)],
    );
    assert_eq!(probe, Value::Int(1000 + 700 + 70 + 2));
}

/// **A `Duration` field** is stored as the seconds it names and read back as
/// a duration a script compares.
#[test]
fn a_duration_field_reads_back_as_its_seconds() {
    let declaring = "component Fuse { Duration delay = 500ms; Angle turn = 90deg; }";
    let user = "import \"game/fuse.erg\";
        fn bool Probe(Entity e) {
            if (var f = e.Get(Fuse)) { return f.delay > 400ms && f.delay < 600ms; }
            return false;
        }
        behavior Lighter { void Go() { this.Add(Fuse { }); } }";
    let program = compile(
        &[("game/fuse.erg", declaring), ("game/user.erg", user)],
        "game/user.erg",
    );
    let (mut world, _, _) = registered(&[declarations("game/fuse.erg", declaring)]);
    let entity = world.spawn(Transform::identity());
    run(&mut world, &program, "Lighter.Go", entity, &[]);

    assert_eq!(
        read(&world, entity, "Fuse", "delay"),
        Some(ScriptValue::Float(0.5))
    );
    let (probe, _) = run(
        &mut world,
        &program,
        "Probe",
        entity,
        &[Value::Entity(entity)],
    );
    assert_eq!(probe, Value::Bool(true));
}

// ─── What the hot-reload pump sends ─────────────────────────────────────────

/// Runs the hot-reload pump over `runtime` until it has recompiled something,
/// or a few seconds passed: the watcher reports from its own thread.
fn pump_until_recompiled(runtime: &khora_core::Runtime) {
    let pump = inventory::iter::<khora_data::ecs::DataSystemRegistration>
        .into_iter()
        .find(|system| system.name == "script_hot_reload")
        .expect("the pump is registered");
    let pending = runtime
        .resources
        .get::<khora_io::script::hot_reload::PendingReloads>()
        .expect("a reload channel");
    let mut world = World::new();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    while pending.is_empty() && std::time::Instant::now() < deadline {
        (pump.run)(
            &mut world,
            runtime,
            &mut khora_core::lane::OutputDeck::new(),
            khora_data::ecs::SystemContext {
                since: khora_data::ecs::Tick(0),
            },
        );
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
    assert!(!pending.is_empty(), "the pump recompiled nothing");
}

/// **An importer's components follow a struct it imports.** `Route.start`
/// is a `Point`, declared in another module; editing `Point`'s defaults
/// recompiles `route.erg` as an importer — and the default a new `Route` row
/// starts at must be the new one, or the registration keeps a `Point` the
/// scripts no longer have.
#[test]
fn an_edit_to_an_imported_struct_reaches_the_importers_components() {
    let assets = tempfile::TempDir::new().expect("a temp dir");
    let game = assets.path().join("scripts/game");
    std::fs::create_dir_all(&game).expect("makes the script root");
    let shapes = game.join("shapes.erg");
    std::fs::write(&shapes, "struct Point { int x; int y = 7; }").expect("writes");
    std::fs::write(
        game.join("route.erg"),
        "import \"game/shapes.erg\";
         component Route { Point start = Point { x: 1 }; }",
    )
    .expect("writes");

    // The mount, then the registration of what it sent.
    let declared = declaration_channel();
    load_all(&assets.path().join("scripts"), &reload_channel(), &declared);
    let mut world = World::new();
    let mut state = DeclaredComponents::default();
    apply_declarations(&mut world, &mut state, &declared.drain());

    let mut runtime = khora_core::Runtime::default();
    let watch = khora_io::asset::AssetWatch::default();
    watch.watch(assets.path()).expect("watches the assets");
    runtime.resources.insert(Arc::new(watch));
    runtime.resources.insert(reload_channel());
    runtime.resources.insert(declared.clone());
    std::fs::write(&shapes, "struct Point { int x; int y = 9; }").expect("edits");
    pump_until_recompiled(&runtime);
    apply_declarations(&mut world, &mut state, &declared.drain());

    let route = key(&world, "Route");
    let layout = world
        .components()
        .vtable(route)
        .and_then(|vtable| vtable.columns.packed())
        .expect("Route is declared");
    let start = &layout.fields()[layout.slot_of("start").expect("start")].default;
    assert_eq!(
        start,
        &fields(&[("x", ScriptValue::Int(1)), ("y", ScriptValue::Int(9))]),
        "a new Route starts at the Point the scripts now declare"
    );
}

// ─── Retyping a field the storage holds as a value ──────────────────────────

/// A world where `Badge` (`string label`, `string? note`) is registered from
/// `game/badge.erg` and one entity holds it, both fields written.
fn tagged() -> (World, DeclaredComponents, EntityId) {
    let (mut world, state, _) = registered(&[declarations(
        "game/badge.erg",
        "component Badge { string label = \"none\"; string? note; }",
    )]);
    let entity = world.spawn(Transform::identity());
    attach(
        &mut world,
        entity,
        "Badge",
        fields(&[
            ("label", ScriptValue::Str("sword".to_owned())),
            ("note", ScriptValue::Str("sharp".to_owned())),
        ]),
    );
    (world, state, entity)
}

/// **Retyping `string` to `int[]` is a retype**: both are stored out of line,
/// as one kind, but a script reading `label` now expects an array — the text
/// must not survive into it.
#[test]
fn a_reload_retyping_a_text_field_to_an_array_takes_the_default() {
    let (mut world, mut state, entity) = tagged();

    apply_declarations(
        &mut world,
        &mut state,
        &[declarations(
            "game/badge.erg",
            "component Badge { int[] label; string? note; }",
        )],
    );

    assert_eq!(
        read(&world, entity, "Badge", "label"),
        Some(ScriptValue::Array(Vec::new())),
        "an `int[]` field does not hold the text it held as a `string`"
    );
}

/// **Retyping `string?` to `int?` is a retype too** — though the kind and the
/// default (`null`) are the same, so nothing the registration compares
/// changed. A script reading `note` as an `int?` must not find text.
#[test]
fn a_reload_retyping_an_optional_takes_the_default() {
    let (mut world, mut state, entity) = tagged();

    apply_declarations(
        &mut world,
        &mut state,
        &[declarations(
            "game/badge.erg",
            "component Badge { string label = \"none\"; int? note; }",
        )],
    );

    assert_eq!(
        read(&world, entity, "Badge", "note"),
        Some(ScriptValue::Null),
        "an `int?` field does not hold the text it held as a `string?`"
    );
}

/// **`int` → `float` keeps a value only when it converts losslessly.** A
/// `float` holds every int up to 2^24 exactly; past it, the conversion
/// rounds, and the spec treats that value as a retype.
#[test]
fn a_reload_from_int_to_float_does_not_round_a_value() {
    let (mut world, mut state, written, _) = reloading(HEALTH);
    let large = world.spawn(Transform::identity());
    attach(
        &mut world,
        large,
        "Health",
        fields(&[("current", ScriptValue::Int(16_777_217))]),
    );

    reload(
        &mut world,
        &mut state,
        "[Domain(Physics)]
         component Health { float current = 1.5; int max = 100; }",
    );

    assert_eq!(
        read(&world, written, "Health", "current"),
        Some(ScriptValue::Float(42.0)),
        "a small int converts losslessly"
    );
    assert_ne!(
        read(&world, large, "Health", "current"),
        Some(ScriptValue::Float(16_777_216.0)),
        "16_777_217 has no float: it is not silently rounded"
    );
}

/// **An int written for a float inside an optional or an array is stored as
/// that float**: `float? level = 1` and `float[] steps = [1, 3]` start a row
/// at `1.0` and `[1.0, 3.0]`, as a plain `float` field's int default does —
/// a script reading them as floats must not divide integers.
#[test]
fn int_defaults_inside_optional_and_array_floats_are_stored_as_floats() {
    let declaring = "component Gauge { float? level = 1; float[] steps = [1, 3]; }";
    let user = "import \"game/gauge.erg\";
        fn float Halves(Entity e) {
            if (var g = e.Get(Gauge)) { return (g.level ?? 0.0) / 2 + g.steps[1] / 2; }
            return -1.0;
        }
        behavior Filler { void Go() { this.Add(Gauge { }); } }";
    let program = compile(
        &[("game/gauge.erg", declaring), ("game/user.erg", user)],
        "game/user.erg",
    );
    let (mut world, _, _) = registered(&[declarations("game/gauge.erg", declaring)]);
    let entity = world.spawn(Transform::identity());
    run(&mut world, &program, "Filler.Go", entity, &[]);

    let (halves, _) = run(
        &mut world,
        &program,
        "Halves",
        entity,
        &[Value::Entity(entity)],
    );
    assert_eq!(halves, Value::Float(2.0), "1.0 / 2 + 3.0 / 2");
    assert_eq!(
        read(&world, entity, "Gauge", "level"),
        Some(ScriptValue::Float(1.0))
    );
    assert_eq!(
        read(&world, entity, "Gauge", "steps"),
        Some(ScriptValue::Array(vec![
            ScriptValue::Float(1.0),
            ScriptValue::Float(3.0)
        ]))
    );
}

/// **`Get` right after `Add`, in the same frame, reads the declared
/// defaults** from the world's projection: the row is not attached until the
/// frame's commands apply, so the copy is the component's default row.
#[test]
fn get_after_add_in_one_frame_reads_the_declared_defaults() {
    let (mut world, program) = medic(
        "fn int Fresh(Entity e) {
             e.Add(Health { current: 5 });
             if (var h = e.Get(Health)) { return h.current * 1000 + h.max; }
             return -1;
         }",
    );
    let entity = world.spawn(Transform::identity());

    let (fresh, applied) = run(
        &mut world,
        &program,
        "Fresh",
        entity,
        &[Value::Entity(entity)],
    );

    assert_eq!(applied, 1);
    assert_eq!(fresh, Value::Int(5_100));
}

// ─── A value whose type the storage kind does not show, across a restart ────

/// `Badge` as the scripts declare it once `note` became an `int?`.
const BADGE_SINCE: &str = "component Badge { string label = \"none\"; int? note; }";

/// A script reading `Badge` as `BADGE_SINCE` declares it: `note`, or -1 when
/// it is `null`; -2 when the entity holds no `Badge` a script can read.
fn badge_reader() -> Program {
    compile(
        &[
            ("game/badge.erg", BADGE_SINCE),
            (
                "game/user.erg",
                "import \"game/badge.erg\";
                 fn int Note(Entity e) {
                     if (var b = e.Get(Badge)) { return b.note ?? -1; }
                     return -2;
                 }",
            ),
        ],
        "game/user.erg",
    )
}

/// The scene `tagged()` saves: one entity whose `Badge.note` is the text
/// `"sharp"`, written while `note` was a `string?` — and its identity.
fn badge_scene() -> (
    khora_data::scene::SceneRecord,
    khora_core::ecs::PersistentId,
) {
    let (mut world, _, entity) = tagged();
    world.mark_authored(entity).expect("authored");
    let id = world.persistent_id(entity).expect("an id");
    (
        khora_data::scene::capture_world(&world).expect("the scene captures"),
        id,
    )
}

/// **A scene's text is not read into what the scripts now declare `int?`.**
/// The scene was saved while `Badge.note` was a `string?`; the scripts were
/// edited while the editor was closed. Both types are stored as one kind
/// (`Value`), so the load's kind check takes the text — and a script reading
/// its own `int?` finds a string, or no `Badge` at all. A reload retyping the
/// field resets it (`a_reload_retyping_an_optional_takes_the_default`); a
/// load must not bring back what that reset refused.
#[test]
fn a_scene_value_of_another_type_is_not_read_into_a_script_field() {
    use khora_data::scene::{apply, Identity};
    let (scene, id) = badge_scene();
    let (mut world, _, _) = registered(&[declarations("game/badge.erg", BADGE_SINCE)]);

    let applied = apply(&mut world, &scene, Identity::Keep).expect("the scene loads");
    let entity = world.entity_with_id(id).expect("the entity is back");

    assert_eq!(
        read(&world, entity, "Badge", "note"),
        Some(ScriptValue::Null),
        "an `int?` field does not hold the text the scene saved as a `string?`; report: {:?}",
        applied.report.entries
    );
    let (note, _) = run(
        &mut world,
        &badge_reader(),
        "Note",
        entity,
        &[Value::Entity(entity)],
    );
    assert_eq!(note, Value::Int(-1), "the script reads its `Badge`");
}

/// **The same scene opened before the scripts registered** — loaded in
/// `setup`, or in the editor before the first frame — registers `Badge` from
/// the file. The first declaration the scripts send then adopts it; with no
/// type remembered for it, `note`'s retype goes unseen and the text stays.
#[test]
fn a_component_adopted_from_a_scene_resets_a_retyped_value_field() {
    use khora_data::scene::{apply, Identity};
    let (scene, id) = badge_scene();
    let mut world = World::new();
    apply(&mut world, &scene, Identity::Keep).expect("the scene loads");
    let entity = world.entity_with_id(id).expect("the entity is back");
    let mut state = DeclaredComponents::default();

    let report = apply_declarations(
        &mut world,
        &mut state,
        &[declarations("game/badge.erg", BADGE_SINCE)],
    );

    assert_eq!(report.adopted, vec!["Badge".to_owned()]);
    assert_eq!(
        read(&world, entity, "Badge", "note"),
        Some(ScriptValue::Null),
        "an `int?` field does not keep the text the file held as a `string?`"
    );
    let (note, _) = run(
        &mut world,
        &badge_reader(),
        "Note",
        entity,
        &[Value::Entity(entity)],
    );
    assert_eq!(note, Value::Int(-1), "the script reads its `Badge`");
}

/// **A struct a component holds, retyped, is a retype of the field.**
/// `Route.start` is a `Point`; `Point.x` goes from `int` to `string`. The
/// field's type is still `Point` and its kind still `Value`, so nothing the
/// registration compares says the stored `{ x: 1 }` no longer is one — and a
/// script reading its `Route` cannot take it.
#[test]
fn a_reload_retyping_a_held_struct_field_does_not_keep_the_old_value() {
    let before = "struct Point { int x; int y = 7; }
        component Route { Point start = Point { x: 1 }; }";
    let after = "struct Point { string x = \"west\"; int y = 7; }
        component Route { Point start = Point { }; }";
    let (mut world, mut state, _) = registered(&[declarations("game/route.erg", before)]);
    let entity = world.spawn(Transform::identity());
    attach(&mut world, entity, "Route", ScriptValue::Unit);

    apply_declarations(
        &mut world,
        &mut state,
        &[declarations("game/route.erg", after)],
    );

    let program = compile(
        &[
            ("game/route.erg", after),
            (
                "game/user.erg",
                "import \"game/route.erg\";
                 fn int Probe(Entity e) {
                     if (var r = e.Get(Route)) { return r.start.y; }
                     return -1;
                 }",
            ),
        ],
        "game/user.erg",
    );
    let (probe, _) = run(
        &mut world,
        &program,
        "Probe",
        entity,
        &[Value::Entity(entity)],
    );
    assert_eq!(
        probe,
        Value::Int(7),
        "the script reads its `Route`; stored: {:?}",
        read(&world, entity, "Route", "start")
    );
}

/// **Stop after a retype during Play.** Play snapshots the world while
/// `Badge.note` is a `string?`; a reload during Play makes it an `int?` and
/// resets it; Stop restores the snapshot onto the live declaration by name.
/// The text the reset refused must not come back with it.
#[test]
fn a_snapshot_restored_after_a_retype_does_not_bring_back_the_old_value() {
    use khora_core::scene::SerializationGoal;
    use khora_io::serialization::SerializationService;
    let (mut world, mut state, entity) = tagged();
    world.mark_authored(entity).expect("authored");
    let id = world.persistent_id(entity).expect("an id");
    let service = SerializationService::new();
    let snapshot = service
        .save_world(&world, SerializationGoal::FastestLoad)
        .expect("Play snapshots the world");

    apply_declarations(
        &mut world,
        &mut state,
        &[declarations("game/badge.erg", BADGE_SINCE)],
    );
    assert_eq!(
        read(&world, entity, "Badge", "note"),
        Some(ScriptValue::Null),
        "the reload reset the retyped field"
    );
    service
        .replace_world(&snapshot, &mut world)
        .expect("Stop restores the snapshot");
    let entity = world.entity_with_id(id).expect("the entity is back");

    assert_eq!(
        read(&world, entity, "Badge", "note"),
        Some(ScriptValue::Null),
        "an `int?` field does not take back the text it held as a `string?`"
    );
}

// ─── Two modules, one name ──────────────────────────────────────────────────

/// **The rival takes over when the owner stops declaring.** `game/rival.erg`
/// declares `Health` too and was refused while `game/health.erg` owned it;
/// once the owner drops it, the rival still declares it — so it is not an
/// orphan, and its scripts' layout is the one stored. A restart would give it
/// to the rival (the owner's set now empty); the running engine must agree.
#[test]
fn a_refused_rival_owns_a_name_its_owner_stops_declaring() {
    let rival = "[Domain(Physics)]
        component Health { int current = 100; int max = 100; int armor = 5; }";
    let (mut world, mut state, written, _) = reloading(HEALTH);
    let refused = apply_declarations(
        &mut world,
        &mut state,
        &[declarations("game/rival.erg", rival)],
    );
    assert_eq!(refused.refused, vec!["Health".to_owned()]);

    let report = reload(&mut world, &mut state, "fn void NoMoreHealth() { }");

    assert!(
        !state.is_orphaned("Health"),
        "a module still declares `Health`: {report:?}"
    );
    assert_eq!(state.owner_of("Health"), Some("game/rival.erg"));
    assert_eq!(
        read(&world, written, "Health", "armor"),
        Some(ScriptValue::Int(5)),
        "the rows take the rival's layout"
    );
}
