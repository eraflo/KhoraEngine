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

//! A script reads a component: `e.Get(C)` returns a copy or `null`,
//! `e.Has(C)` says whether it is there.
//!
//! What the language promises, before anything reaches a `World`: what the
//! checker accepts and refuses, which components a program declares it reads,
//! what a read costs and what its fingerprint covers, and what a read returns
//! from a working set built by hand — including the writes the same run made.
//! The real mirror, the flow and a real `World` are the engine's end-to-end
//! tests.

use std::collections::HashMap;
use std::sync::Arc;

use khora_core::ecs::entity::EntityId;
use khora_core::math::Vec3;
use khora_core::script::{
    ColumnSnapshot, ComponentName, ComponentProjection, FrameComponents, ProjectedComponent,
    ScriptValue,
};
use khora_script::vm::{Machine, Program, Run, Value};
use khora_script::{
    compile_module, CompileOutcome, Diagnostic, Host, MemoryLoader, ENGINE_COMPONENTS_MODULE,
};

const SUBJECT: EntityId = EntityId {
    index: 4,
    generation: 1,
};

const OTHER: EntityId = EntityId {
    index: 9,
    generation: 1,
};

/// The engine module, as the mirror would serve it.
const ENGINE: &str = "
component RigidBody {
    float mass;
    bool ccd_enabled;
    Vec3 initial_velocity;
}
component Collider {
    float friction;
    float restitution;
    bool is_sensor;
}
component Health {
    int current;
    int max;
}
component Trio {
    float a;
    float b;
    float c;
}
component Twin {
    float a;
}
struct Transform {
    Vec3 translation;
    Quat rotation;
    Vec3 scale;
}
struct GlobalTransform {
    Vec3 translation;
}
";

fn compile(main: &str) -> CompileOutcome {
    let loader = MemoryLoader::new()
        .with(ENGINE_COMPONENTS_MODULE, ENGINE)
        .with(
            "main.erg",
            format!("import \"{ENGINE_COMPONENTS_MODULE}\";\n{main}"),
        );
    compile_module(&loader, "main.erg")
}

fn errors_of(result: &CompileOutcome) -> Vec<&Diagnostic> {
    result.diagnostics.iter().filter(|d| d.is_error()).collect()
}

fn build(main: &str) -> Program {
    let result = compile(main);
    let errors = errors_of(&result);
    assert!(errors.is_empty(), "should compile: {errors:?}");
    result.program.expect("a program with no errors")
}

fn said(diagnostic: &Diagnostic) -> String {
    format!(
        "{} | {}",
        diagnostic.message,
        diagnostic.note.as_deref().unwrap_or_default()
    )
}

