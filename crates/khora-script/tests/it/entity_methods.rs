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

//! An engine operation whose first parameter is an `Entity` is called **on**
//! that entity: `this.SetPosition(v)`, `target.Raise("Hit", 3)`,
//! `enemy.Despawn()`. One spelling — the free form is refused, and the refusal
//! writes the method form out. `Spawn` has no entity to be called on and stays
//! a free function. `Set`, `Add`, `Remove`, `Get` and `Has` are the language's
//! own, and no script may declare them.

use khora_core::ecs::entity::EntityId;
use khora_core::math::{Quaternion, Vec3};
use khora_core::script::{ScriptValue, WorldCommand};
use khora_script::vm::{Machine, Program, Run, Value};
use khora_script::{compile_module, CompileOutcome, Diagnostic, Host, MemoryLoader};

/// The entity a behavior runs as.
const SUBJECT: EntityId = EntityId {
    index: 7,
    generation: 1,
};

/// Another entity, for a receiver that is not `this`.
const OTHER: EntityId = EntityId {
    index: 9,
    generation: 2,
};

fn compile(source: &str) -> CompileOutcome {
    compile_module(&MemoryLoader::new().with("main.erg", source), "main.erg")
}

fn build(source: &str) -> Program {
    let result = compile(source);
    let errors: Vec<&Diagnostic> = result.diagnostics.iter().filter(|d| d.is_error()).collect();
    assert!(errors.is_empty(), "should compile: {errors:?}");
    result.program.expect("a program with no errors")
}

/// The errors `source` is refused with; it must be refused.
fn refusals(source: &str) -> Vec<Diagnostic> {
    let result = compile(source);
    assert!(
        !result.succeeded(),
        "should be refused, and compiled:\n{source}"
    );
    result
        .diagnostics
        .into_iter()
        .filter(Diagnostic::is_error)
        .collect()
}

/// What a diagnostic tells the author: its line and its note.
fn said(diagnostic: &Diagnostic) -> String {
    format!(
        "{} | {}",
        diagnostic.message,
        diagnostic.note.as_deref().unwrap_or_default()
    )
}

/// Whether some refusal of `source` says `fragment`.
fn refused_saying(source: &str, fragment: &str) {
    let found = refusals(source);
    assert!(
        found.iter().any(|d| said(d).contains(fragment)),
        "expected a refusal saying {fragment:?}; got {:?}",
        found.iter().map(said).collect::<Vec<_>>()
    );
}

/// Runs `function` with `args` as the subject's behavior; the host afterwards.
fn run(program: &Program, function: &str, args: &[Value]) -> Host {
    let mut host = Host::new().for_entity(SUBJECT);
    let mut machine = Machine::new(program, function, args).expect("the function exists");
    assert_eq!(machine.run(program, &mut host, u64::MAX), Run::Completed);
    host
}

// ─── The method form ────────────────────────────────────────────────────────

/// **The same native, the same command.** `this.SetPosition(v)` queues exactly
/// what `SetPosition(this, v)` queued: the receiver goes where the first
/// argument went.
#[test]
fn a_method_call_resolves_to_the_native() {
    let program = build(
        "behavior Mover {
             void Go() {
                 Vec3 v = Vec3(4.0, 5.0, 6.0);
                 this.SetPosition(v);
             }
         }",
    );

    let host = run(&program, "Mover.Go", &[]);
    assert_eq!(
        host.commands.as_slice(),
        &[WorldCommand::SetTranslation {
            entity: SUBJECT,
            value: Vec3::new(4.0, 5.0, 6.0),
        }]
    );
}

/// Every entity-first native of the world surface is a method of the entity,
/// and the receiver can be any expression of type `Entity` — a parameter as
/// well as `this`.
#[test]
fn every_entity_operation_is_a_method_of_the_entity() {
    let program = build(
        "fn void Main(Entity e) {
             e.Translate(Vec3(0.0, 1.0, 0.0));
             e.SetScale(Vec3(2.0, 2.0, 2.0));
             e.SetParent(e);
             e.Detach();
             e.Despawn();
         }",
    );

    let host = run(&program, "Main", &[Value::Entity(OTHER)]);
    assert_eq!(
        host.commands.as_slice(),
        &[
            WorldCommand::Translate {
                entity: OTHER,
                delta: Vec3::new(0.0, 1.0, 0.0),
            },
            WorldCommand::SetScale {
                entity: OTHER,
                value: Vec3::new(2.0, 2.0, 2.0),
            },
            WorldCommand::SetParent {
                entity: OTHER,
                parent: Some(OTHER),
            },
            WorldCommand::SetParent {
                entity: OTHER,
                parent: None,
            },
            WorldCommand::Despawn { entity: OTHER },
        ]
    );
}

