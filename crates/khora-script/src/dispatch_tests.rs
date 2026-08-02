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

//! Event dispatch tests.
//!
//! The decisive one for this phase: `on` stops firing after a `Despawn`. Not
//! because anything unsubscribed — because delivery asks whether the entity is
//! still there, and nothing remembered otherwise.

use std::collections::HashSet;

use khora_core::ecs::entity::EntityId;
use khora_core::script::{EventQueue, ScriptEvent, ScriptValue, WorldCommand};

use crate::arena::{Persisted, PersistentStore};
use crate::bytecode::compile_with;
use crate::dispatch::{deliver, handles, initialise, NotDelivered};
use crate::lexer::lex;
use crate::native::Host;
use crate::parser::parse;
use crate::types::check_with;
use crate::vm::{Program, Run, Value};

/// Compiles against the registry the host will run with — the same one on both
/// sides, which is the contract the indices depend on.
fn build_with(source: &str, natives: &crate::native::NativeRegistry) -> Program {
    let lexed = lex(source);
    assert!(lexed.diagnostics.is_empty(), "{:?}", lexed.diagnostics);

    let parsed = parse(lexed.tokens.clone());
    assert!(parsed.diagnostics.is_empty(), "{:?}", parsed.diagnostics);

    let checked = check_with(&parsed.module, natives);
    assert!(checked.diagnostics.is_empty(), "{:?}", checked.diagnostics);

    let compiled = compile_with(&parsed.module, natives);
    assert!(
        compiled.diagnostics.is_empty(),
        "{:?}",
        compiled.diagnostics
    );
    compiled.program
}

fn build(source: &str) -> Program {
    build_with(source, &registry_with_despawn())
}

fn entity(index: u32) -> EntityId {
    EntityId {
        index,
        generation: 1,
    }
}

/// A world that knows which entities are alive, and nothing else.
fn living(entities: &[EntityId]) -> impl Fn(EntityId) -> bool + '_ {
    let alive: HashSet<EntityId> = entities.iter().copied().collect();
    move |id| alive.contains(&id)
}

/// A guard that loses health when it is hurt, and reports when it dies.
const GUARD: &str = r#"
behavior Guard {
    int health = 100;

    on Damaged(int amount) {
        health -= amount;
        if (health <= 0) {
            Despawn();
        }
    }

    on Healed(int amount) {
        health += amount;
    }
}
"#;

/// `Despawn()` on the running entity — enough to prove an effect leaves a
/// handler.
fn registry_with_despawn() -> crate::native::NativeRegistry {
    use crate::native::{NativeError, NativeFn, NativeTy};

    static DESPAWN: NativeFn = NativeFn {
        name: "Despawn",
        params: &[],
        result: NativeTy::Void,
        cost: 1,
        call: |context, _| {
            let entity = context
                .entity
                .ok_or_else(|| NativeError::new("`Despawn` needs an entity"))?;
            context.commands.push(WorldCommand::Despawn { entity });
            Ok(Value::Unit)
        },
    };

    let mut natives = crate::native::NativeRegistry::with_builtins();
    natives.register(&DESPAWN);
    natives
}

fn guard_host(health: i64) -> Host {
    let mut fields = PersistentStore::with_slots(1);
    fields.set(0, Persisted::Scalar(Value::Int(health)));

    Host {
        natives: registry_with_despawn(),
        ..Host::new()
    }
    .with_fields(fields)
}

fn health(host: &Host) -> i64 {
    match host.fields.get(0) {
        Some(Persisted::Scalar(value)) => value.as_int().expect("health is an int"),
        other => panic!("expected an int in slot 0, found {other:?}"),
    }
}

// ─── Delivery ───────────────────────────────────────────────────────────────

/// **The milestone.** An event reaches the handler that declared it, and the
/// behavior's field is changed by it.
#[test]
fn an_event_reaches_its_handler_and_changes_a_field() {
    let program = build(GUARD);
    let target = entity(1);
    let mut host = guard_host(100);

    let event = ScriptEvent::new(target, "Damaged").with(ScriptValue::Int(30));
    let outcome = deliver(
        &program,
        "Guard",
        &event,
        &mut host,
        u64::MAX,
        living(&[target]),
    )
    .expect("delivered");

    assert_eq!(outcome.0, Run::Completed);
    assert_eq!(health(&host), 70);
}

/// Fields persist across deliveries — which is what makes them fields rather
/// than locals.
#[test]
fn a_field_carries_from_one_event_to_the_next() {
    let program = build(GUARD);
    let target = entity(1);
    let mut host = guard_host(100);

    for amount in [30, 20] {
        let event = ScriptEvent::new(target, "Damaged").with(ScriptValue::Int(amount));
        deliver(
            &program,
            "Guard",
            &event,
            &mut host,
            u64::MAX,
            living(&[target]),
        )
        .expect("delivered");
    }

    assert_eq!(health(&host), 50);
}

