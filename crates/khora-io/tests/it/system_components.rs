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

//! A project's systems reach the `World` as components (Ergon spec 10): each
//! system's own component — written by the engine as the system runs, on an
//! entity no scene holds, so `Runtime` and resumable — and its state
//! companion, as a behavior's. Each role records where its behavior or
//! system sits in its module, and a name changing kind on a reload takes the
//! registration of its new kind.

use khora_data::ecs::{ComponentProvenance, ComponentVTable, ScriptRole, SemanticDomain, World};
use khora_io::script::declarations::{
    apply_declarations, DeclarationReport, DeclaredComponents, ScriptDeclarations,
};
use khora_io::script::mirror::PreludeLoader;
use khora_script::vm::Program;
use khora_script::{compile_module, Diagnostic, MemoryLoader};

const REGENERATION: &str = "system Regeneration {
    float total = 0.5;
    int sweeps = 0;
    void Update(float dt) { sweeps += 1; }
    every 2s { total = 0.0; }
}";

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

fn role(world: &World, name: &str) -> Option<ScriptRole> {
    vtable(world, name).script.clone()
}

// ─── A system's two components ──────────────────────────────────────────────

/// **A system registers two components, both `Runtime` and resumable**: its
/// own, with its fields, in `Script`, playing the part of a system of its
/// module; and its state companion, playing the part of its state.
#[test]
fn a_system_registers_its_component_and_its_state_runtime_and_resumable() {
    let (world, state, report) = registered(&[declarations("game/regen.erg", REGENERATION)]);

    assert_eq!(
        report.registered,
        vec!["Regeneration".to_owned(), "Regeneration#state".to_owned()]
    );

    let system = vtable(&world, "Regeneration");
    assert_eq!(
        system.provenance,
        ComponentProvenance::Runtime,
        "no scene holds a system's fields"
    );
    assert!(system.resumable, "a game save keeps them so play resumes");
    assert_eq!(system.domain, SemanticDomain::Script);
    assert_eq!(field_names(system), vec!["total", "sweeps"]);
    assert_eq!(
        role(&world, "Regeneration"),
        Some(ScriptRole::System {
            module: "game/regen.erg".to_owned(),
            order: 0,
        })
    );

    let companion = vtable(&world, "Regeneration#state");
    assert_eq!(companion.provenance, ComponentProvenance::Runtime);
    assert!(companion.resumable);
    assert_eq!(
        role(&world, "Regeneration#state"),
        Some(ScriptRole::BehaviorState {
            behavior: "Regeneration".to_owned()
        })
    );

    assert_eq!(state.owner_of("Regeneration"), Some("game/regen.erg"));
    assert_eq!(state.owner_of("Regeneration#state"), Some("game/regen.erg"));
}

/// **Each role records where its behavior or system sits in its module**:
/// source order, one list for both, a component between them taking none.
#[test]
fn behaviors_and_systems_carry_their_source_order_in_their_roles() {
    let (world, _, report) = registered(&[declarations(
        "game/a.erg",
        "behavior Zulu { int a = 0; }
         component Loot { int value = 1; }
         system Mike { int b = 0; }
         behavior Alpha { int c = 0; }",
    )]);
    assert!(report.refused.is_empty(), "{report:?}");

    let module = "game/a.erg".to_owned();
    assert_eq!(
        role(&world, "Zulu"),
        Some(ScriptRole::Behavior {
            module: module.clone(),
            order: 0
        })
    );
    assert_eq!(
        role(&world, "Mike"),
        Some(ScriptRole::System {
            module: module.clone(),
            order: 1
        })
    );
    assert_eq!(
        role(&world, "Alpha"),
        Some(ScriptRole::Behavior { module, order: 2 })
    );
    assert_eq!(
        vtable(&world, "Alpha").provenance,
        ComponentProvenance::Authored
    );
    assert!(!vtable(&world, "Alpha").resumable);
    assert_eq!(role(&world, "Loot"), None);
}

