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

//! **What reaches a handler, and what does not** — an event is delivered by
//! the program's table of declared events, not by a member's name.
//!
//! - The handler that runs is the one it always was: the current state's,
//!   then the behavior's own.
//! - Only an event the program declares is delivered. A `ScriptEvent` named
//!   like a method or a lifecycle hook (`Update`, `OnSpawn`, …) never runs
//!   that member: it is "no handler".
//! - The payload is checked against the event's declared signature: a count
//!   or a kind it does not allow is refused as a mismatch before any code
//!   runs; an `int` reaches a `float` parameter as a float.
//! - `Touched` / `Separated` still carry `Entity other`.

use khora_core::ecs::entity::EntityId;
use khora_core::script::{ScriptEvent, ScriptValue, WorldCommand};
use khora_script::arena::Persisted;
use khora_script::dispatch::{deliver, handles, NotDelivered};
use khora_script::native::Host;
use khora_script::vm::Value;
use khora_script::{compile_module, Diagnostic, MemoryLoader, Program};

const MODULE: &str = "main.erg";

/// `source` compiled the way a project compiles it — through the module
/// pipeline, so the engine's events are in scope.
fn build(source: &str) -> Program {
    let result = compile_module(&MemoryLoader::new().with(MODULE, source), MODULE);
    let errors: Vec<&Diagnostic> = result.diagnostics.iter().filter(|d| d.is_error()).collect();
    assert!(errors.is_empty(), "should compile:\n{source}\n{errors:?}");
    result.program.expect("a program with no errors")
}

fn subject() -> EntityId {
    EntityId {
        index: 1,
        generation: 1,
    }
}

/// A fresh instance of `behavior`, running for the subject.
fn host_of(program: &Program, behavior: &str) -> Host {
    Host::new()
        .with_fields(super::support::fresh_store(program, behavior))
        .for_entity(subject())
}

/// `name` raised at the subject, carrying `args`.
fn event(name: &str, args: &[ScriptValue]) -> ScriptEvent {
    args.iter()
        .cloned()
        .fold(ScriptEvent::new(subject(), name), ScriptEvent::with)
}

/// Delivers `event` to `behavior`, the subject being alive.
fn hand(
    program: &Program,
    behavior: &str,
    host: &mut Host,
    event: &ScriptEvent,
) -> Result<khora_script::dispatch::Delivered, NotDelivered> {
    deliver(program, behavior, event, host, u64::MAX, |_| true)
}

/// Field `name` of `behavior`, as the store holds it.
fn field(program: &Program, behavior: &str, host: &Host, name: &str) -> Option<Value> {
    let slot = program
        .layout(behavior)
        .and_then(|layout| layout.slot_of(name))
        .unwrap_or_else(|| panic!("`{name}` is a field of `{behavior}`"));
    match host.fields.get(slot)? {
        Persisted::Scalar(value) => Some(*value),
        _ => None,
    }
}

fn int(program: &Program, behavior: &str, host: &Host, name: &str) -> Option<i64> {
    field(program, behavior, host, name).and_then(|value| value.as_int())
}

// ─── The handler that runs ──────────────────────────────────────────────────

/// A guard with an `on Damaged` of its own, and another in its `Chase` state.
const STATEFUL: &str = r#"
event Damaged(int amount);
event Alarm;

behavior Guard {
    int health = 100;
    int fury = 0;

    state Patrol {
        int steps = 0;
    }

    state Chase {
        on Damaged(int amount) { fury += amount; }
    }

    on Damaged(int amount) { health -= amount; }
    on Alarm { become Chase; }
}
"#;

/// **The behavior's own handler runs in a state that declares none** for the
/// event.
#[test]
fn a_behaviors_own_handler_runs_in_a_state_that_declares_none() {
    let program = build(STATEFUL);
    let mut host = host_of(&program, "Guard");

    hand(
        &program,
        "Guard",
        &mut host,
        &event("Damaged", &[ScriptValue::Int(10)]),
    )
    .expect("delivered in Patrol");

    assert_eq!(int(&program, "Guard", &host, "health"), Some(90));
    assert_eq!(int(&program, "Guard", &host, "fury"), Some(0));
}