/// The refusal of `main` saying `fragment` — after proving the engine module
/// compiles on its own, so the refusal is about `main`.
fn refused_saying(main: &str, fragment: &str) -> Diagnostic {
    let control = compile("fn void Probe() { }");
    assert!(
        control.succeeded(),
        "the engine module must compile on its own: {:?}",
        errors_of(&control)
    );
    let result = compile(main);
    assert!(
        !result.succeeded(),
        "should be refused, and compiled:\n{main}"
    );
    result
        .diagnostics
        .iter()
        .filter(|d| d.is_error())
        .find(|d| said(d).contains(fragment))
        .cloned()
        .unwrap_or_else(|| {
            panic!(
                "expected a refusal saying {fragment:?}; got {:?}",
                result.diagnostics.iter().map(said).collect::<Vec<_>>()
            )
        })
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

/// Component `name` held by `holders`, one column, with `default` as its
/// default row.
fn projected(
    name: &str,
    holders: &[(EntityId, ScriptValue)],
    default: ScriptValue,
) -> ProjectedComponent {
    let rows = holders.iter().map(|(_, row)| row.clone()).collect();
    let index: HashMap<EntityId, (u32, u32)> = holders
        .iter()
        .enumerate()
        .map(|(row, (entity, _))| (*entity, (0, row as u32)))
        .collect();
    ProjectedComponent::new(
        ComponentName::new(name),
        vec![Box::new(Rows(rows))],
        index,
        Some(default),
    )
}

fn body(mass: f32, ccd: bool) -> ScriptValue {
    fields(&[
        ("mass", ScriptValue::Float(mass)),
        ("ccd_enabled", ScriptValue::Bool(ccd)),
        (
            "initial_velocity",
            ScriptValue::Vec3(Vec3::new(1.0, 2.0, 3.0)),
        ),
    ])
}

fn collider(friction: f32) -> ScriptValue {
    fields(&[
        ("friction", ScriptValue::Float(friction)),
        ("restitution", ScriptValue::Float(0.25)),
        ("is_sensor", ScriptValue::Bool(false)),
    ])
}

fn health(current: i64, max: i64) -> ScriptValue {
    fields(&[
        ("current", ScriptValue::Int(current)),
        ("max", ScriptValue::Int(max)),
    ])
}

/// What the flow would project: the subject holds a rigid body and a health,
/// nobody holds a collider, and `Trio` is held by the subject.
fn projection() -> ComponentProjection {
    ComponentProjection::new(vec![
        projected("RigidBody", &[(SUBJECT, body(2.0, true))], body(1.0, false)),
        projected("Collider", &[], collider(0.5)),
        projected("Health", &[(SUBJECT, health(5, 10))], health(100, 100)),
        projected(
            "Trio",
            &[(
                SUBJECT,
                fields(&[
                    ("a", ScriptValue::Float(1.0)),
                    ("b", ScriptValue::Float(2.0)),
                    ("c", ScriptValue::Float(3.0)),
                ]),
            )],
            fields(&[
                ("a", ScriptValue::Float(0.0)),
                ("b", ScriptValue::Float(0.0)),
                ("c", ScriptValue::Float(0.0)),
            ]),
        ),
        projected(
            "Twin",
            &[(SUBJECT, fields(&[("a", ScriptValue::Float(1.0))]))],
            fields(&[("a", ScriptValue::Float(0.0))]),
        ),
    ])
}

fn host_over(projection: ComponentProjection) -> Host {
    let mut host = Host::new().for_entity(SUBJECT);
    host.components = FrameComponents::new(Arc::new(projection));
    host
}

/// Runs `function` whole over the projection, returning its result.
fn run(program: &Program, function: &str, args: &[Value]) -> Value {
    let mut host = host_over(projection());
    let mut machine = Machine::new(program, function, args).expect("the function exists");
    let outcome = machine.run(program, &mut host, u64::MAX);
    assert_eq!(outcome, Run::Completed, "`{function}` completes");
    machine.result()
}

// ─── What a read returns ────────────────────────────────────────────────────

/// **A copy of the entity's component**, read field by field like a struct.
#[test]
fn get_returns_a_copy_of_the_projected_component() {
    let program = build(
        "fn float Mass(Entity e) {
             if (var b = e.Get(RigidBody)) { return b.mass; }
             return -1.0;
         }
         fn float Lift(Entity e) {
             if (var b = e.Get(RigidBody)) { return b.initial_velocity.y; }
             return -1.0;
         }
         fn float Chained(Entity e) { return e.Get(RigidBody)?.mass ?? -1.0; }",
    );

    assert_eq!(
        run(&program, "Mass", &[Value::Entity(SUBJECT)]),
        Value::Float(2.0)
    );
    assert_eq!(
        run(&program, "Lift", &[Value::Entity(SUBJECT)]),
        Value::Float(2.0)
    );
    assert_eq!(
        run(&program, "Chained", &[Value::Entity(SUBJECT)]),
        Value::Float(2.0)
    );
}

/// An entity that does not hold the component reads `null` — and so does a
/// component nobody holds.
#[test]
fn get_returns_null_for_an_entity_without_it() {
    let program = build(
        "fn int Body(Entity e) { if (var b = e.Get(RigidBody)) { return 1; } return 0; }
         fn int Grip(Entity e) { if (var c = e.Get(Collider)) { return 1; } return 0; }",
    );

    assert_eq!(
        run(&program, "Body", &[Value::Entity(OTHER)]),
        Value::Int(0)
    );
    assert_eq!(
        run(&program, "Grip", &[Value::Entity(SUBJECT)]),
        Value::Int(0)
    );
    assert_eq!(
        run(&program, "Body", &[Value::Entity(SUBJECT)]),
        Value::Int(1),
        "and the holder reads it"
    );
}

#[test]
fn has_reads_the_working_set() {
    let program = build("fn bool F(Entity e) { return e.Has(Health); }");

    assert_eq!(
        run(&program, "F", &[Value::Entity(SUBJECT)]),
        Value::Bool(true)
    );
    assert_eq!(
        run(&program, "F", &[Value::Entity(OTHER)]),
        Value::Bool(false)
    );
}

/// **A script sees its own write.** `Set` then `Get` in one run reads the
/// fields written; the fields it did not name keep the projected value.
#[test]
fn a_get_after_a_set_reads_the_write() {
    let program = build(
        "fn float F(Entity e) {
             e.Set(RigidBody { mass: 12.0 });
             if (var b = e.Get(RigidBody)) {
                 return b.ccd_enabled ? b.mass + 100.0 : b.mass;
             }
             return -1.0;
         }",
    );

    assert_eq!(
        run(&program, "F", &[Value::Entity(SUBJECT)]),
        Value::Float(112.0)
    );
}

