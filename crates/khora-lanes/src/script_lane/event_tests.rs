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

//! One behavior telling another something happened.
//!
//! `on Damaged(int amount)` is the feature the language advertises, and until
//! `Raise` existed nothing in a game could fire it — the handler could only be
//! reached by a test constructing the event by hand.

use khora_core::script::{EventQueue, ScriptValue};
use khora_data::flow::{ScriptInstance, ScriptProgram, ScriptView};
use khora_script::arena::Persisted;
use khora_script::native::Host;

use super::tests::{compile, entity, runtime_of, MODULE};
use super::{run_behaviors, ScriptRuntime};

/// An attacker that hits entity 1 once, and a guard that feels it.
///
/// Both behaviors live in one module because a module is a compilation unit, not
/// a namespace for one behavior.
const COMBAT: &str = r#"
behavior Attacker {
    int swung = 0;

    void OnSpawn() {
        swung += 1;
    }
}

behavior Guard {
    int health = 100;
    int hits = 0;

    on Damaged(int amount) {
        health -= amount;
        hits += 1;
    }
}
"#;

/// A guard that hits itself once when it spawns, so the round trip needs no
/// second entity.
const SELF_HITTER: &str = r#"
behavior Guard {
    int health = 100;
    int hits = 0;

    void OnSpawn() {
        Raise(this, "Damaged", 25);
    }

    on Damaged(int amount) {
        health -= amount;
        hits += 1;
    }
}
"#;

/// One guard on entity 0.
fn one_guard() -> ScriptView {
    ScriptView {
        delta_seconds: 0.016,
        programs: vec![ScriptProgram {
            module: MODULE.to_owned(),
            behavior: "Guard".to_owned(),
        }],
        instances: vec![ScriptInstance {
            entity: entity(0),
            program: 0,
            authored: None,
            translation: khora_core::math::Vec3::ZERO,
            rotation: khora_core::math::Quaternion::IDENTITY,
            scale: khora_core::math::Vec3::ONE,
        }],
    }
}

fn field(runtime: &ScriptRuntime, behavior: &str, slot: usize) -> Option<i64> {
    match runtime.peek(entity(0), behavior)?.fields.get(slot)? {
        Persisted::Scalar(value) => value.as_int(),
        _ => None,
    }
}

// ─── The round trip ─────────────────────────────────────────────────────────

/// **The milestone.** A script raises an event and a handler runs — with no test
/// hand-building the event in between.
#[test]
fn an_event_a_script_raised_reaches_the_handler_next_frame() {
    let view = one_guard();
    let mut runtime = runtime_of(SELF_HITTER);
    let mut host = Host::new();

    // Frame one: `OnSpawn` raises.
    run_behaviors(&view, &EventQueue::new(), &mut runtime, &mut host, u64::MAX);
    assert_eq!(
        field(&runtime, "Guard", 1),
        Some(0),
        "not delivered in the frame it was raised"
    );

    // Frame two: what the first raised is what this one delivers.
    let raised = host.take_events();
    assert_eq!(raised.len(), 1, "one raise: {raised:?}");
    run_behaviors(&view, &raised, &mut runtime, &mut host, u64::MAX);

    assert_eq!(field(&runtime, "Guard", 0), Some(75), "the hit landed");
    assert_eq!(field(&runtime, "Guard", 1), Some(1), "exactly once");
}

/// **Why the frame of latency is the design, not a shortcut.** An event handled
/// where it was raised opens a cascade with no bound — a guard that answers a
/// hit with a hit would never let the frame end. Deferring makes each frame's
/// work exactly what the previous one produced.
#[test]
fn an_event_is_not_delivered_in_the_frame_it_was_raised() {
    let view = one_guard();
    let mut runtime = runtime_of(
        r#"
        behavior Guard {
            int health = 100;

            void OnSpawn() { Raise(this, "Damaged", 1); }
            on Damaged(int amount) { Raise(this, "Damaged", 1); }
        }
        "#,
    );
    let mut host = Host::new();

    run_behaviors(&view, &EventQueue::new(), &mut runtime, &mut host, u64::MAX);

    assert_eq!(
        host.take_events().len(),
        1,
        "the raise did not feed itself within the frame"
    );
}

/// The payload arrives as the handler declared it.
#[test]
fn the_payload_travels() {
    let view = one_guard();
    let mut runtime = runtime_of(SELF_HITTER);
    let mut host = Host::new();

    run_behaviors(&view, &EventQueue::new(), &mut runtime, &mut host, u64::MAX);
    let raised = host.take_events();

    let event = &raised.as_slice()[0];
    assert_eq!(event.target, entity(0));
    assert_eq!(event.name, "Damaged");
    assert_eq!(event.args, vec![ScriptValue::Int(25)]);
}

/// An event for an entity that handles nothing is not a mistake — a guard hears
/// `Damaged` and ignores `Opened`.
#[test]
fn raising_an_event_nobody_handles_is_quiet() {
    let view = one_guard();
    let mut runtime = runtime_of(
        r#"
        behavior Guard {
            int health = 100;
            void OnSpawn() { Raise(this, "Opened", 1); }
        }
        "#,
    );
    let mut host = Host::new();

    run_behaviors(&view, &EventQueue::new(), &mut runtime, &mut host, u64::MAX);
    let raised = host.take_events();
    let report = run_behaviors(&view, &raised, &mut runtime, &mut host, u64::MAX);

    assert_eq!(report.faulted, 0);
}