/// **The current state's handler runs instead of the behavior's own** for the
/// same event — the precedence the table must keep.
#[test]
fn a_states_handler_runs_instead_of_the_behaviors_own_for_the_same_event() {
    let program = build(STATEFUL);
    let mut host = host_of(&program, "Guard");
    hand(&program, "Guard", &mut host, &event("Alarm", &[])).expect("delivered");
    assert_eq!(
        khora_script::dispatch::current_state(&program, "Guard", &host),
        Some("Chase"),
        "the premise: chasing"
    );

    hand(
        &program,
        "Guard",
        &mut host,
        &event("Damaged", &[ScriptValue::Int(5)]),
    )
    .expect("delivered in Chase");

    assert_eq!(
        int(&program, "Guard", &host, "fury"),
        Some(5),
        "Chase's ran"
    );
    assert_eq!(
        int(&program, "Guard", &host, "health"),
        Some(100),
        "and the behavior's did not"
    );
}

/// **A program read back from its serialized form delivers as the one that
/// was written** — whatever table delivery reads is the program's, not a
/// cache lost on the way.
#[test]
fn a_program_read_back_from_its_serialized_form_delivers_the_same() {
    let written = build(STATEFUL);
    let text = serde_json::to_string(&written).expect("a program serializes");
    let program: Program = serde_json::from_str(&text).expect("and reads back");
    let mut host = host_of(&program, "Guard");

    hand(
        &program,
        "Guard",
        &mut host,
        &event("Damaged", &[ScriptValue::Int(10)]),
    )
    .expect("delivered by the program read back");
    hand(&program, "Guard", &mut host, &event("Alarm", &[])).expect("delivered");
    hand(
        &program,
        "Guard",
        &mut host,
        &event("Damaged", &[ScriptValue::Int(5)]),
    )
    .expect("delivered in Chase");

    assert_eq!(int(&program, "Guard", &host, "health"), Some(90));
    assert_eq!(int(&program, "Guard", &host, "fury"), Some(5));
}

// ─── Only events are delivered ──────────────────────────────────────────────

/// A guard with members an event must never reach: a plain method and the
/// lifecycle hooks, each counting its runs.
const MEMBERS: &str = r#"
event Damaged(int amount);

behavior Guard {
    int health = 100;
    int hurt = 0;
    int spawned = 0;
    int ticks = 0;
    int farewells = 0;

    void Hurt(int amount) { hurt += amount; }
    void OnSpawn() { spawned += 1; }
    void Update(float dt) { ticks += 1; }
    void OnDespawn() { farewells += 1; }

    on Damaged(int amount) { health -= amount; }
}
"#;

/// Every counter of `MEMBERS`' guard, in declaration order after `health`.
fn counters(program: &Program, host: &Host) -> Vec<Option<i64>> {
    ["hurt", "spawned", "ticks", "farewells"]
        .iter()
        .map(|name| int(program, "Guard", host, name))
        .collect()
}

/// **An event named like a plain method never runs it** — it is not an
/// event of the program, so it is "no handler".
#[test]
fn an_event_named_like_a_method_does_not_run_the_method() {
    let program = build(MEMBERS);
    let mut host = host_of(&program, "Guard");
    let before = counters(&program, &host);

    let refused = hand(
        &program,
        "Guard",
        &mut host,
        &event("Hurt", &[ScriptValue::Int(5)]),
    )
    .expect_err("`Hurt` is a method, not an event");

    assert!(
        matches!(refused, NotDelivered::NoHandler { .. }),
        "{refused:?}"
    );
    assert_eq!(counters(&program, &host), before, "the method did not run");
}

/// **An event named like a lifecycle hook never runs it**: `Update`,
/// `OnSpawn`, `OnDespawn` — with the payload each hook would take.
#[test]
fn an_event_named_like_a_lifecycle_hook_does_not_run_the_hook() {
    let program = build(MEMBERS);
    let mut host = host_of(&program, "Guard");
    let before = counters(&program, &host);

    for raised in [
        event("Update", &[ScriptValue::Float(0.1)]),
        event("OnSpawn", &[]),
        event("OnDespawn", &[]),
    ] {
        let refused = hand(&program, "Guard", &mut host, &raised)
            .expect_err("a lifecycle hook is not an event");
        assert!(
            matches!(refused, NotDelivered::NoHandler { .. }),
            "`{}`: {refused:?}",
            raised.name
        );
    }

    assert_eq!(counters(&program, &host), before, "no hook ran");
}

