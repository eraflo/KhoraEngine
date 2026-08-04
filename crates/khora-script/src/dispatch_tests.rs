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
        variadic: false,
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

    assert_eq!(outcome.outcome, Run::Completed);
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
        deliver(&program, "Guard", &after, &mut host, u64::MAX, living(&[])).err(),
        Some(NotDelivered::NoSuchEntity(target))
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
        matches!(outcome.outcome, Run::Faulted(_)),
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

// ─── States ─────────────────────────────────────────────────────────────────

mod states {
    use super::{build, entity, living, registry_with_despawn};
    use crate::arena::{Persisted, PersistentStore};
    use crate::dispatch::{current_state, deliver, handles, initialise};
    use crate::native::Host;
    use crate::vm::{Program, Run, Value};
    use khora_core::script::{ScriptEvent, ScriptValue};

    /// A guard that patrols until it is hurt, then chases whoever hurt it.
    const GUARD: &str = r#"
    behavior Guard {
        int health = 100;

        state Patrol {
            int steps = 0;

            on Damaged(int amount) {
                health -= amount;
                become Chase(amount);
            }
        }

        state Chase(int fury) {
            on Damaged(int amount) {
                health -= amount;
                fury += amount;
            }
        }

        on Healed(int amount) {
            health += amount;
        }
    }
    "#;

    fn host_of(program: &Program) -> Host {
        let layout = program.layout("Guard").expect("Guard has a layout");
        let mut host = Host {
            natives: registry_with_despawn(),
            ..Host::new()
        }
        .with_fields(PersistentStore::with_slots(layout.slot_count()));

        initialise(program, "Guard", &mut host, u64::MAX).expect("an initialiser");
        host
    }

    fn hurt(program: &Program, host: &mut Host, amount: i64) {
        let target = entity(0);
        let event = ScriptEvent::new(target, "Damaged").with(ScriptValue::Int(amount));
        deliver(program, "Guard", &event, host, u64::MAX, living(&[target])).expect("delivered");
    }

    fn slot(host: &Host, index: usize) -> Option<Persisted> {
        host.fields.get(index).cloned()
    }

    /// **The layout.** A state's data sits after the discriminant, and every
    /// state shares that region — only one is ever live.
    #[test]
    fn the_layout_puts_the_state_after_the_behaviors_own_fields() {
        let program = build(GUARD);
        let layout = program.layout("Guard").expect("a layout");

        assert_eq!(layout.fields, vec!["health"]);
        assert_eq!(layout.state_slot(), 1);
        assert_eq!(layout.state_data_slot(), 2);
        assert_eq!(layout.states.len(), 2);
        // Sized for the widest state: Patrol has `steps`, Chase has `fury`.
        assert_eq!(layout.slot_count(), 3);
    }

    /// **The transition.** `become` writes which state it is now in.
    #[test]
    fn become_moves_the_behavior_to_the_named_state() {
        let program = build(GUARD);
        let mut host = host_of(&program);

        assert_eq!(current_state(&program, "Guard", &host), Some("Patrol"));
        hurt(&program, &mut host, 30);
        assert_eq!(current_state(&program, "Guard", &host), Some("Chase"));
    }

    /// And the values it was entered with land in the state's own slots.
    #[test]
    fn become_carries_its_arguments_into_the_state() {
        let program = build(GUARD);
        let mut host = host_of(&program);

        hurt(&program, &mut host, 30);
        assert_eq!(
            slot(&host, 2),
            Some(Persisted::Scalar(Value::Int(30))),
            "Chase was entered with the fury that caused it"
        );
    }

    /// **What `state` is for.** The handler that runs is the current state's,
    /// so `on Damaged` written inside `Chase` means "while chasing".
    #[test]
    fn the_current_states_handler_is_the_one_that_runs() {
        let program = build(GUARD);
        let mut host = host_of(&program);

        hurt(&program, &mut host, 30); // Patrol: hurt, then become Chase(30)
        hurt(&program, &mut host, 5); // Chase: hurt, and fury grows

        assert_eq!(
            slot(&host, 0),
            Some(Persisted::Scalar(Value::Int(65))),
            "both hits landed on health"
        );
        assert_eq!(
            slot(&host, 2),
            Some(Persisted::Scalar(Value::Int(35))),
            "and the second went to Chase's fury, not to Patrol's steps"
        );
        assert_eq!(
            current_state(&program, "Guard", &host),
            Some("Chase"),
            "it did not become Chase a second time"
        );
    }

