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

//! `Spawn(position, C { … }, …)` is an `Entity`.
//!
//! The id is reserved in the turn, so the script holds the entity at once:
//! `var b = Spawn(…)`, then `b.Set(…)`, `b.Get(…)`, `b.Has(…)` in the same
//! turn — the working set holds what the spawn attached, and a later write
//! merges onto it. The `Spawn` command carries the reserved id, and every
//! command naming `b` after it names that same id. A value left unused is
//! still a spawn, as it always was.

use std::collections::HashMap;
use std::sync::Arc;

use khora_core::ecs::entity::EntityId;
use khora_core::math::{Quaternion, Vec3};
use khora_core::script::{
    ColumnSnapshot, ComponentName, ComponentProjection, FrameComponents, ProjectedComponent,
    ScriptValue, WorldCommand,
};
use khora_script::vm::{Machine, Program, Run, Value};
use khora_script::{
    check, compile, compile_module, lex, parse, CompileOutcome, Diagnostic, Host, MemoryLoader,
    ENGINE_COMPONENTS_MODULE,
};

const SUBJECT: EntityId = EntityId {
    index: 4,
    generation: 1,
};

/// The engine module, as the mirror would serve it.
const ENGINE: &str = "
component Health {
    int current;
    int max;
}
component Collider {
    float friction;
    bool is_sensor;
}
";

fn compile_main(main: &str) -> CompileOutcome {
    let loader = MemoryLoader::new()
        .with(ENGINE_COMPONENTS_MODULE, ENGINE)
        .with(
            "main.erg",
            format!("import \"{ENGINE_COMPONENTS_MODULE}\";\n{main}"),
        );
    compile_module(&loader, "main.erg")
}

fn errors_of(result: &CompileOutcome) -> Vec<String> {
    result
        .diagnostics
        .iter()
        .filter(|d| d.is_error())
        .map(said)
        .collect()
}

fn said(diagnostic: &Diagnostic) -> String {
    format!(
        "{} | {}",
        diagnostic.message,
        diagnostic.note.as_deref().unwrap_or_default()
    )
}

fn build(main: &str) -> Program {
    let result = compile_main(main);
    let errors = errors_of(&result);
    assert!(errors.is_empty(), "should compile:\n{main}\n{errors:?}");
    result.program.expect("a program with no errors")
}

fn fields(pairs: &[(&str, ScriptValue)]) -> ScriptValue {
    ScriptValue::Struct(
        pairs
            .iter()
            .map(|(name, value)| ((*name).to_owned(), value.clone()))
            .collect(),
    )
}

/// A column of rows already converted.
struct Rows(Vec<ScriptValue>);

impl ColumnSnapshot for Rows {
    fn len(&self) -> usize {
        self.0.len()
    }
    fn read(&self, row: usize) -> ScriptValue {
        self.0[row].clone()
    }
}

/// A component the program reads, held by nobody, with `default` as its
/// default row.
fn read_by_nobody(name: &str, default: ScriptValue) -> ProjectedComponent {
    ProjectedComponent::new(
        ComponentName::new(name),
        vec![Box::new(Rows(Vec::new()))],
        HashMap::new(),
        Some(default),
    )
}

/// A host over a working set reading `Health` and `Collider`, neither held by
/// anyone: whatever an entity holds in it, this frame's commands gave it.
fn host() -> Host {
    let mut host = Host::new().for_entity(SUBJECT);
    host.components = FrameComponents::new(Arc::new(ComponentProjection::new(vec![
        read_by_nobody(
            "Health",
            fields(&[
                ("current", ScriptValue::Int(100)),
                ("max", ScriptValue::Int(100)),
            ]),
        ),
        read_by_nobody(
            "Collider",
            fields(&[
                ("friction", ScriptValue::Float(0.5)),
                ("is_sensor", ScriptValue::Bool(false)),
            ]),
        ),
    ])));
    host
}