/// **A state's own `Update` is not reached by an event either.**
#[test]
fn a_states_method_is_not_reached_by_an_event() {
    let program = build(
        r#"
        behavior Guard {
            int ticks = 0;

            state Patrol {
                void Update(float dt) { ticks += 1; }
            }
        }
        "#,
    );
    let mut host = host_of(&program, "Guard");

    let refused = hand(
        &program,
        "Guard",
        &mut host,
        &event("Update", &[ScriptValue::Float(0.1)]),
    )
    .expect_err("a state's `Update` is not an event");

    assert!(
        matches!(refused, NotDelivered::NoHandler { .. }),
        "{refused:?}"
    );
    assert_eq!(int(&program, "Guard", &host, "ticks"), Some(0));
}

/// **A behavior does not handle its methods.** Asked what it handles, a guard
/// answers its events only.
#[test]
fn a_behavior_is_not_said_to_handle_its_methods() {
    let program = build(MEMBERS);

    assert!(handles(&program, "Guard", "Damaged"), "the premise");
    for member in ["Hurt", "Update", "OnSpawn", "OnDespawn"] {
        assert!(
            !handles(&program, "Guard", member),
            "`{member}` is a member, not an event"
        );
    }
}

// ─── The payload, checked against the declaration ───────────────────────────

/// **A payload of the wrong kind is a mismatch**, refused before any code
/// runs: no kind narrows into an `int` — not a float, not text, not an
/// entity, not a bool.
#[test]
fn a_payload_of_the_wrong_kind_is_refused_as_a_mismatch() {
    let program = build(MEMBERS);

    for wrong in [
        ScriptValue::Float(2.5),
        ScriptValue::Str("ten".to_owned()),
        ScriptValue::Entity(subject()),
        ScriptValue::Bool(true),
    ] {
        let mut host = host_of(&program, "Guard");
        let kind = wrong.type_name();
        let refused = hand(&program, "Guard", &mut host, &event("Damaged", &[wrong]))
            .expect_err("`Damaged` declares an `int`");

        assert!(
            matches!(refused, NotDelivered::Mismatch { .. }),
            "a {kind} for `int amount`: {refused:?}"
        );
        let said = refused.to_string();
        assert!(
            said.contains("Damaged") && said.contains("int") && said.contains(kind),
            "the refusal names the event, its signature and what came: {said}"
        );
        assert_eq!(
            int(&program, "Guard", &host, "health"),
            Some(100),
            "the handler did not run for a {kind}"
        );
    }
}

/// **Too many values is a mismatch too**, as too few is.
#[test]
fn a_payload_longer_than_the_declaration_is_refused_as_a_mismatch() {
    let program = build(MEMBERS);
    let mut host = host_of(&program, "Guard");

    let refused = hand(
        &program,
        "Guard",
        &mut host,
        &event("Damaged", &[ScriptValue::Int(1), ScriptValue::Int(2)]),
    )
    .expect_err("`Damaged` declares one `int`");

    assert!(
        matches!(refused, NotDelivered::Mismatch { .. }),
        "{refused:?}"
    );
    assert_eq!(int(&program, "Guard", &host, "health"), Some(100));
}

/// **An `int` reaches a `float` parameter as a float** — widened at
/// delivery, so the field it is stored in holds a float.
#[test]
fn an_int_reaches_a_float_parameter_as_a_float() {
    let program = build(
        r#"
        event Pushed(float force);

        behavior Crate {
            float last = 0.0;
            on Pushed(float force) { last = force; }
        }
        "#,
    );
    let mut host = host_of(&program, "Crate");

    hand(
        &program,
        "Crate",
        &mut host,
        &event("Pushed", &[ScriptValue::Int(3)]),
    )
    .expect("an int widens into a float");

    assert_eq!(
        field(&program, "Crate", &host, "last"),
        Some(Value::Float(3.0)),
        "stored as the float the parameter declares"
    );
}