#[test]
fn a_behavior_may_declare_several_handlers() {
    let program = build(GUARD);
    let target = entity(1);
    let mut host = guard_host(50);

    for (name, amount) in [("Damaged", 10), ("Healed", 25)] {
        let event = ScriptEvent::new(target, name).with(ScriptValue::Int(amount));
        deliver(
            &program,
            "Guard",
            &event,
            &mut host,
            u64::MAX,
            living(&[target]),
        )
        .expect("delivered");
    }

    assert_eq!(health(&host), 65);
}

/// A handler reaches the engine the same way anything else does — through a
/// command, never the `World`.
#[test]
fn a_handler_can_queue_an_effect() {
    let program = build(GUARD);
    let target = entity(1);
    let mut host = guard_host(20);

    let event = ScriptEvent::new(target, "Damaged").with(ScriptValue::Int(30));
    deliver(
        &program,
        "Guard",
        &event,
        &mut host,
        u64::MAX,
        living(&[target]),
    )
    .expect("delivered");

    assert_eq!(health(&host), -10);
    assert_eq!(
        host.commands.as_slice(),
        &[WorldCommand::Despawn { entity: target }],
        "the guard asked to be removed"
    );
}

/// The effect is queued for the entity the *event* named, so a native called
/// from a handler acts on the right subject.
#[test]
fn the_handler_runs_for_the_entity_the_event_named() {
    let program = build(GUARD);
    let (first, second) = (entity(1), entity(2));
    let mut host = guard_host(5);

    let event = ScriptEvent::new(second, "Damaged").with(ScriptValue::Int(10));
    deliver(
        &program,
        "Guard",
        &event,
        &mut host,
        u64::MAX,
        living(&[first, second]),
    )
    .expect("delivered");

    assert_eq!(
        host.commands.as_slice()[0].entity(),
        Some(second),
        "not whoever the host last ran"
    );
}

// ─── What is not delivered ──────────────────────────────────────────────────

/// **The lifetime rule.** Nothing unsubscribed — delivery asks whether the
/// entity is still there, and it is not.
#[test]
fn an_event_to_a_despawned_entity_is_not_delivered() {
    let program = build(GUARD);
    let target = entity(1);
    let mut host = guard_host(100);

    let event = ScriptEvent::new(target, "Damaged").with(ScriptValue::Int(30));
    let refused = deliver(&program, "Guard", &event, &mut host, u64::MAX, living(&[]))
        .expect_err("the entity is gone");

    assert_eq!(refused, NotDelivered::NoSuchEntity(target));
    assert_eq!(health(&host), 100, "and the handler did not run");
    assert!(refused.to_string().contains("hears nothing"));
}

/// The whole sequence the milestone describes: hurt, die, and then hear
/// nothing.
#[test]
fn a_handler_stops_firing_once_the_entity_is_gone() {
    let program = build(GUARD);
    let target = entity(1);
    let mut host = guard_host(20);

    // Hurt enough to ask for removal.
    let killing = ScriptEvent::new(target, "Damaged").with(ScriptValue::Int(30));
    deliver(
        &program,
        "Guard",
        &killing,
        &mut host,
        u64::MAX,
        living(&[target]),
    )
    .expect("delivered while alive");
    assert_eq!(host.commands.len(), 1, "it asked to be despawned");

    // The engine applied that command; the entity is no longer alive.
    let after = ScriptEvent::new(target, "Damaged").with(ScriptValue::Int(5));
    assert_eq!(
        deliver(&program, "Guard", &after, &mut host, u64::MAX, living(&[])),
        Err(NotDelivered::NoSuchEntity(target))
    );
    assert_eq!(health(&host), -10, "unchanged by the second event");
}

/// The normal case, not a mistake: a guard hears `Damaged` and ignores
/// `Opened`. A caller distinguishes it rather than logging every one.
#[test]
fn an_event_the_behavior_does_not_handle_is_reported_as_such() {
    let program = build(GUARD);
    let target = entity(1);
    let mut host = guard_host(100);

    let event = ScriptEvent::new(target, "Opened");
    let refused = deliver(
        &program,
        "Guard",
        &event,
        &mut host,
        u64::MAX,
        living(&[target]),
    )
    .expect_err("no such handler");

    assert!(matches!(refused, NotDelivered::NoHandler { .. }));
    assert!(refused.to_string().contains("no `on Opened`"));
}