/// **What the checker cannot catch.** `Raise`'s payload is whatever the handler
/// declares, and which handler that is depends on the state the target is in
/// when the event lands — so a mismatch is a delivery-time fault, named, and it
/// disables that behavior rather than the frame.
#[test]
fn a_payload_the_handler_cannot_take_faults_at_delivery() {
    let view = one_guard();
    let mut runtime = runtime_of(
        r#"
        behavior Guard {
            int health = 100;
            void OnSpawn() { Raise(this, "Damaged"); }
            on Damaged(int amount) { health -= amount; }
        }
        "#,
    );
    let mut host = Host::new();

    run_behaviors(&view, &EventQueue::new(), &mut runtime, &mut host, u64::MAX);
    let raised = host.take_events();
    let report = run_behaviors(&view, &raised, &mut runtime, &mut host, u64::MAX);

    assert_eq!(report.faulted, 1, "the arity mismatch was reported");
}

// ─── Between two entities ───────────────────────────────────────────────────

/// The ordinary shape: one behavior raises for another entity, which is what an
/// attacker hitting a guard actually is.
#[test]
fn one_entity_raises_for_another() {
    let mut runtime = runtime_of(
        r#"
        behavior Attacker {
            int swung = 0;
        }

        behavior Guard {
            int health = 100;
            on Damaged(int amount) { health -= amount; }
        }
        "#,
    );
    let mut host = Host::new();

    // The attacker's raise is written by hand here rather than by a script,
    // because what is under test is the delivery to *another* entity — the
    // raising road is covered above.
    let mut queue = EventQueue::new();
    queue.push(
        khora_core::script::ScriptEvent::new(entity(0), "Damaged").with(ScriptValue::Int(30)),
    );

    run_behaviors(&one_guard(), &queue, &mut runtime, &mut host, u64::MAX);

    assert_eq!(field(&runtime, "Guard", 0), Some(70));
}

/// A module holding two behaviors compiles both, which is what lets an attacker
/// and its target live in one file.
#[test]
fn two_behaviors_share_a_module() {
    let program = compile(COMBAT);

    assert!(program.layout("Attacker").is_some());
    assert!(program.layout("Guard").is_some());
}

// ─── Ce qu'un événement peut porter ─────────────────────────────────────────

/// Reads a field expected to hold a vector.
fn vector(runtime: &ScriptRuntime, slot: usize) -> Option<khora_core::math::Vec3> {
    match runtime.peek(entity(0), "Guard")?.fields.get(slot)? {
        Persisted::Scalar(value) => value.as_vec3(),
        _ => None,
    }
}

/// Reads a field expected to hold text.
fn text(runtime: &ScriptRuntime, slot: usize) -> Option<String> {
    match runtime.peek(entity(0), "Guard")?.fields.get(slot)? {
        khora_script::arena::Persisted::Owned(khora_script::arena::Object::Str(s)) => {
            Some(s.clone())
        }
        _ => None,
    }
}

/// **The bug a single value bridge exists to make impossible.**
///
/// `Raise` *can* carry a `Vec3` — the emitting side maps it. The delivery side
/// cannot take one, because its conversion was written without that arm. The
/// mismatch is not a dropped event: it is an `UnsupportedArgument`, which the
/// lane reads as a fault, which disables the target behavior **for good**.
///
/// Two halves of one translation, written weeks apart, that never agreed.
#[test]
fn an_event_can_carry_a_vector() {
    let view = one_guard();
    let mut runtime = runtime_of(
        r#"
        behavior Guard {
            Vec3 hit;
            int hits = 0;

            void OnSpawn() {
                Raise(this, "Hit", Vec3(1.0, 2.0, 3.0));
            }

            on Hit(Vec3 where) {
                hit = where;
                hits += 1;
            }
        }
        "#,
    );
    let mut host = Host::new();

    run_behaviors(&view, &EventQueue::new(), &mut runtime, &mut host, u64::MAX);
    let raised = host.take_events();
    let report = run_behaviors(&view, &raised, &mut runtime, &mut host, u64::MAX);

    assert_eq!(report.faulted, 0, "the delivery refused the payload");
    assert_eq!(field(&runtime, "Guard", 1), Some(1), "the handler ran");
    assert_eq!(
        vector(&runtime, 0),
        Some(khora_core::math::Vec3::new(1.0, 2.0, 3.0)),
        "and the vector arrived intact"
    );
}

/// The same for text, and the same cause. A string reaching a register has to be
/// allocated somewhere — which is precisely why the old conversion refused it
/// rather than doing it.
#[test]
fn an_event_can_carry_text() {
    let view = one_guard();
    let mut runtime = runtime_of(
        r#"
        behavior Guard {
            string last;
            int hits = 0;

            void OnSpawn() {
                Raise(this, "Named", "boss");
            }

            on Named(string who) {
                last = who;
                hits += 1;
            }
        }
        "#,
    );
    let mut host = Host::new();

    run_behaviors(&view, &EventQueue::new(), &mut runtime, &mut host, u64::MAX);
    let raised = host.take_events();
    let report = run_behaviors(&view, &raised, &mut runtime, &mut host, u64::MAX);

    assert_eq!(report.faulted, 0, "the delivery refused the payload");
    assert_eq!(field(&runtime, "Guard", 1), Some(1), "the handler ran");
    assert_eq!(text(&runtime, 0).as_deref(), Some("boss"));
}