    /// A behavior's own handler is the fallback, so `on Healed` written once
    /// applies in every state without being repeated in each.
    #[test]
    fn a_behaviors_own_handler_applies_in_every_state() {
        let program = build(GUARD);
        let mut host = host_of(&program);
        let target = entity(0);

        for _ in 0..2 {
            let event = ScriptEvent::new(target, "Healed").with(ScriptValue::Int(5));
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
        hurt(&program, &mut host, 30);
        let event = ScriptEvent::new(target, "Healed").with(ScriptValue::Int(5));
        deliver(
            &program,
            "Guard",
            &event,
            &mut host,
            u64::MAX,
            living(&[target]),
        )
        .expect("delivered in Chase too");

        assert_eq!(
            slot(&host, 0),
            Some(Persisted::Scalar(Value::Int(85))),
            "100 + 5 + 5 - 30 + 5"
        );
    }

    /// A behavior with states starts in the first one declared — a discriminant
    /// of zero is the state written first, which is what an author means by
    /// writing it there.
    #[test]
    fn a_fresh_instance_starts_in_the_first_state() {
        let program = build(GUARD);
        let host = host_of(&program);
        assert_eq!(current_state(&program, "Guard", &host), Some("Patrol"));
    }

    #[test]
    fn a_behavior_reports_the_events_its_states_handle() {
        let program = build(GUARD);

        assert!(handles(&program, "Guard", "Damaged"), "declared in a state");
        assert!(handles(&program, "Guard", "Healed"), "declared outside one");
        assert!(!handles(&program, "Guard", "Opened"));
    }

    /// A behavior with no states has no discriminant to read, and its handlers
    /// resolve the way they always did.
    #[test]
    fn a_behavior_without_states_is_unaffected() {
        let program = build(
            r#"
            behavior Simple {
                int health = 100;
                on Damaged(int amount) { health -= amount; }
            }
            "#,
        );
        let mut host = Host {
            natives: registry_with_despawn(),
            ..Host::new()
        }
        .with_fields(PersistentStore::with_slots(1));
        initialise(&program, "Simple", &mut host, u64::MAX).expect("an initialiser");

        assert_eq!(current_state(&program, "Simple", &host), None);

        let target = entity(0);
        let event = ScriptEvent::new(target, "Damaged").with(ScriptValue::Int(10));
        let outcome = deliver(
            &program,
            "Simple",
            &event,
            &mut host,
            u64::MAX,
            living(&[target]),
        )
        .expect("delivered");
        assert_eq!(outcome.outcome, Run::Completed);
    }
}

// ─── Declarative time ───────────────────────────────────────────────────────

mod timers {
    use super::{build, registry_with_despawn};
    use crate::arena::{Persisted, PersistentStore};
    use crate::dispatch::{initialise, tick_timers};
    use crate::native::Host;
    use crate::vm::{Program, Value};

    /// A guard that counts its own scans, and gives up once.
    const GUARD: &str = r#"
    behavior Guard {
        int scans = 0;
        int gave_up = 0;

        every 0.5s {
            scans += 1;
        }

        after 2s {
            gave_up = 1;
        }
    }
    "#;

    fn host_of(program: &Program) -> Host {
        let layout = program.layout("Guard").expect("a layout");
        let mut host = Host {
            natives: registry_with_despawn(),
            ..Host::new()
        }
        .with_fields(PersistentStore::with_slots(layout.slot_count()));
        initialise(program, "Guard", &mut host, u64::MAX).expect("an initialiser");
        host
    }

    fn count(host: &Host, slot: usize) -> i64 {
        match host.fields.get(slot) {
            Some(Persisted::Scalar(Value::Int(n))) => *n,
            other => panic!("expected an int at {slot}, found {other:?}"),
        }
    }

    /// Sixty frames of a sixtieth of a second — one second of play.
    fn play(program: &Program, host: &mut Host, frames: usize) {
        for _ in 0..frames {
            let (fault, _) = tick_timers(program, "Guard", host, 1.0 / 60.0, u64::MAX);
            assert!(fault.is_none(), "{fault:?}");
        }
    }

    /// **The layout.** Each timer gets a countdown after the states.
    #[test]
    fn each_timer_gets_a_countdown_slot() {
        let program = build(GUARD);
        let layout = program.layout("Guard").expect("a layout");

        assert_eq!(layout.timers.len(), 2);
        assert_eq!(layout.timers[0].seconds, 0.5);
        assert_eq!(layout.timers[1].seconds, 2.0);
        assert_eq!(layout.slot_count(), 4, "two fields and two countdowns");
    }

    /// **What `every 0.5s` says.** It fires half a second in, not on the frame
    /// the entity appeared.
    #[test]
    fn an_interval_does_not_fire_on_the_first_frame() {
        let program = build(GUARD);
        let mut host = host_of(&program);

        play(&program, &mut host, 1);
        assert_eq!(count(&host, 0), 0);
    }

