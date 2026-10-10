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

//! A script writes a component: `e.Set(C { … })`, `e.Add(C { … })`,
//! `e.Remove(C)`, `Spawn(position, C { … }, …)`.
//!
//! What the language promises, before anything reaches a `World`: the command
//! each operation queues — a patch of the fields written, in the order they
//! were written — what the checker refuses, what a write costs, and what its
//! fingerprint covers. The engine module here is written by hand, in the shape
//! the mirror serves; the real mirror and a real `World` are the engine's
//! end-to-end tests.

use khora_core::ecs::entity::EntityId;
use khora_core::math::{Quaternion, Vec3};
use khora_core::script::{ComponentName, ScriptValue, WorldCommand};
use khora_script::vm::{Machine, Program, Run, Value};
use khora_script::{
    compile_module, CompileOutcome, Diagnostic, Host, MemoryLoader, ENGINE_COMPONENTS_MODULE,
};

const SUBJECT: EntityId = EntityId {
    index: 4,
    generation: 1,
};

/// The engine module, as the mirror would serve it: components an author may
/// write, and structs for what the engine writes itself.
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

/// `main.erg` importing the engine module, compiled.
fn compile(main: &str) -> CompileOutcome {
    compile_with(main, &[])
}

/// `main.erg`, with the engine module and `others` beside it.
fn compile_with(main: &str, others: &[(&str, &str)]) -> CompileOutcome {
    let mut loader = MemoryLoader::new()
        .with(ENGINE_COMPONENTS_MODULE, ENGINE)
        .with(
            "main.erg",
            format!("import \"{ENGINE_COMPONENTS_MODULE}\";\n{main}"),
        );
    for &(path, source) in others {
        loader = loader.with(path, source);
    }
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

/// The refusals of `main` — after proving the engine module compiles, so a
/// refusal is about `main` and not about a module the language cannot read.
fn refusals(main: &str) -> Vec<Diagnostic> {
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
        .into_iter()
        .filter(Diagnostic::is_error)
        .collect()
}

fn said(diagnostic: &Diagnostic) -> String {
    format!(
        "{} | {}",
        diagnostic.message,
        diagnostic.note.as_deref().unwrap_or_default()
    )
}

fn refused_saying(main: &str, fragment: &str) -> Diagnostic {
    let found = refusals(main);
    found
        .iter()
        .find(|d| said(d).contains(fragment))
        .cloned()
        .unwrap_or_else(|| {
            panic!(
                "expected a refusal saying {fragment:?}; got {:?}",
                found.iter().map(said).collect::<Vec<_>>()
            )
        })
}

/// What `function` queued, run whole as the subject's behavior.
fn queued(program: &Program, function: &str, args: &[Value]) -> Vec<WorldCommand> {
    let mut host = Host::new().for_entity(SUBJECT);
    let mut machine = Machine::new(program, function, args).expect("the function exists");
    assert_eq!(machine.run(program, &mut host, u64::MAX), Run::Completed);
    host.commands.as_slice().to_vec()
}

fn patch(fields: &[(&str, ScriptValue)]) -> ScriptValue {
    ScriptValue::Struct(
        fields
            .iter()
            .map(|(name, value)| ((*name).to_owned(), value.clone()))
            .collect(),
    )
}

// ─── What each operation queues ─────────────────────────────────────────────

/// **A patch, not a value.** `Set` sends the fields it names and nothing
/// else: a field left out is not filled with a default, it is left alone.
#[test]
fn a_set_sends_only_the_fields_it_names() {
    let program = build(
        "behavior Anchor {
             void Go() { this.Set(RigidBody { mass: 12.0 }); }
         }",
    );

    assert_eq!(
        queued(&program, "Anchor.Go", &[]),
        vec![WorldCommand::SetComponent {
            entity: SUBJECT,
            component: ComponentName::new("RigidBody"),
            value: patch(&[("mass", ScriptValue::Float(12.0))]),
        }]
    );
}