/// Runs `function` whole on a fresh host: its result and what it queued.
fn run(program: &Program, function: &str) -> (Value, Vec<WorldCommand>) {
    let mut host = host();
    let mut machine = Machine::new(program, function, &[]).expect("the function exists");
    assert_eq!(machine.run(program, &mut host, u64::MAX), Run::Completed);
    (machine.result(), host.commands.as_slice().to_vec())
}

/// Runs `function` on a fresh host `slice` fuel at a time, resumed until it
/// completes: its result and what it queued.
fn run_sliced(program: &Program, function: &str, slice: u64) -> (Value, Vec<WorldCommand>) {
    let mut host = host();
    let mut machine = Machine::new(program, function, &[]).expect("the function exists");
    for _ in 0..100_000 {
        match machine.run(program, &mut host, slice) {
            Run::Completed => return (machine.result(), host.commands.as_slice().to_vec()),
            Run::Suspended(_) => {}
            Run::Faulted(fault) => panic!("`{function}` faulted at a slice of {slice}: {fault:?}"),
        }
    }
    panic!("`{function}` never completed at a slice of {slice}");
}

/// The entities the spawns among `commands` carry, in order.
fn spawned(commands: &[WorldCommand]) -> Vec<EntityId> {
    commands
        .iter()
        .filter_map(|command| match command {
            WorldCommand::Spawn { entity, .. } => Some(*entity),
            _ => None,
        })
        .collect()
}

fn entity_of(value: &Value) -> EntityId {
    match value {
        Value::Entity(entity) => *entity,
        other => panic!("an entity, not {other:?}"),
    }
}

// ─── What `Spawn` returns ───────────────────────────────────────────────────

/// **`Spawn` is the entity it spawns.** The value it returns is the id the
/// queued `Spawn` carries, with the spawn's position and components.
#[test]
fn spawn_returns_the_entity_its_command_carries() {
    let program = build(
        "fn Entity F() {
             var b = Spawn(Vec3(1.0, 2.0, 3.0), Health { current: 7, max: 10 });
             return b;
         }",
    );

    let (result, commands) = run(&program, "F");

    let b = entity_of(&result);
    assert_eq!(
        commands,
        vec![WorldCommand::Spawn {
            entity: b,
            position: Vec3::new(1.0, 2.0, 3.0),
            rotation: Quaternion::IDENTITY,
            components: vec![(
                ComponentName::new("Health"),
                fields(&[
                    ("current", ScriptValue::Int(7)),
                    ("max", ScriptValue::Int(10)),
                ]),
            )],
        }]
    );
    assert_ne!(b, SUBJECT, "the spawned entity is a new one");
}

/// `Spawn` has the type `Entity`: it initialises a declared `Entity`, an
/// `Entity?`, and is passed where an `Entity` is taken.
#[test]
fn spawn_has_the_type_entity() {
    let program = build(
        "fn Entity Keep(Entity e) { return e; }
         fn Entity F() {
             Entity a = Spawn(Vec3(0.0, 0.0, 0.0));
             Entity? b = Spawn(Vec3(0.0, 0.0, 0.0));
             return Keep(Spawn(Vec3(0.0, 0.0, 0.0)));
         }",
    );

    let (result, commands) = run(&program, "F");

    let ids = spawned(&commands);
    assert_eq!(ids.len(), 3, "three spawns: {commands:?}");
    assert_eq!(entity_of(&result), ids[2], "the third spawn was passed on");
}

/// **Two spawns are two entities**, each carrying its own id, distinct from
/// every other the run holds.
#[test]
fn two_spawns_are_two_entities() {
    let program = build(
        "fn Entity F() {
             var a = Spawn(Vec3(0.0, 0.0, 0.0));
             var b = Spawn(Vec3(0.0, 0.0, 0.0));
             return b;
         }",
    );

    let (result, commands) = run(&program, "F");

    let ids = spawned(&commands);
    assert_eq!(ids.len(), 2);
    assert_ne!(ids[0], ids[1], "two spawns share an id");
    assert_eq!(entity_of(&result), ids[1]);
}