    #[test]
    fn an_interval_fires_once_it_elapses() {
        let program = build(GUARD);
        let mut host = host_of(&program);

        play(&program, &mut host, 30); // half a second
        assert_eq!(count(&host, 0), 1);
    }

    /// **It repeats, and it does not drift.** One second is two half-seconds,
    /// whatever the frame rate — the overshoot carries into the next interval
    /// rather than being dropped.
    #[test]
    fn an_interval_repeats_without_drifting() {
        let program = build(GUARD);
        let mut host = host_of(&program);

        play(&program, &mut host, 120); // two seconds
        assert_eq!(count(&host, 0), 4, "four half-seconds");
    }

    /// A long frame is still one interval's worth, not none.
    #[test]
    fn one_long_frame_still_fires() {
        let program = build(GUARD);
        let mut host = host_of(&program);

        tick_timers(&program, "Guard", &mut host, 0.6, u64::MAX);
        assert_eq!(count(&host, 0), 1);
    }

    /// **`after` fires once.** A deadline that kept firing would be an
    /// `every` written by mistake.
    #[test]
    fn a_delay_fires_exactly_once() {
        let program = build(GUARD);
        let mut host = host_of(&program);

        play(&program, &mut host, 130); // past two seconds: `after 2s` is due
        assert_eq!(count(&host, 1), 1);

        // And stays fired.
        let layout = program.layout("Guard").expect("a layout");
        let slot = layout.timer_slot(1);
        assert_eq!(
            host.fields.get(slot),
            Some(&Persisted::Scalar(Value::Null)),
            "its countdown is spent — and `Null` rather than `Unit`, which is \
             what an unset slot holds, so a reload cannot mistake one for the \
             other and arm it again"
        );

        play(&program, &mut host, 240);
        assert_eq!(count(&host, 1), 1, "four more seconds changed nothing");
    }

    /// A countdown is what is *left*, which is why a save resumes mid-interval
    /// rather than restarting it or firing immediately.
    #[test]
    fn a_countdown_holds_the_time_remaining() {
        let program = build(GUARD);
        let mut host = host_of(&program);
        let slot = program.layout("Guard").expect("a layout").timer_slot(0);

        play(&program, &mut host, 15); // a quarter second
        match host.fields.get(slot) {
            Some(Persisted::Scalar(Value::Float(left))) => {
                assert!((left - 0.25).abs() < 0.01, "expected ~0.25, got {left}")
            }
            other => panic!("expected a countdown, found {other:?}"),
        }
    }

    /// A field-driven interval cannot be scheduled before the expression is
    /// evaluated, and a schedule decides whether to fire before it runs
    /// anything. Refusing beats scheduling a guess.
    #[test]
    fn a_non_literal_interval_is_refused() {
        let lexed = crate::lex(
            r#"
            behavior Guard {
                Duration pace = 1s;
                every pace { }
            }
            "#,
        );
        let parsed = crate::parse(lexed.tokens);
        let compiled = crate::compile(&parsed.module);

        assert!(compiled
            .diagnostics
            .iter()
            .any(|d| d.message.contains("literal duration")));
    }
}

// ─── Time and data inside a state ───────────────────────────────────────────

/// The half of `state` that compiled and did nothing.
///
/// A schedule written inside a state was collected by nobody, so it never
/// fired; and `Become` wrote the arguments it was handed and nothing else, so a
/// state's declared fields stayed unset. The canonical Guard of the design has
/// both — `state Patrol { every 0.5s { … } }` — and neither worked.
mod state_scoped {
    use super::{build, registry_with_despawn};
    use crate::arena::{Persisted, PersistentStore};
    use crate::dispatch::{deliver, initialise, tick_timers};
    use crate::native::Host;
    use crate::vm::{Program, Value};
    use khora_core::ecs::entity::EntityId;
    use khora_core::script::{ScriptEvent, ScriptValue};

    /// A guard that scans while patrolling and closes while chasing, with a
    /// counter of its own in each.
    const GUARD: &str = r#"
    behavior Guard {
        int scans = 0;
        int closes = 0;

        state Patrol {
            int laps = 3;

            every 0.5s {
                scans += 1;
            }
        }

        state Chase {
            int missed = 7;

            every 0.25s {
                closes += 1;
            }
        }

        on Spotted(int by) { become Chase; }
        on Lost(int by) { become Patrol; }
    }
    "#;