/// Declared *is* subscribed, and the question has an answer without running
/// anything.
#[test]
fn a_behavior_can_be_asked_what_it_handles() {
    let program = build(GUARD);

    assert!(handles(&program, "Guard", "Damaged"));
    assert!(handles(&program, "Guard", "Healed"));
    assert!(!handles(&program, "Guard", "Opened"));
    assert!(!handles(&program, "Chest", "Damaged"));
}

/// An event carrying the wrong number of arguments means the script was
/// compiled against a different definition of it — which the message says.
#[test]
fn an_event_with_the_wrong_arity_is_refused() {
    let program = build(GUARD);
    let target = entity(1);
    let mut host = guard_host(100);

    let event = ScriptEvent::new(target, "Damaged");
    let refused = deliver(
        &program,
        "Guard",
        &event,
        &mut host,
        u64::MAX,
        living(&[target]),
    )
    .expect_err("Damaged takes one argument");

    assert!(matches!(refused, NotDelivered::WrongArity { .. }));
    assert!(refused.to_string().contains("different definition"));
    assert_eq!(health(&host), 100, "nothing ran");
}

/// An argument a register cannot hold is refused rather than dropped: an event
/// that silently lost its payload would be far harder to see.
#[test]
fn an_argument_a_register_cannot_hold_is_refused() {
    let program = build(GUARD);
    let target = entity(1);
    let mut host = guard_host(100);

    let event =
        ScriptEvent::new(target, "Damaged").with(ScriptValue::Vec3(khora_core::math::Vec3::ONE));
    let refused = deliver(
        &program,
        "Guard",
        &event,
        &mut host,
        u64::MAX,
        living(&[target]),
    )
    .expect_err("a Vec3 does not fit a register yet");

    assert!(matches!(
        refused,
        NotDelivered::UnsupportedArgument { index: 0, .. }
    ));
    assert!(refused.to_string().contains("Vec3"));
}

// ─── The queue ──────────────────────────────────────────────────────────────

#[test]
fn a_queue_keeps_the_order_events_were_raised() {
    let mut queue = EventQueue::new();
    for amount in [1, 2, 3] {
        queue.push(ScriptEvent::new(entity(1), "Damaged").with(ScriptValue::Int(amount)));
    }

    let amounts: Vec<_> = queue
        .as_slice()
        .iter()
        .map(|e| e.args[0].as_int().expect("an int"))
        .collect();
    assert_eq!(amounts, vec![1, 2, 3]);
}

#[test]
fn a_queue_can_be_read_per_entity() {
    let mut queue = EventQueue::new();
    queue.push(ScriptEvent::new(entity(1), "Damaged"));
    queue.push(ScriptEvent::new(entity(2), "Damaged"));
    queue.push(ScriptEvent::new(entity(1), "Healed"));

    let names: Vec<_> = queue
        .for_entity(entity(1))
        .map(|e| e.name.as_str())
        .collect();
    assert_eq!(names, vec!["Damaged", "Healed"]);
}

#[test]
fn draining_empties_the_queue() {
    let mut queue = EventQueue::new();
    queue.push(ScriptEvent::new(entity(1), "Damaged"));

    assert_eq!(queue.drain().count(), 1);
    assert!(queue.is_empty());
}

// ─── Fields ─────────────────────────────────────────────────────────────────

/// A slot the store does not have reads as unset, and arithmetic on it faults
/// rather than inventing a number.
///
/// This is a net, not the mechanism. Sizing an instance's store from the
/// behavior's declared fields — and filling their defaults — belongs to
/// whatever loads the instance, which is where a script that gained a field
/// since the last save gets reconciled. What the VM guarantees is only that a
/// gap is never quietly read as zero.
#[test]
fn a_slot_the_store_does_not_have_is_not_quietly_a_number() {
    let program = build(
        r#"
        behavior Counter {
            int seen = 0;
            int extra = 0;

            on Ping(int by) {
                extra += by;
            }
        }
        "#,
    );

    let target = entity(1);
    // A store from before `extra` existed.
    let mut fields = PersistentStore::with_slots(1);
    fields.set(0, Persisted::Scalar(Value::Int(5)));
    let mut host = Host {
        natives: registry_with_despawn(),
        ..Host::new()
    }
    .with_fields(fields);

    let event = ScriptEvent::new(target, "Ping").with(ScriptValue::Int(2));
    let outcome = deliver(
        &program,
        "Counter",
        &event,
        &mut host,
        u64::MAX,
        living(&[target]),
    )
    .expect("delivered");

    assert!(
        matches!(outcome.0, Run::Faulted(_)),
        "adding to an unset field must not produce a number: {outcome:?}"
    );
    assert_eq!(
        host.fields.get(0),
        Some(&Persisted::Scalar(Value::Int(5))),
        "and the field it did have is untouched"
    );
}