/// `Add` makes `Has` true and `Get` the defaults with the patch over them;
/// `Remove` makes `Has` false and `Get` null — within the same run.
#[test]
fn has_follows_add_and_remove_in_one_run() {
    let program = build(
        "fn float Added(Entity e) {
             float before = e.Has(Collider) ? 1.0 : 0.0;
             e.Add(Collider { restitution: 0.75 });
             float now = e.Has(Collider) ? 10.0 : 0.0;
             if (var c = e.Get(Collider)) { return before + now + c.friction + c.restitution; }
             return -1.0;
         }
         fn int Removed(Entity e) {
             e.Remove(Health);
             int held = e.Has(Health) ? 1 : 0;
             if (var h = e.Get(Health)) { return held + 10; }
             return held;
         }",
    );

    // 0 before, 10 after, the default friction 0.5, the patched 0.75.
    assert_eq!(
        run(&program, "Added", &[Value::Entity(SUBJECT)]),
        Value::Float(11.25)
    );
    assert_eq!(
        run(&program, "Removed", &[Value::Entity(SUBJECT)]),
        Value::Int(0)
    );
}

/// **A copy is a copy.** A write after the `Get` reaches the working set, not
/// the value already read.
#[test]
fn a_copy_does_not_follow_later_writes() {
    let program = build(
        "fn int F(Entity e) {
             var before = e.Get(Health);
             e.Set(Health { current: 9 });
             var later = e.Get(Health);
             return (before?.current ?? -1) * 100 + (later?.current ?? -1);
         }",
    );

    assert_eq!(
        run(&program, "F", &[Value::Entity(SUBJECT)]),
        Value::Int(509)
    );
}

/// A row that does not fit the component the program knows is a fault of the
/// run, never a panic of the engine.
#[test]
fn a_row_that_does_not_fit_faults_the_run() {
    let program =
        build("fn int F(Entity e) { if (var h = e.Get(Health)) { return h.current; } return -1; }");
    let broken = ComponentProjection::new(vec![projected(
        "Health",
        &[(
            SUBJECT,
            fields(&[
                ("current", ScriptValue::Str("five".to_owned())),
                ("max", ScriptValue::Int(10)),
            ]),
        )],
        health(100, 100),
    )]);
    let mut host = host_over(broken);
    let mut machine =
        Machine::new(&program, "F", &[Value::Entity(SUBJECT)]).expect("the function exists");

    let outcome = machine.run(&program, &mut host, u64::MAX);

    assert!(
        matches!(outcome, Run::Faulted(_)),
        "a row that does not import faults; got {outcome:?}"
    );
}

// ─── The read set ───────────────────────────────────────────────────────────

/// **A program says what it reads**: every component a `Get` or a `Has`
/// names, wherever it sits, sorted and each once. A component it only writes
/// is not read.
#[test]
fn a_program_lists_the_components_it_reads() {
    let program = build(
        "fn bool A(Entity e) { return e.Has(RigidBody) && e.Has(Health); }
         fn void W(Entity e) { e.Set(Trio { a: 1.0 }); e.Remove(Twin); }
         behavior Guard {
             void Update(float dt) {
                 var h = this.Get(Health);
                 if (var c = this.Get(Collider)) { }
             }
         }",
    );

    assert_eq!(program.reads, vec!["Collider", "Health", "RigidBody"]);
    assert!(
        build("fn void F(Entity e) { e.Set(Trio { a: 1.0 }); }")
            .reads
            .is_empty(),
        "a program that reads nothing lists nothing"
    );
}

// ─── What the checker accepts and refuses ───────────────────────────────────

/// **Writing a field of a copy is an error**: it would change the copy and
/// nothing else. The message says so and how the component is written —
/// whether the field is the copy's own or one inside it.
#[test]
fn a_copy_is_read_only() {
    for (main, message) in [
        (
            "behavior Medic { void Go() { if (var h = this.Get(Health)) { h.current = 5; } } }",
            "`h` is a copy of `Health` and is read-only",
        ),
        (
            "behavior Medic { void Go() { if (var h = this.Get(Health)) { h.current += 1; } } }",
            "`h` is a copy of `Health` and is read-only",
        ),
        (
            "behavior Medic { void Go() { Health? h = this.Get(Health); if (var k = h) { Health same = k; same.max = 5; } } }",
            "`same` is a copy of `Health` and is read-only",
        ),
    ] {
        let refusal = refused_saying(main, message);
        let text = said(&refusal);
        assert!(
            text.contains("Set(Health {"),
            "the refusal says how the component is written; got {text:?}"
        );
    }

    let nested = refused_saying(
        "behavior Pusher { void Go() { if (var b = this.Get(RigidBody)) { b.initial_velocity.x = 1.0; } } }",
        "`b` is a copy of `RigidBody` and is read-only",
    );
    assert!(
        said(&nested).contains("Set(RigidBody {"),
        "{}",
        said(&nested)
    );
}