/// The values are the script's own, computed at run time and converted: an
/// engine type stays itself, a number is the number the script made.
#[test]
fn a_set_carries_the_values_the_script_computed() {
    let program = build(
        "fn void Main(Entity e, float m) {
             e.Set(RigidBody { mass: m * 6.0, initial_velocity: Vec3(1.0, 0.0, 0.0), ccd_enabled: true });
         }",
    );

    assert_eq!(
        queued(
            &program,
            "Main",
            &[Value::Entity(SUBJECT), Value::Float(2.0)]
        ),
        vec![WorldCommand::SetComponent {
            entity: SUBJECT,
            component: ComponentName::new("RigidBody"),
            value: patch(&[
                ("mass", ScriptValue::Float(12.0)),
                (
                    "initial_velocity",
                    ScriptValue::Vec3(Vec3::new(1.0, 0.0, 0.0))
                ),
                ("ccd_enabled", ScriptValue::Bool(true)),
            ]),
        }]
    );
}

/// The fields go out in the order the author wrote them, which is the order a
/// diagnostic about them should echo back.
#[test]
fn a_patch_keeps_the_order_the_fields_were_written_in() {
    let program = build("fn void Main(Entity e) { e.Set(Health { max: 10, current: 5 }); }");

    assert_eq!(
        queued(&program, "Main", &[Value::Entity(SUBJECT)]),
        vec![WorldCommand::SetComponent {
            entity: SUBJECT,
            component: ComponentName::new("Health"),
            value: patch(&[
                ("max", ScriptValue::Int(10)),
                ("current", ScriptValue::Int(5))
            ]),
        }]
    );
}

/// `Add` attaches — the engine starts from the component's defaults and
/// applies the patch over them — and `Remove` names the component alone.
#[test]
fn add_and_remove_queue_their_commands() {
    let program = build(
        "behavior Anchor {
             void Grip() { this.Add(Collider { friction: 0.8 }); }
             void Release() { this.Remove(Collider); }
         }",
    );

    assert_eq!(
        queued(&program, "Anchor.Grip", &[]),
        vec![WorldCommand::AddComponent {
            entity: SUBJECT,
            component: ComponentName::new("Collider"),
            value: patch(&[("friction", ScriptValue::Float(0.8))]),
        }]
    );
    assert_eq!(
        queued(&program, "Anchor.Release", &[]),
        vec![WorldCommand::RemoveComponent {
            entity: SUBJECT,
            component: ComponentName::new("Collider"),
        }]
    );
}

/// `Spawn` places with the identity rotation and carries each component's
/// patch, in the order written.
#[test]
fn a_spawn_carries_each_component_patch() {
    let program = build(
        "fn void Main() {
             Spawn(Vec3(1.0, 2.0, 3.0), RigidBody { mass: 3.0 }, Collider { friction: 0.8, is_sensor: true });
         }",
    );

    assert_eq!(
        queued(&program, "Main", &[]),
        vec![WorldCommand::Spawn {
            position: Vec3::new(1.0, 2.0, 3.0),
            rotation: Quaternion::IDENTITY,
            components: vec![
                (
                    ComponentName::new("RigidBody"),
                    patch(&[("mass", ScriptValue::Float(3.0))]),
                ),
                (
                    ComponentName::new("Collider"),
                    patch(&[
                        ("friction", ScriptValue::Float(0.8)),
                        ("is_sensor", ScriptValue::Bool(true)),
                    ]),
                ),
            ],
        }]
    );
}

// ─── What the checker refuses ───────────────────────────────────────────────

/// **A component is not a type.** It is written with `Set` and read with
/// `Get`; a local, a parameter, a struct field or a return of that type is
/// refused, saying so.
#[test]
fn a_component_is_not_a_variable_type() {
    let message = "`RigidBody` is a component";
    let local = refused_saying("fn void F() { RigidBody r; }", message);
    let note = said(&local);
    assert!(
        note.contains("Set(RigidBody"),
        "the refusal says how a component is written; got {note:?}"
    );
    refused_saying("fn void F(RigidBody r) { }", message);
    refused_saying("struct Bag { RigidBody body; }", message);
    refused_saying("fn RigidBody F() { return null; }", message);
}

/// A component literal is an argument of `Set`, `Add` or `Spawn` and nothing
/// else — a value nothing writes would be a patch of nothing.
#[test]
fn a_component_literal_is_only_an_argument() {
    refused_saying(
        "fn void F() { var x = RigidBody { mass: 1.0 }; }",
        "RigidBody",
    );
    refused_saying("fn void F() { Log(RigidBody { mass: 1.0 }); }", "RigidBody");
}