/// **An edit moving a behavior updates its place**: re-sent with a system
/// written before it, the behavior's role holds its new position.
#[test]
fn a_reload_reordering_a_module_updates_the_order_in_the_roles() {
    let mut world = World::new();
    let mut state = DeclaredComponents::default();
    apply_declarations(
        &mut world,
        &mut state,
        &[declarations(
            "game/a.erg",
            "behavior Guard { int hp = 1; }
             system Clock { int ticks = 0; }",
        )],
    );
    apply_declarations(
        &mut world,
        &mut state,
        &[declarations(
            "game/a.erg",
            "system Clock { int ticks = 0; }
             behavior Guard { int hp = 1; }",
        )],
    );

    let module = "game/a.erg".to_owned();
    assert_eq!(
        role(&world, "Clock"),
        Some(ScriptRole::System {
            module: module.clone(),
            order: 0
        })
    );
    assert_eq!(
        role(&world, "Guard"),
        Some(ScriptRole::Behavior { module, order: 1 })
    );
}

// ─── A name changing kind ───────────────────────────────────────────────────

/// **A behavior turned into a system by a reload takes a system's
/// registration** — `Runtime`, resumable, the system's role — and turned
/// back, a behavior's: authored, not resumable.
#[test]
fn a_name_changing_kind_on_a_reload_takes_its_new_provenance() {
    let mut world = World::new();
    let mut state = DeclaredComponents::default();
    let module = "game/guard.erg";
    apply_declarations(
        &mut world,
        &mut state,
        &[declarations(module, "behavior Guard { int hp = 1; }")],
    );
    assert_eq!(
        vtable(&world, "Guard").provenance,
        ComponentProvenance::Authored
    );

    let report = apply_declarations(
        &mut world,
        &mut state,
        &[declarations(module, "system Guard { int hp = 1; }")],
    );
    assert!(report.refused.is_empty(), "{report:?}");
    let guard = vtable(&world, "Guard");
    assert_eq!(
        guard.provenance,
        ComponentProvenance::Runtime,
        "now a system"
    );
    assert!(guard.resumable);
    assert_eq!(
        role(&world, "Guard"),
        Some(ScriptRole::System {
            module: module.to_owned(),
            order: 0
        })
    );

    apply_declarations(
        &mut world,
        &mut state,
        &[declarations(module, "behavior Guard { int hp = 1; }")],
    );
    let guard = vtable(&world, "Guard");
    assert_eq!(
        guard.provenance,
        ComponentProvenance::Authored,
        "a behavior again"
    );
    assert!(!guard.resumable);
    assert_eq!(
        role(&world, "Guard"),
        Some(ScriptRole::Behavior {
            module: module.to_owned(),
            order: 0
        })
    );
}

// ─── Names other modules hold ───────────────────────────────────────────────

/// **A system named like another module's behavior is refused at
/// registration** — the two modules never import each other — and brings no
/// companion of its own: the behavior keeps its fields and its role.
#[test]
fn a_system_named_like_another_modules_behavior_is_refused_at_registration() {
    let (world, state, report) = registered(&[
        declarations("a.erg", "behavior Guard { int a = 1; }"),
        declarations("b.erg", "system Guard { float b = 2.0; }"),
    ]);

    assert!(
        report.refused.contains(&"Guard".to_owned()),
        "the system is refused: {report:?}"
    );
    assert_eq!(state.owner_of("Guard"), Some("a.erg"));
    assert_eq!(field_names(vtable(&world, "Guard")), vec!["a"]);
    assert_eq!(
        role(&world, "Guard"),
        Some(ScriptRole::Behavior {
            module: "a.erg".to_owned(),
            order: 0
        })
    );
    assert_eq!(state.owner_of("Guard#state"), Some("a.erg"));
}

/// **A system named like another module's component is refused at
/// registration**, the component kept, and no `#state` registered for a
/// system that is not.
#[test]
fn a_system_named_like_another_modules_component_is_refused_at_registration() {
    let (world, state, report) = registered(&[
        declarations("a.erg", "component Clock { int n = 0; }"),
        declarations("b.erg", "system Clock { int ticks = 0; }"),
    ]);

    assert!(
        report.refused.contains(&"Clock".to_owned()),
        "the system is refused: {report:?}"
    );
    assert_eq!(state.owner_of("Clock"), Some("a.erg"));
    let clock = vtable(&world, "Clock");
    assert_eq!(field_names(clock), vec!["n"]);
    assert_eq!(clock.provenance, ComponentProvenance::Authored);
    assert_eq!(role(&world, "Clock"), None);
    assert!(
        world.components().key_named("Clock#state").is_none(),
        "no companion for a refused system: {report:?}"
    );
}