/// **A component names the type of a copy, never of nothing.** A local with
/// a `Get` behind it — written, inferred, narrowed — compiles; a local with no
/// value, a parameter, a field or a return of a component stay refused.
#[test]
fn a_component_typed_local_needs_its_value() {
    for main in [
        "behavior Medic { void Go() { Health? h = this.Get(Health); } }",
        "behavior Medic { void Go() { var h = this.Get(Health); } }",
        "behavior Medic { void Go() { if (var h = this.Get(Health)) { Health same = h; int c = same.current; } } }",
        "fn int Current(Entity e) { Health? h = e.Get(Health); return h?.current ?? 0; }",
    ] {
        let result = compile(main);
        assert!(
            result.succeeded(),
            "should compile:\n{main}\n{:?}",
            errors_of(&result)
        );
    }

    let message = "`Health` is a component";
    refused_saying("fn void F() { Health h; }", message);
    refused_saying("fn void F(Health h) { }", message);
    refused_saying("struct Bag { Health h; }", message);
    refused_saying("fn Health? F(Entity e) { return e.Get(Health); }", message);
}

/// **Growing or shrinking an array field of a copy is a write to the copy**,
/// refused like `b.slots[0] = 1` is: `Push` and `RemoveAt` on it would change
/// the copy and nothing else — the trap the read-only rule exists to close.
#[test]
fn a_copy_array_field_is_read_only() {
    let loader = |main: &str| {
        MemoryLoader::new()
            .with(
                ENGINE_COMPONENTS_MODULE,
                "component Bag { int[] slots; string label; }",
            )
            .with(
                "main.erg",
                format!("import \"{ENGINE_COMPONENTS_MODULE}\";\n{main}"),
            )
    };
    let indexed = compile_module(
        &loader("fn void F(Entity e) { if (var b = e.Get(Bag)) { b.slots[0] = 1; } }"),
        "main.erg",
    );
    assert!(
        !indexed.succeeded(),
        "the control: an index write through a copy is refused"
    );

    for main in [
        "fn void F(Entity e) { if (var b = e.Get(Bag)) { b.slots.Push(1); } }",
        "fn void F(Entity e) { if (var b = e.Get(Bag)) { b.slots.RemoveAt(0); } }",
    ] {
        let result = compile_module(&loader(main), "main.erg");
        let refusal = result
            .diagnostics
            .iter()
            .filter(|d| d.is_error())
            .find(|d| said(d).contains("is a copy of `Bag` and is read-only"));
        assert!(
            refusal.is_some(),
            "a mutation of a copy's array should be refused:\n{main}\ngot {:?}",
            result.diagnostics.iter().map(said).collect::<Vec<_>>()
        );
    }
}

/// **`Get` reads a component, not a struct.** `Transform` is a struct the
/// engine places with its own operations, and the refusal points at the one
/// that reads it — `Position()`; an engine-computed struct is refused too.
#[test]
fn get_reads_a_component_not_a_struct() {
    let refusal = refused_saying(
        "behavior Mover { void Go() { var t = this.Get(Transform); } }",
        "Position()",
    );
    assert!(
        said(&refusal).contains("Transform"),
        "the refusal is about `Transform`; got {:?}",
        said(&refusal)
    );
    refused_saying(
        "behavior Mover { void Go() { bool placed = this.Has(Transform); } }",
        "Transform",
    );
    refused_saying(
        "behavior Mover { void Go() { var g = this.Get(GlobalTransform); } }",
        "GlobalTransform",
    );
    refused_saying(
        "behavior Mover { void Go() { var s = this.Get(Shield); } }",
        "Shield",
    );
}

/// `Get` and `Has` take one bare component name — not a literal, not a value.
#[test]
fn get_takes_a_bare_component_name() {
    for main in [
        "fn void F(Entity e) { var h = e.Get(Health); }",
        "fn void F(Entity e) { bool b = e.Has(Health); }",
    ] {
        let result = compile(main);
        assert!(
            result.succeeded(),
            "the bare name is accepted:\n{main}\n{:?}",
            errors_of(&result)
        );
    }
    refused_saying(
        "fn void F(Entity e) { var h = e.Get(Health { current: 1 }); }",
        "Health",
    );
    refused_saying("fn void F(Entity e) { var h = e.Get(); }", "Get");
    refused_saying(
        "fn void F(Entity e) { bool b = e.Has(Health, Collider); }",
        "Has",
    );
}