/// **A derived component is not writable.** `GlobalTransform` is something the
/// engine computes; the mirror serves it as a plain struct, and a struct is not
/// a component a script writes.
#[test]
fn a_derived_component_is_not_writable() {
    let message = "`GlobalTransform` is not a component a script writes";
    refused_saying(
        "behavior Mover { void Go() { this.Set(GlobalTransform { translation: Vec3(0.0, 0.0, 0.0) }); } }",
        message,
    );
    refused_saying(
        "behavior Mover { void Go() { this.Add(GlobalTransform { translation: Vec3(0.0, 0.0, 0.0) }); } }",
        message,
    );
    refused_saying(
        "fn void Main() { Spawn(Vec3(0.0, 0.0, 0.0), GlobalTransform { translation: Vec3(0.0, 0.0, 0.0) }); }",
        message,
    );
}

/// `Transform` is a struct too, and the refusal points at what places an
/// entity instead — the placement natives, by name.
#[test]
fn a_transform_write_points_at_the_placement_natives() {
    let refusal = refused_saying(
        "behavior Mover {
             void Go() {
                 this.Set(Transform { translation: Vec3(0.0, 1.0, 0.0), rotation: Quat(0.0, 0.0, 0.0, 1.0), scale: Vec3(1.0, 1.0, 1.0) });
             }
         }",
        "SetPosition",
    );
    let text = said(&refusal);
    assert!(
        text.contains("Transform"),
        "the refusal is about `Transform`; got {text:?}"
    );
    for native in ["SetPosition", "Translate", "SetRotation", "SetScale"] {
        assert!(
            text.contains(native),
            "the refusal names `{native}`; got {text:?}"
        );
    }
}

/// **The mistake is underlined where it was written** — the field name, not
/// the statement — whichever operation carries the literal.
#[test]
fn an_unknown_field_is_rejected() {
    for main in [
        "fn void F(Entity e) { e.Set(RigidBody { weight: 1.0 }); }",
        "fn void F(Entity e) { e.Add(RigidBody { weight: 1.0 }); }",
        "fn void F() { Spawn(Vec3(0.0, 0.0, 0.0), RigidBody { weight: 1.0 }); }",
    ] {
        let refusal = refused_saying(main, "weight");
        let source = format!("import \"{ENGINE_COMPONENTS_MODULE}\";\n{main}");
        let underlined = &source[refusal.span.start as usize..refusal.span.end as usize];
        assert_eq!(underlined, "weight", "in {main}");
    }
}

/// Every value must fit its field.
#[test]
fn a_field_of_the_wrong_type_is_rejected() {
    refused_saying(
        r#"fn void F(Entity e) { e.Set(RigidBody { mass: "heavy" }); }"#,
        "expected `float`, found `string`",
    );
    refused_saying(
        r#"fn void F() { Spawn(Vec3(0.0, 0.0, 0.0), Health { current: 1.5 }); }"#,
        "expected `int`, found `float`",
    );
}

/// `Remove` takes a component's name — not a literal, not a plain struct.
#[test]
fn remove_takes_a_bare_component_name() {
    refused_saying(
        "fn void F(Entity e) { e.Remove(RigidBody { mass: 1.0 }); }",
        "RigidBody",
    );
    refused_saying(
        "fn void F(Entity e) { e.Remove(GlobalTransform); }",
        "GlobalTransform",
    );
}

/// `Spawn`'s first argument is the position; the rest are component literals,
/// none twice.
#[test]
fn spawn_takes_a_position_then_distinct_components() {
    refusals("fn void F() { Spawn(RigidBody { mass: 1.0 }); }");
    refused_saying(
        "fn void F() { Spawn(Vec3(0.0, 0.0, 0.0), RigidBody { mass: 1.0 }, RigidBody { mass: 2.0 }); }",
        "RigidBody",
    );
}

/// **Only the engine declares components, until scripts can.** A `component`
/// in the script's own module, or in a module it imports, is refused.
#[test]
fn a_component_declared_in_a_script_is_refused() {
    let message = "declaring a component in a script is not supported yet";
    refused_saying("component Shield { int charge; }", message);

    let control = compile("fn void Probe() { }");
    assert!(control.succeeded(), "{:?}", errors_of(&control));
    let imported = compile_with(
        "import \"gear/shield.erg\";",
        &[("gear/shield.erg", "component Shield { int charge; }")],
    );
    assert!(
        imported
            .diagnostics
            .iter()
            .any(|d| d.message.contains(message)),
        "got {:?}",
        imported.diagnostics
    );
}

// ─── Cost and identity ──────────────────────────────────────────────────────