/// A `Duration` travels as a float — the bridge has no duration of its own —
/// so a float payload reaches a `Duration` parameter.
#[test]
fn a_float_reaches_a_duration_parameter() {
    let program = build(
        r#"
        event Stunned(Duration length);

        behavior Guard {
            int stuns = 0;
            on Stunned(Duration length) { stuns += 1; }
        }
        "#,
    );
    let mut host = host_of(&program, "Guard");

    hand(
        &program,
        "Guard",
        &mut host,
        &event("Stunned", &[ScriptValue::Float(0.5)]),
    )
    .expect("a float is what a duration travels as");

    assert_eq!(int(&program, "Guard", &host, "stuns"), Some(1));
}

/// **A handler with no parameter list receives a payload that fits its
/// event**, and ignores it.
#[test]
fn a_handler_without_parameters_receives_a_payload_that_fits() {
    let program = build(
        r#"
        event Damaged(int amount);

        behavior Guard {
            int hits = 0;
            on Damaged { hits += 1; }
        }
        "#,
    );
    let mut host = host_of(&program, "Guard");

    hand(
        &program,
        "Guard",
        &mut host,
        &event("Damaged", &[ScriptValue::Int(7)]),
    )
    .expect("delivered");

    assert_eq!(int(&program, "Guard", &host, "hits"), Some(1));
}

// ─── The engine's events ────────────────────────────────────────────────────

/// A guard that scales whatever touches it or leaves it.
const CONTACTS: &str = r#"
behavior Guard {
    on Touched(Entity other) { other.SetScale(Vec3(2.0, 2.0, 2.0)); }
    on Separated(Entity other) { other.SetScale(Vec3(3.0, 3.0, 3.0)); }
}
"#;

fn scaled(host: &Host, target: EntityId, value: f32) -> bool {
    host.commands.as_slice().iter().any(|command| {
        *command
            == WorldCommand::SetScale {
                entity: target,
                value: khora_core::math::Vec3::new(value, value, value),
            }
    })
}

/// **`Separated` carries the other entity**, as `Touched` does, and reaches a
/// handler taking it.
#[test]
fn separated_reaches_its_handler_with_the_other_entity() {
    let program = build(CONTACTS);
    let mut host = host_of(&program, "Guard");
    let other = EntityId {
        index: 7,
        generation: 1,
    };

    hand(
        &program,
        "Guard",
        &mut host,
        &event("Separated", &[ScriptValue::Entity(other)]),
    )
    .expect("delivered");

    assert!(
        scaled(&host, other, 3.0),
        "the handler was handed the other entity: {:?}",
        host.commands.as_slice()
    );
}

/// **A contact carrying something other than an entity is a mismatch** —
/// the engine's events are checked against their declarations like any.
#[test]
fn a_touched_event_carrying_no_entity_is_refused_as_a_mismatch() {
    let program = build(CONTACTS);
    let mut host = host_of(&program, "Guard");

    let refused = hand(
        &program,
        "Guard",
        &mut host,
        &event("Touched", &[ScriptValue::Int(7)]),
    )
    .expect_err("`Touched` declares an `Entity`");

    assert!(
        matches!(refused, NotDelivered::Mismatch { .. }),
        "{refused:?}"
    );
    assert!(
        host.commands.as_slice().is_empty(),
        "the handler did not run"
    );
}

// ─── A handler without parameters ───────────────────────────────────────────

/// **A payload that does not fit is refused whichever handler would take
/// it** — one written without parameters included. "Dropped and reported"
/// is about the payload, not about the handler: a `Damaged` carrying text is
/// as wrong for `on Damaged { … }` as for `on Damaged(int amount)`.
#[test]
fn a_payload_that_does_not_fit_is_refused_by_a_parameterless_handler_too() {
    let program = build(
        r#"
        event Damaged(int amount);

        behavior Guard {
            int hits = 0;
            on Damaged { hits += 1; }
        }
        "#,
    );
    let mut host = host_of(&program, "Guard");

    let delivered = hand(
        &program,
        "Guard",
        &mut host,
        &event("Damaged", &[ScriptValue::Str("ten".to_owned())]),
    );

    assert!(
        matches!(delivered, Err(NotDelivered::Mismatch { .. })),
        "a string for `Damaged(int)` ran the parameterless handler: {delivered:?}"
    );
    assert_eq!(int(&program, "Guard", &host, "hits"), Some(0));
}