/// **What a declared default is worth.** `int seen = 5;` is an expression, so
/// only running code can produce it — and an instance that skipped that would
/// have unset fields rather than the values its author wrote.
#[test]
fn initialising_gives_an_instance_the_defaults_its_behavior_declares() {
    let program = build(
        r#"
        behavior Counter {
            int seen = 5;
            float ratio = 0.5;
            int untouched;

            on Ping(int by) {
                seen += by;
            }
        }
        "#,
    );

    let target = entity(1);
    let mut host = Host {
        natives: registry_with_despawn(),
        ..Host::new()
    }
    .with_fields(PersistentStore::with_slots(3));

    assert_eq!(
        initialise(&program, "Counter", &mut host, u64::MAX).map(|(run, _)| run),
        Some(Run::Completed)
    );
    assert_eq!(host.fields.get(0), Some(&Persisted::Scalar(Value::Int(5))));
    assert_eq!(
        host.fields.get(1),
        Some(&Persisted::Scalar(Value::Float(0.5)))
    );
    assert_eq!(
        host.fields.get(2),
        Some(&Persisted::Scalar(Value::Int(0))),
        "a field with no written default gets its type's zero, not a hole"
    );

    let event = ScriptEvent::new(target, "Ping").with(ScriptValue::Int(2));
    deliver(
        &program,
        "Counter",
        &event,
        &mut host,
        u64::MAX,
        living(&[target]),
    )
    .expect("delivered");

    assert_eq!(
        host.fields.get(0),
        Some(&Persisted::Scalar(Value::Int(7))),
        "the handler built on the default rather than replacing it"
    );
}

/// A default can be any expression, including a call.
#[test]
fn a_default_may_be_computed() {
    let program = build(
        r#"
        behavior Computed {
            float side = 0.0;
            float diagonal = 0.0;
        }
        "#,
    );

    let mut host = Host {
        natives: registry_with_despawn(),
        ..Host::new()
    }
    .with_fields(PersistentStore::with_slots(2));

    initialise(&program, "Computed", &mut host, u64::MAX).expect("the initialiser exists");
    assert_eq!(
        host.fields.get(1),
        Some(&Persisted::Scalar(Value::Float(0.0)))
    );
}

/// A parameter named like a field wins inside the member that declared it,
/// which is what every language with fields does.
#[test]
fn a_parameter_shadows_a_field_of_the_same_name() {
    let program = build(
        r#"
        behavior Shadowed {
            int amount = 100;

            on Set(int amount) {
                Log("ignored");
            }
        }
        "#,
    );

    let target = entity(1);
    let mut fields = PersistentStore::with_slots(1);
    fields.set(0, Persisted::Scalar(Value::Int(100)));
    let mut host = Host {
        natives: registry_with_despawn(),
        ..Host::new()
    }
    .with_fields(fields);

    let event = ScriptEvent::new(target, "Set").with(ScriptValue::Int(7));
    deliver(
        &program,
        "Shadowed",
        &event,
        &mut host,
        u64::MAX,
        living(&[target]),
    )
    .expect("delivered");

    assert_eq!(
        host.fields.get(0),
        Some(&Persisted::Scalar(Value::Int(100))),
        "the field was not touched by a parameter that shares its name"
    );
}

/// Text in a field is stored by value: an arena handle would be stale by the
/// next frame, and a field is exactly what has to outlive one.
#[test]
fn a_string_field_is_kept_by_value() {
    let program = build(
        r#"
        behavior Named {
            string label = "";

            on Rename(int unused) {
                label = "boss";
            }
        }
        "#,
    );

    let target = entity(1);
    let mut host = Host {
        natives: registry_with_despawn(),
        ..Host::new()
    }
    .with_fields(PersistentStore::with_slots(1));

    let event = ScriptEvent::new(target, "Rename").with(ScriptValue::Int(0));
    deliver(
        &program,
        "Named",
        &event,
        &mut host,
        u64::MAX,
        living(&[target]),
    )
    .expect("delivered");

    assert_eq!(
        host.fields.get(0),
        Some(&Persisted::Owned(crate::arena::Object::Str(
            "boss".to_owned()
        ))),
    );

    // And it survives the frame the arena does not.
    host.end_frame();
    assert_eq!(
        host.fields.get(0),
        Some(&Persisted::Owned(crate::arena::Object::Str(
            "boss".to_owned()
        ))),
    );
}