/// **A value left unused is still a spawn** — and each such spawn carries an
/// id of its own all the same.
#[test]
fn spawns_whose_value_is_unused_carry_distinct_ids() {
    let program = build(
        "fn void F() {
             Spawn(Vec3(0.0, 1.0, 0.0), Health { current: 3 });
             Spawn(Vec3(0.0, 2.0, 0.0));
         }",
    );

    let (_, commands) = run(&program, "F");

    let ids = spawned(&commands);
    assert_eq!(ids.len(), 2, "both spawns are queued: {commands:?}");
    assert_ne!(ids[0], ids[1], "two spawns share an id");
    assert!(!ids.contains(&SUBJECT));
}

// ─── The spawned entity in the same turn ────────────────────────────────────

/// **A write after the spawn names the spawned entity**: `b.Set(…)` queues a
/// `Set` of the id the spawn carries, after the spawn.
#[test]
fn a_set_after_a_spawn_names_the_spawned_entity() {
    let program = build(
        "fn void F() {
             var b = Spawn(Vec3(0.0, 0.0, 0.0), Health { current: 7, max: 10 });
             b.Set(Health { current: 3 });
         }",
    );

    let (_, commands) = run(&program, "F");

    let ids = spawned(&commands);
    assert_eq!(ids.len(), 1);
    assert_eq!(commands.len(), 2, "{commands:?}");
    assert_eq!(
        commands[1],
        WorldCommand::SetComponent {
            entity: ids[0],
            component: ComponentName::new("Health"),
            value: fields(&[("current", ScriptValue::Int(3))]),
        }
    );
}

/// **`b.Get` reads what was spawned**, in the same turn: the fields the spawn
/// named, and the default row's for the others.
#[test]
fn get_in_the_same_turn_reads_what_was_spawned() {
    let program = build(
        "fn int Whole() {
             var b = Spawn(Vec3(0.0, 0.0, 0.0), Health { current: 7, max: 10 });
             if (var h = b.Get(Health)) { return h.current * 1000 + h.max; }
             return -1;
         }
         fn int Partial() {
             var b = Spawn(Vec3(0.0, 0.0, 0.0), Health { current: 7 });
             if (var h = b.Get(Health)) { return h.current * 1000 + h.max; }
             return -1;
         }",
    );

    assert_eq!(run(&program, "Whole").0, Value::Int(7010));
    assert_eq!(
        run(&program, "Partial").0,
        Value::Int(7100),
        "`max` from the default row"
    );
}

/// **A `Set` after the spawn merges onto what was spawned**: the field it
/// names changes, the other keeps the spawn's value.
#[test]
fn get_after_a_set_reads_the_set_merged_onto_the_spawn() {
    let program = build(
        "fn int F() {
             var b = Spawn(Vec3(0.0, 0.0, 0.0), Health { current: 7, max: 10 });
             b.Set(Health { current: 3 });
             if (var h = b.Get(Health)) { return h.current * 1000 + h.max; }
             return -1;
         }",
    );

    assert_eq!(run(&program, "F").0, Value::Int(3010));
}

/// **`b.Has` answers for the spawned entity**: it holds what the spawn
/// attached, and not a component the spawn did not carry.
#[test]
fn has_in_the_same_turn_answers_for_the_spawned_entity() {
    let program = build(
        "fn int F() {
             var b = Spawn(Vec3(0.0, 0.0, 0.0), Health { current: 7 });
             int answer = 0;
             if (b.Has(Health)) { answer += 1; }
             if (b.Has(Collider)) { answer += 10; }
             return answer;
         }",
    );

    assert_eq!(run(&program, "F").0, Value::Int(1));
}