/// **A write pays for its size.** The instruction that writes a component
/// costs one plus the fields it carries — a `Spawn`, one plus every
/// component's fields — and a run is charged what its instructions cost.
#[test]
fn a_write_costs_one_plus_its_fields() {
    let program = build(
        "fn void One(Entity e, float x) { e.Set(Trio { a: x }); }
         fn void Two(Entity e, float x) { e.Set(Trio { a: x, b: x }); }
         fn void Three(Entity e, float x) { e.Add(Trio { a: x, b: x, c: x }); }
         fn void Spawned(Vec3 p, float x) { Spawn(p, Trio { a: x, b: x }, Twin { a: x }); }",
    );

    for (function, fields) in [("One", 1), ("Two", 2), ("Three", 3), ("Spawned", 3)] {
        let code = &program.function(function).expect("compiled").code;
        let sized: Vec<u64> = code
            .iter()
            .map(|instruction| instruction.cost())
            .filter(|cost| *cost > 1)
            .collect();
        assert_eq!(
            sized,
            vec![1 + fields],
            "`{function}` has one write, costing 1 + {fields}"
        );

        let args = if function == "Spawned" {
            vec![Value::Vec3(Vec3::ZERO), Value::Float(1.0)]
        } else {
            vec![Value::Entity(SUBJECT), Value::Float(1.0)]
        };
        let mut host = Host::new().for_entity(SUBJECT);
        let mut machine = Machine::new(&program, function, &args).expect("the function exists");
        let (run, spent) = machine.run_counting(&program, &mut host, u64::MAX);
        assert_eq!(run, Run::Completed);
        let declared: u64 = code.iter().map(|instruction| instruction.cost()).sum();
        assert_eq!(
            spent, declared,
            "`{function}` is charged what its instructions cost"
        );
    }
}

/// **A write is named in its fingerprint, never numbered.** Changing the
/// component, a field or the operation changes the function's fingerprint;
/// moving where its patch or its component's name sits in the program — by
/// another write compiled before it — does not.
#[test]
fn a_write_hashes_its_names() {
    fn fingerprint(main: &str, function: &str) -> u64 {
        build(main)
            .function(function)
            .expect("compiled")
            .fingerprint
    }

    let set = "fn void F(Entity e) { e.Set(Trio { a: 1.0 }); }";
    let base = fingerprint(set, "F");

    assert_ne!(
        base,
        fingerprint("fn void F(Entity e) { e.Set(Twin { a: 1.0 }); }", "F"),
        "another component"
    );
    assert_ne!(
        base,
        fingerprint("fn void F(Entity e) { e.Set(Trio { b: 1.0 }); }", "F"),
        "another field"
    );
    assert_ne!(
        base,
        fingerprint("fn void F(Entity e) { e.Add(Trio { a: 1.0 }); }", "F"),
        "another operation"
    );
    assert_eq!(
        base,
        fingerprint(
            "fn void G(Entity e) { e.Set(Health { current: 1, max: 2 }); e.Remove(Collider); }
             fn void F(Entity e) { e.Set(Trio { a: 1.0 }); }",
            "F"
        ),
        "the same write, its patch elsewhere in the program"
    );

    let remove = "fn void F(Entity e) { e.Remove(Trio); }";
    assert_ne!(
        fingerprint(remove, "F"),
        fingerprint("fn void F(Entity e) { e.Remove(Twin); }", "F"),
        "another component removed"
    );
    assert_eq!(
        fingerprint(remove, "F"),
        fingerprint(
            r#"fn string G() { return "before"; }
               fn void H(Entity e) { e.Remove(Collider); }
               fn void F(Entity e) { e.Remove(Trio); }"#,
            "F"
        ),
        "the same removal, its name elsewhere in the string table"
    );

    let spawn = "fn void F() { Spawn(Vec3(0.0, 0.0, 0.0), Trio { a: 1.0 }); }";
    assert_ne!(
        fingerprint(spawn, "F"),
        fingerprint(
            "fn void F() { Spawn(Vec3(0.0, 0.0, 0.0), Twin { a: 1.0 }); }",
            "F"
        ),
        "another component spawned"
    );
    assert_eq!(
        fingerprint(spawn, "F"),
        fingerprint(
            "fn void G() { Spawn(Vec3(0.0, 0.0, 0.0), Health { current: 1 }); }
             fn void F() { Spawn(Vec3(0.0, 0.0, 0.0), Trio { a: 1.0 }); }",
            "F"
        ),
        "the same spawn, its patches elsewhere in the program"
    );
}