/// `SetRotation` is a native, written as a method like the rest: it turns the
/// entity, replacing its rotation.
#[test]
fn set_rotation_is_a_method_of_the_entity() {
    let program = build(
        "behavior Turret {
             void Update(float dt) { this.SetRotation(Quat(0.0, 0.6, 0.0, 0.8)); }
         }",
    );

    let host = run(&program, "Turret.Update", &[Value::Float(0.016)]);
    assert_eq!(
        host.commands.as_slice(),
        &[WorldCommand::SetRotation {
            entity: SUBJECT,
            value: Quaternion {
                x: 0.0,
                y: 0.6,
                z: 0.0,
                w: 0.8,
            },
        }]
    );
}

// ─── One spelling ───────────────────────────────────────────────────────────

/// **The free form is refused, and the refusal is the rewrite.** The receiver
/// is the first argument as written, the rest follow the dot.
#[test]
fn the_free_form_is_refused_with_its_rewrite() {
    refused_saying(
        "behavior Mover {
             void Go() {
                 Vec3 v = Vec3(1.0, 0.0, 0.0);
                 SetPosition(this, v);
             }
         }",
        "Write `this.SetPosition(v)`",
    );
    refused_saying(
        "fn void Main(Entity child, Entity parent) { SetParent(child, parent); }",
        "Write `child.SetParent(parent)`",
    );
    refused_saying(
        "fn void Main(Entity other) { Despawn(other); }",
        "Write `other.Despawn()`",
    );
}

/// `Raise` is an entity operation too: `target.Raise("Hit", 3)` raises the
/// event on `target`, exactly as the free form did, and the free form is
/// refused with the rewrite.
#[test]
fn raise_is_a_method() {
    let program = build(
        r#"behavior Guard {
               void Hurt() { this.Raise("Hit", 3); }
           }
           fn void Strike(Entity target) { target.Raise("Hit", 3); }"#,
    );

    let mut host = run(&program, "Guard.Hurt", &[]);
    let raised = host.take_events();
    assert_eq!(raised.len(), 1, "one event raised");
    let event = &raised.as_slice()[0];
    assert_eq!(event.target, SUBJECT);
    assert_eq!(event.name, "Hit");
    assert_eq!(event.args, vec![ScriptValue::Int(3)]);

    let mut host = run(&program, "Strike", &[Value::Entity(OTHER)]);
    let raised = host.take_events();
    assert_eq!(raised.len(), 1, "one event raised");
    assert_eq!(raised.as_slice()[0].target, OTHER, "raised on the receiver");

    refused_saying(
        r#"behavior Guard { void Hurt() { Raise(this, "Hit", 3); } }"#,
        r#"Write `this.Raise("Hit", 3)`"#,
    );
}

/// **`Spawn` creates the entity, so there is none to call it on.** It stays a
/// free function — placing with the identity rotation — and the method form is
/// refused.
#[test]
fn spawn_stays_a_free_function() {
    let program = build("fn void Main() { Spawn(Vec3(1.0, 2.0, 3.0)); }");
    let host = run(&program, "Main", &[]);
    assert_eq!(
        host.commands.as_slice(),
        &[WorldCommand::Spawn {
            position: Vec3::new(1.0, 2.0, 3.0),
            rotation: Quaternion::IDENTITY,
            components: Vec::new(),
        }]
    );

    refused_saying(
        "behavior Spawner { void Go() { this.Spawn(Vec3(1.0, 2.0, 3.0)); } }",
        "`Spawn`",
    );
}

// ─── The intrinsics' names ──────────────────────────────────────────────────

/// **`Set`, `Add`, `Remove`, `Get` and `Has` belong to the language.** A free
/// function, a behavior's method or a state's method by one of those names is
/// refused, naming it.
#[test]
fn the_reserved_names_cannot_be_declared() {
    for name in ["Set", "Get", "Has", "Add", "Remove"] {
        let quoted = format!("`{name}`");
        refused_saying(&format!("fn void {name}() {{ }}"), &quoted);
        refused_saying(
            &format!("behavior Guard {{ void {name}() {{ }} }}"),
            &quoted,
        );
        refused_saying(
            &format!("behavior Guard {{ state Idle {{ void {name}() {{ }} }} }}"),
            &quoted,
        );
    }
}