/// **A spawn reserves its id once, however its run is cut.** At every slice
/// of fuel the run queues one spawn and a `Set` of the entity it carries, and
/// returns that entity.
#[test]
fn a_spawn_reserves_once_however_its_run_is_sliced() {
    let program = build(
        "fn Entity F() {
             var b = Spawn(Vec3(0.0, 0.0, 0.0), Health { current: 7, max: 10 });
             b.Set(Health { current: 3 });
             return b;
         }",
    );

    for slice in 1..=40 {
        let (result, commands) = run_sliced(&program, "F", slice);
        let ids = spawned(&commands);
        assert_eq!(ids.len(), 1, "at a slice of {slice}: {commands:?}");
        assert_eq!(entity_of(&result), ids[0], "at a slice of {slice}");
        assert_eq!(
            commands.last().and_then(WorldCommand::entity),
            Some(ids[0]),
            "at a slice of {slice}, the `Set` names the spawned entity"
        );
    }
}

// ─── Fingerprints and cost ──────────────────────────────────────────────────

fn program_of(source: &str) -> Program {
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

fn function_fingerprint(program: &Program, name: &str) -> u64 {
    program
        .function(name)
        .unwrap_or_else(|| panic!("`{name}` is a function of the program"))
        .fingerprint
}

/// A module of every component operation but `Spawn`, and a function beside a
/// spawning one.
const WITHOUT_SPAWN: &str = "component Health { int current; int max; }
event Hurt(int amount);
behavior Guard {
    int health = 100;
    void Update(float dt) {
        this.Set(Health { current: 5 });
        this.Add(Health { max: 10 });
        if (var h = this.Get(Health)) { health = h.current; }
        if (this.Has(Health)) { this.Remove(Health); }
        this.Raise(Hurt(3));
    }
}
fn int Quiet() { return 1 + 2; }";

const WITH_SPAWN: &str = "component Health { int current; int max; }
fn void Fire() { Spawn(Vec3(0.0, 1.0, 0.0), Health { current: 3 }); }
fn int Quiet() { return 1 + 2; }";

/// What `WITHOUT_SPAWN` fingerprints to, pinned: code that holds no `Spawn`
/// is the same code, whatever `Spawn` became.
const WITHOUT_SPAWN_FINGERPRINT: u64 = 2690286953331110674;
/// What `WITH_SPAWN`'s `Quiet` fingerprints to, pinned.
const QUIET_FINGERPRINT: u64 = 4002117188516457586;
/// What `WITH_SPAWN`'s `Fire` fingerprinted to while a spawn wrote no entity
/// anywhere: one that does is other code.
const FIRE_FINGERPRINT_BEFORE_SPAWN_RETURNED: u64 = 16687987326840086810;
/// What `Fire` costs to run whole, pinned: the spawn instruction keeps its
/// cost. One less than while a spawn wrote nothing: the `Unit` its call loaded
/// in place of a value is gone, its entity being the value now.
const FIRE_COST: u64 = 14;

/// **A program without a `Spawn` keeps its fingerprint**, and so does a
/// function without one beside a spawning function: a save holding them
/// resumes into them as before.
#[test]
fn code_without_a_spawn_keeps_its_fingerprint() {
    assert_eq!(
        program_of(WITHOUT_SPAWN).fingerprint(),
        WITHOUT_SPAWN_FINGERPRINT
    );
    assert_eq!(
        function_fingerprint(&program_of(WITH_SPAWN), "Quiet"),
        QUIET_FINGERPRINT
    );
}

/// **A function holding a `Spawn` is other code**: its spawn now writes the
/// entity somewhere, and a fingerprint that ignored where would resume one
/// version into the other.
#[test]
fn a_function_holding_a_spawn_changes_its_fingerprint() {
    assert_ne!(
        function_fingerprint(&program_of(WITH_SPAWN), "Fire"),
        FIRE_FINGERPRINT_BEFORE_SPAWN_RETURNED
    );
}

/// A spawn whose value is unused costs what it cost.
#[test]
fn a_spawn_keeps_its_cost() {
    let program = program_of(WITH_SPAWN);
    let mut host = Host::new();
    let mut machine = Machine::new(&program, "Fire", &[]).expect("`Fire` exists");
    let (outcome, cost) = machine.run_counting(&program, &mut host, u64::MAX);
    assert_eq!(outcome, Run::Completed);
    assert_eq!(cost, FIRE_COST);
}