/// **The mirror's component is a struct layout**, so a copy is an ordinary
/// struct object: each `component` declaration yields one, its fields in
/// declaration order, with no defaults — a copy always has every field.
#[test]
fn the_mirror_component_is_a_struct_layout() {
    let program =
        build("fn int F(Entity e) { if (var h = e.Get(Health)) { return h.max; } return -1; }");

    let (_, layout) = program
        .struct_named("RigidBody")
        .expect("a component declaration yields a struct layout");
    assert_eq!(
        layout.fields,
        vec![
            ("mass".to_owned(), "float".to_owned()),
            ("ccd_enabled".to_owned(), "bool".to_owned()),
            ("initial_velocity".to_owned(), "Vec3".to_owned()),
        ]
    );
    assert!(
        layout.defaults.iter().all(Option::is_none),
        "a component's layout carries no defaults: {:?}",
        layout.defaults
    );
    assert!(program.struct_named("Health").is_some());

    assert_eq!(
        run(&program, "F", &[Value::Entity(SUBJECT)]),
        Value::Int(10),
        "the copy imports as that struct"
    );
}

// ─── Cost and identity ──────────────────────────────────────────────────────

/// **A read pays for its size.** A `Get` costs one plus the fields of the
/// component; a `Has` costs one. A run is charged what its instructions cost,
/// whether the read found the component or not.
#[test]
fn a_read_costs_one_plus_its_fields() {
    let program = build(
        "fn void One(Entity e) { var x = e.Get(Twin); }
         fn void Three(Entity e) { var x = e.Get(Trio); }
         fn void Probe(Entity e) { bool x = e.Has(Trio); }",
    );

    for (function, sized) in [("One", vec![2]), ("Three", vec![4]), ("Probe", vec![])] {
        let code = &program.function(function).expect("compiled").code;
        let costs: Vec<u64> = code
            .iter()
            .map(|instruction| instruction.cost())
            .filter(|cost| *cost > 1)
            .collect();
        assert_eq!(costs, sized, "`{function}`");

        let declared: u64 = code.iter().map(|instruction| instruction.cost()).sum();
        for target in [SUBJECT, OTHER] {
            let mut host = host_over(projection());
            let mut machine = Machine::new(&program, function, &[Value::Entity(target)])
                .expect("the function exists");
            let (outcome, spent) = machine.run_counting(&program, &mut host, u64::MAX);
            assert_eq!(outcome, Run::Completed);
            assert_eq!(
                spent, declared,
                "`{function}` on {target:?} is charged what its instructions cost"
            );
        }
    }
}

/// **A read is named in its fingerprint, never numbered.** Another component
/// — even one of the same size — or the other operation changes it; moving
/// where its name sits in the string table, by reads compiled before it, does
/// not.
#[test]
fn a_read_hashes_its_names() {
    fn fingerprint(main: &str, function: &str) -> u64 {
        build(main)
            .function(function)
            .expect("compiled")
            .fingerprint
    }

    let get = "fn void F(Entity e) { var x = e.Get(RigidBody); }";
    let base = fingerprint(get, "F");

    assert_ne!(
        base,
        fingerprint("fn void F(Entity e) { var x = e.Get(Collider); }", "F"),
        "another component of the same size"
    );
    assert_ne!(
        base,
        fingerprint("fn void F(Entity e) { var x = e.Has(RigidBody); }", "F"),
        "another operation"
    );
    assert_eq!(
        base,
        fingerprint(
            r#"fn string G() { return "before"; }
               fn void H(Entity e) { var h = e.Get(Health); bool c = e.Has(Collider); }
               fn void F(Entity e) { var x = e.Get(RigidBody); }"#,
            "F"
        ),
        "the same read, its name elsewhere in the string table"
    );

    let has = "fn bool F(Entity e) { return e.Has(Trio); }";
    assert_ne!(
        fingerprint(has, "F"),
        fingerprint("fn bool F(Entity e) { return e.Has(Twin); }", "F"),
        "another component tested"
    );
    assert_eq!(
        fingerprint(has, "F"),
        fingerprint(
            "fn bool G(Entity e) { return e.Has(Health); }
             fn bool F(Entity e) { return e.Has(Trio); }",
            "F"
        ),
        "the same test, its name elsewhere"
    );
}
