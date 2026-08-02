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

//! Tests for the world-changing surface.
//!
//! The decisive one: a source program says `Translate(this, …)` and a
//! `WorldCommand` comes out the other side. Every layer between — `this`
//! resolving to an entity, `Vec3` surviving a register, the checker agreeing
//! that a native takes one — has to hold for that to happen at all.

use super::{Host, NativeRegistry};
use crate::bytecode::compile_with;
use crate::lexer::lex;
use crate::parser::parse;
use crate::types::check_with;
use crate::vm::{Fault, Machine, Run, Value};
use khora_core::ecs::entity::EntityId;
use khora_core::math::Vec3;
use khora_core::script::WorldCommand;

fn subject() -> EntityId {
    EntityId {
        index: 7,
        generation: 1,
    }
}

/// Compiles a behavior and runs one of its members, returning the host so a test
/// can look at what it queued.
fn run_member(source: &str, member: &str, host: &mut Host) -> Run {
    let natives = NativeRegistry::discovered();

    let lexed = lex(source);
    assert!(lexed.diagnostics.is_empty(), "{:?}", lexed.diagnostics);
    let parsed = parse(lexed.tokens.clone());
    assert!(parsed.diagnostics.is_empty(), "{:?}", parsed.diagnostics);
    let checked = check_with(&parsed.module, &natives);
    assert!(checked.diagnostics.is_empty(), "{:?}", checked.diagnostics);
    let compiled = compile_with(&parsed.module, &natives);
    assert!(
        compiled.diagnostics.is_empty(),
        "{:?}",
        compiled.diagnostics
    );

    let mut machine = Machine::new(&compiled.program, member, &[]).expect("the member exists");
    machine.run(&compiled.program, host, u64::MAX)
}

/// A host standing where the lane would put one: a subject, and a place.
fn host_at(position: Vec3) -> Host {
    Host {
        entity: Some(subject()),
        position: Some(position),
        ..Host::new()
    }
}

// ─── The whole road ─────────────────────────────────────────────────────────

/// **The milestone.** A script asks for an effect, and the effect is a queued
/// command rather than a write — which is the property the parallel schedule
/// rests on.
#[test]
fn a_script_moves_an_entity_by_queueing_a_command() {
    let mut host = host_at(Vec3::ZERO);
    let outcome = run_member(
        "behavior Mover {
             void Go() { Translate(this, Vec3(0.0, 1.0, 0.0)); }
         }",
        "Mover.Go",
        &mut host,
    );

    assert_eq!(outcome, Run::Completed);
    assert_eq!(
        host.commands.as_slice(),
        &[WorldCommand::Translate {
            entity: subject(),
            delta: Vec3::new(0.0, 1.0, 0.0),
        }]
    );
}

/// `this` is the entity. A behavior is a component on one and has no identity
/// apart from it, so every effect a script asks for is aimed with this.
#[test]
fn this_resolves_to_the_running_entity() {
    let mut host = host_at(Vec3::ZERO);
    run_member(
        "behavior Mover { void Go() { Despawn(this); } }",
        "Mover.Go",
        &mut host,
    );

    assert_eq!(
        host.commands.as_slice(),
        &[WorldCommand::Despawn { entity: subject() }]
    );
}

/// A free function has no subject, and is told so where it is written rather
/// than by faulting at run time.
#[test]
fn this_is_refused_in_a_free_function() {
    let natives = NativeRegistry::discovered();
    let parsed = parse(lex("fn void Go() { Despawn(this); }").tokens);
    let found: Vec<String> = check_with(&parsed.module, &natives)
        .diagnostics
        .into_iter()
        .map(|d| d.message)
        .collect();

    assert!(
        found.iter().any(|m| m.contains("`this`")),
        "expected a complaint about `this`: {found:?}"
    );
}

/// And if a host runs a behavior's member without naming whose it is, the VM
/// says so rather than aiming at entity zero.
#[test]
fn this_faults_when_the_host_named_no_entity() {
    let mut host = Host::new();
    let outcome = run_member(
        "behavior Mover { void Go() { Despawn(this); } }",
        "Mover.Go",
        &mut host,
    );

    assert_eq!(outcome, Run::Faulted(Fault::NoSubject));
    assert!(host.commands.is_empty(), "nothing was queued");
}

// ─── Vectors ────────────────────────────────────────────────────────────────

/// A vector is built from expressions, not just literals — which is the whole
/// point of `speed * dt`.
#[test]
fn a_vector_is_built_from_computed_components() {
    let mut host = host_at(Vec3::ZERO);
    let outcome = run_member(
        "behavior Mover {
             void Go() {
                 float speed = 3.0;
                 float dt = 0.5;
                 SetPosition(this, Vec3(speed * dt, 0.0, 0.0 - speed));
             }
         }",
        "Mover.Go",
        &mut host,
    );

    assert_eq!(outcome, Run::Completed);
    assert_eq!(
        host.commands.as_slice(),
        &[WorldCommand::SetTranslation {
            entity: subject(),
            value: Vec3::new(1.5, 0.0, -3.0),
        }]
    );
}