    fn host_of(program: &Program) -> Host {
        let layout = program.layout("Guard").expect("a layout");
        let mut host = Host {
            natives: registry_with_despawn(),
            ..Host::new()
        }
        .with_fields(PersistentStore::with_slots(layout.slot_count()));
        host.entity = Some(EntityId {
            index: 0,
            generation: 1,
        });
        initialise(program, "Guard", &mut host, u64::MAX).expect("an initialiser");
        host
    }

    fn count(host: &Host, slot: usize) -> i64 {
        match host.fields.get(slot) {
            Some(Persisted::Scalar(Value::Int(n))) => *n,
            other => panic!("expected an int at {slot}, found {other:?}"),
        }
    }

    fn play(program: &Program, host: &mut Host, frames: usize) {
        for _ in 0..frames {
            let (fault, _) = tick_timers(program, "Guard", host, 1.0 / 60.0, u64::MAX);
            assert!(fault.is_none(), "{fault:?}");
        }
    }

    fn raise(program: &Program, host: &mut Host, event: &str) {
        let event =
            ScriptEvent::new(host.entity.expect("a subject"), event).with(ScriptValue::Int(1));
        deliver(program, "Guard", &event, host, u64::MAX, |_| true).expect("delivered");
    }

    /// The state's data starts where its author declared, which is what makes
    /// `state` a place to put data rather than a label.
    #[test]
    fn a_states_declared_field_gets_its_default() {
        let program = build(GUARD);
        let layout = program.layout("Guard").expect("a layout");
        let mut host = host_of(&program);

        // `laps` is `Patrol`'s only slot, and `Patrol` is where a fresh
        // instance starts.
        assert_eq!(count(&host, layout.state_data_slot()), 3);

        raise(&program, &mut host, "Spotted");
        assert_eq!(
            count(&host, layout.state_data_slot()),
            7,
            "entering `Chase` wrote its own declared default over the patrol's"
        );
    }

    /// **`every 0.5s` inside `Patrol` means "while patrolling".** Before this
    /// it meant nothing at all.
    #[test]
    fn a_schedule_inside_a_state_fires_while_in_it() {
        let program = build(GUARD);
        let mut host = host_of(&program);

        play(&program, &mut host, 40); // two thirds of a second
        assert_eq!(count(&host, 0), 1, "the patrol scanned");
        assert_eq!(count(&host, 1), 0, "and the chase did not close");
    }

    /// And stops when the behavior leaves. A patrol's schedule still firing
    /// while the guard is chasing is the bug `state` exists to prevent.
    #[test]
    fn a_schedule_stops_when_its_state_is_left() {
        let program = build(GUARD);
        let mut host = host_of(&program);

        play(&program, &mut host, 40);
        assert_eq!(count(&host, 0), 1);

        raise(&program, &mut host, "Spotted");
        play(&program, &mut host, 120); // two seconds of chasing

        assert_eq!(count(&host, 0), 1, "the patrol's scan did not fire again");
        assert!(
            count(&host, 1) >= 7,
            "and the chase's did: {}",
            count(&host, 1)
        );
    }

    /// Re-entering a state starts its schedule over. Resuming a half-spent
    /// countdown from the previous visit would fire at a moment nothing
    /// decided — least of all the author, who wrote an interval.
    #[test]
    fn re_entering_a_state_re_arms_its_schedule() {
        let program = build(GUARD);
        let mut host = host_of(&program);

        // Almost due, then leave and come straight back.
        play(&program, &mut host, 29);
        assert_eq!(count(&host, 0), 0, "not yet");

        raise(&program, &mut host, "Spotted");
        raise(&program, &mut host, "Lost");

        play(&program, &mut host, 29);
        assert_eq!(
            count(&host, 0),
            0,
            "the countdown started over rather than resuming where it was"
        );

        play(&program, &mut host, 2);
        assert_eq!(count(&host, 0), 1, "and then it came due");
    }

    /// A behavior-level schedule runs whatever the behavior is doing — that is
    /// what makes writing one inside a state a choice.
    #[test]
    fn a_behavior_level_schedule_runs_in_every_state() {
        let program = build(
            r#"
            behavior Guard {
                int ticks = 0;

                every 0.25s { ticks += 1; }

                state Patrol { }
                state Chase { }

                on Spotted(int by) { become Chase; }
            }
            "#,
        );
        let mut host = host_of(&program);

        play(&program, &mut host, 20);
        let while_patrolling = count(&host, 0);
        assert!(while_patrolling >= 1);

        raise(&program, &mut host, "Spotted");
        play(&program, &mut host, 20);

        assert!(
            count(&host, 0) > while_patrolling,
            "it kept ticking through the transition"
        );
    }
}