/// A vector survives a round trip through a register, which is what makes it
/// usable as a local rather than only as an argument.
#[test]
fn a_vector_survives_a_local() {
    let mut host = host_at(Vec3::ZERO);
    run_member(
        "behavior Mover {
             void Go() {
                 Vec3 up = Vec3(0.0, 1.0, 0.0);
                 Translate(this, up);
             }
         }",
        "Mover.Go",
        &mut host,
    );

    assert_eq!(
        host.commands.as_slice(),
        &[WorldCommand::Translate {
            entity: subject(),
            delta: Vec3::new(0.0, 1.0, 0.0),
        }]
    );
}

// ─── Reading ────────────────────────────────────────────────────────────────

/// The read side: where the frame placed this entity.
#[test]
fn position_answers_with_what_the_frame_projected() {
    let mut host = host_at(Vec3::new(1.0, 2.0, 3.0));
    run_member(
        "behavior Mover { void Go() { SetPosition(this, Position()); } }",
        "Mover.Go",
        &mut host,
    );

    assert_eq!(
        host.commands.as_slice(),
        &[WorldCommand::SetTranslation {
            entity: subject(),
            value: Vec3::new(1.0, 2.0, 3.0),
        }]
    );
}

/// **The rule a script must be told, not left to discover.** Commands apply at
/// the frame boundary, so a write is not readable back within the same turn.
#[test]
fn a_write_is_not_readable_back_in_the_same_turn() {
    let mut host = host_at(Vec3::ZERO);
    run_member(
        "behavior Mover {
             void Go() {
                 Translate(this, Vec3(5.0, 0.0, 0.0));
                 SetPosition(this, Position());
             }
         }",
        "Mover.Go",
        &mut host,
    );

    let queued = host.commands.as_slice();
    assert_eq!(queued.len(), 2);
    assert_eq!(
        queued[1],
        WorldCommand::SetTranslation {
            entity: subject(),
            value: Vec3::ZERO,
        },
        "the second call read the frame's world, not the first call's request"
    );
}

/// With no pose to answer from, it faults rather than reporting the origin — a
/// guard that believes it is at (0,0,0) walks somewhere nobody asked.
#[test]
fn position_faults_when_the_frame_placed_nothing() {
    let mut host = Host {
        entity: Some(subject()),
        ..Host::new()
    };
    let outcome = run_member(
        "behavior Mover { void Go() { SetPosition(this, Position()); } }",
        "Mover.Go",
        &mut host,
    );

    assert!(
        matches!(outcome, Run::Faulted(Fault::NativeFailed { name, .. }) if name == "Position"),
        "got {outcome:?}"
    );
}

// ─── Hierarchy ──────────────────────────────────────────────────────────────

/// Detaching is its own call rather than `SetParent(e, null)`: an optional
/// argument would make every hierarchy call carry a nullable to express what is
/// a different intent anyway.
#[test]
fn attaching_and_detaching_are_two_calls() {
    let mut host = host_at(Vec3::ZERO);
    run_member(
        "behavior Mover {
             void Go() {
                 SetParent(this, this);
                 Detach(this);
             }
         }",
        "Mover.Go",
        &mut host,
    );

    assert_eq!(
        host.commands.as_slice(),
        &[
            WorldCommand::SetParent {
                entity: subject(),
                parent: Some(subject()),
            },
            WorldCommand::SetParent {
                entity: subject(),
                parent: None,
            },
        ]
    );
}

// ─── The surface itself ─────────────────────────────────────────────────────

/// Discovered like anything else `#[ergon_fn]` registers — the world surface is
/// not a special case in the registry, only in what it does.
#[test]
fn the_world_functions_are_in_the_discovered_registry() {
    let registry = NativeRegistry::discovered();
    for name in [
        "Vec3",
        "Position",
        "Translate",
        "SetPosition",
        "SetScale",
        "Despawn",
        "SetParent",
        "Detach",
    ] {
        assert!(registry.get(name).is_some(), "`{name}` is not exposed");
    }
}

/// A register holds a vector, so the value that reaches a native is the one the
/// script built — no arena, nothing to expire.
#[test]
fn a_vector_is_a_plain_register_value() {
    assert_eq!(
        Value::Vec3(Vec3::new(1.0, 2.0, 3.0)).as_vec3(),
        Some(Vec3::new(1.0, 2.0, 3.0))
    );
    assert_eq!(Value::Int(1).as_vec3(), None);
}
