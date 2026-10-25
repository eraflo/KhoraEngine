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

use khora_core::ecs::entity::EntityId;
use khora_core::problems::{Problem, ProblemSeverity};
use khora_core::script::{EventQueue, ScriptEvent, ScriptValue};
use khora_data::flow::{ScriptInstance, ScriptProgram, ScriptView};
use khora_script::arena::Persisted;
use khora_script::native::Host;

use super::{compile, entity, runtime_of, MODULE};
use crate::script_lane::{run_behaviors, ScriptRuntime};

/// An attacker that hits entity 1 once, and a guard that feels it.
///
/// Both behaviors live in one module because a module is a compilation unit, not
/// a namespace for one behavior.
const COMBAT: &str = r#"
event Damaged(int amount);

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
event Damaged(int amount);

behavior Guard {
    int health = 100;
    int hits = 0;

    void OnSpawn() {
        this.Raise(Damaged(25));
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
        fixed_steps: 0,
        fixed_delta_seconds: 0.0,
        resumed: false,
        delta_seconds: 0.016,
        input: Default::default(),
        components: Default::default(),
        programs: vec![ScriptProgram {
            module: MODULE.to_owned(),
            behavior: "Guard".to_owned(),
        }],
        instances: vec![ScriptInstance {
            entity: entity(0),
            program: 0,
            arrival: None,
            translation: khora_core::math::Vec3::ZERO,
            rotation: khora_core::math::Quaternion::IDENTITY,
            scale: khora_core::math::Vec3::ONE,
            row: Vec::new(),
        }],
    }
}

fn field(runtime: &ScriptRuntime, behavior: &str, slot: usize) -> Option<i64> {
    match runtime.peek(entity(0), behavior)?.fields.get(slot)? {
        Persisted::Scalar(value) => value.as_int(),
        _ => None,
    }
}

/// Whether `behavior` on `subject` was disabled by a fault.
fn disabled(runtime: &ScriptRuntime, subject: EntityId, behavior: &str) -> bool {
    runtime
        .peek(subject, behavior)
        .is_some_and(|instance| instance.disabled)
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
        event Damaged(int amount);

        behavior Guard {
            int health = 100;

            void OnSpawn() { this.Raise(Damaged(1)); }
            on Damaged(int amount) { this.Raise(Damaged(1)); }
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
        event Opened(int by);

        behavior Guard {
            int health = 100;
            void OnSpawn() { this.Raise(Opened(1)); }
        }
        "#,
    );
    let mut host = Host::new();

    run_behaviors(&view, &EventQueue::new(), &mut runtime, &mut host, u64::MAX);
    let raised = host.take_events();
    let report = run_behaviors(&view, &raised, &mut runtime, &mut host, u64::MAX);

    assert_eq!(report.faulted, 0);
}

/// **What the checker cannot catch is dropped, not a fault of the receiver.**
/// An event the engine hands over is checked against its declaration at
/// delivery: a payload that does not fit it never runs the handler, and the
/// behavior that would have heard it is not disabled for it.
#[test]
fn a_payload_the_handler_cannot_take_is_dropped_not_faulted() {
    let view = one_guard();
    let mut runtime = runtime_of(
        r#"
        event Damaged(int amount);

        behavior Guard {
            int health = 100;
            on Damaged(int amount) { health -= amount; }
        }
        "#,
    );
    let mut host = Host::new();

    run_behaviors(&view, &EventQueue::new(), &mut runtime, &mut host, u64::MAX);
    // The payload is missing, as only an engine-raised event can leave it.
    let mut raised = EventQueue::new();
    raised.push(khora_core::script::ScriptEvent::new(entity(0), "Damaged"));
    let report = run_behaviors(&view, &raised, &mut runtime, &mut host, u64::MAX);

    assert_eq!(report.faulted, 0, "the receiver is not faulted: {report:?}");
    assert_eq!(
        field(&runtime, "Guard", 0),
        Some(100),
        "the handler did not run"
    );
    assert!(
        !disabled(&runtime, entity(0), "Guard"),
        "the behavior is still enabled"
    );
}

// ─── Between two entities ───────────────────────────────────────────────────

/// The ordinary shape: one behavior raises for another entity, which is what an
/// attacker hitting a guard actually is.
#[test]
fn one_entity_raises_for_another() {
    let mut runtime = runtime_of(
        r#"
        event Damaged(int amount);

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
    let stored = runtime.peek(entity(0), "Guard")?.fields.get(slot)?;
    if !matches!(stored, khora_script::arena::Persisted::Owned(..)) {
        return None;
    }
    match khora_script::bridge::from_persisted(stored) {
        Ok(Some(khora_core::script::ScriptValue::Str(s))) => Some(s),
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
        event Hit(Vec3 where);

        behavior Guard {
            Vec3 hit;
            int hits = 0;

            void OnSpawn() {
                this.Raise(Hit(Vec3(1.0, 2.0, 3.0)));
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
        event Named(string who);

        behavior Guard {
            string last;
            int hits = 0;

            void OnSpawn() {
                this.Raise(Named("boss"));
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

// ─── Typed events, end to end ───────────────────────────────────────────────

/// `source` compiled the way a project compiles it — through the module
/// pipeline, from a loader serving nothing but that one module — and a
/// runtime holding it.
fn project_of(source: &str) -> (ScriptRuntime, khora_script::vm::Program) {
    let loader = khora_script::MemoryLoader::new().with(MODULE, source);
    let result = khora_script::compile_module(&loader, MODULE);
    let errors: Vec<_> = result.diagnostics.iter().filter(|d| d.is_error()).collect();
    assert!(errors.is_empty(), "should compile: {errors:?}");
    let program = result.program.expect("a program with no errors");
    let mut runtime = ScriptRuntime::new();
    runtime.add_program(MODULE, program.clone());
    (runtime, program)
}

/// Whether `host` queued a `SetScale` of `target` to `value`.
fn scaled(host: &Host, target: khora_core::ecs::entity::EntityId, value: f32) -> bool {
    host.commands.as_slice().iter().any(|command| {
        *command
            == khora_core::script::WorldCommand::SetScale {
                entity: target,
                value: khora_core::math::Vec3::new(value, value, value),
            }
    })
}

/// **A declared event, raised by its declaration, reaches its handler the
/// next frame with its whole payload** — the amount, and the entity it named.
#[test]
fn a_typed_raise_reaches_its_handler_next_frame_with_its_payload() {
    let view = one_guard();
    let (mut runtime, _) = project_of(
        r#"
        event Damaged(int amount, Entity source);

        behavior Guard {
            int health = 100;
            int hits = 0;

            void OnSpawn() { this.Raise(Damaged(25, this)); }

            on Damaged(int amount, Entity source) {
                health -= amount;
                hits += 1;
                source.SetScale(Vec3(2.0, 2.0, 2.0));
            }
        }
        "#,
    );
    let mut host = Host::new();

    run_behaviors(&view, &EventQueue::new(), &mut runtime, &mut host, u64::MAX);
    let raised = host.take_events();
    assert_eq!(raised.len(), 1, "one raise: {raised:?}");
    let event = &raised.as_slice()[0];
    assert_eq!(event.target, entity(0));
    assert_eq!(event.name, "Damaged");
    assert_eq!(
        event.args,
        vec![ScriptValue::Int(25), ScriptValue::Entity(entity(0))]
    );
    assert_eq!(field(&runtime, "Guard", 1), Some(0), "not this frame");

    let report = run_behaviors(&view, &raised, &mut runtime, &mut host, u64::MAX);

    assert_eq!(report.faulted, 0);
    assert_eq!(field(&runtime, "Guard", 0), Some(75), "the amount arrived");
    assert_eq!(field(&runtime, "Guard", 1), Some(1), "exactly once");
    assert!(
        scaled(&host, entity(0), 2.0),
        "the entity arrived: {:?}",
        host.commands.as_slice()
    );
}

/// **A handler with no parameter list hears its event** and ignores the
/// payload — it is neither skipped nor faulted for the payload it did not
/// take.
#[test]
fn a_handler_with_no_parameters_receives_its_event() {
    let view = one_guard();
    let (mut runtime, _) = project_of(
        r#"
        event Damaged(int amount);

        behavior Guard {
            int hits = 0;

            void OnSpawn() { this.Raise(Damaged(25)); }

            on Damaged { hits += 1; }
        }
        "#,
    );
    let mut host = Host::new();

    run_behaviors(&view, &EventQueue::new(), &mut runtime, &mut host, u64::MAX);
    let raised = host.take_events();
    assert_eq!(raised.len(), 1, "one raise: {raised:?}");
    let report = run_behaviors(&view, &raised, &mut runtime, &mut host, u64::MAX);

    assert_eq!(report.faulted, 0, "the payload faulted the handler");
    assert_eq!(field(&runtime, "Guard", 0), Some(1), "the handler ran once");
}

/// **A contact still reaches a behavior that imports nothing.** `Touched` is
/// the engine's event — declared by it, in scope without an import — and the
/// engine raises it by name with the other entity, as `collision_to_script`
/// does.
#[test]
fn touched_reaches_a_behavior_handling_it_without_importing_anything() {
    let (mut runtime, program) = project_of(
        r#"
        behavior Guard {
            int touches = 0;

            on Touched(Entity other) {
                touches += 1;
                other.SetScale(Vec3(3.0, 3.0, 3.0));
            }
        }
        "#,
    );
    assert!(
        program.events.iter().any(|event| event.name == "Touched"
            && event.params == vec![khora_core::script::ErgonType::Entity]),
        "the engine declares `Touched(Entity other)`: {:?}",
        program.events
    );
    let mut host = Host::new();

    let mut queue = EventQueue::new();
    queue.push(
        khora_core::script::ScriptEvent::new(entity(0), "Touched")
            .with(ScriptValue::Entity(entity(5))),
    );
    let report = run_behaviors(&one_guard(), &queue, &mut runtime, &mut host, u64::MAX);

    assert_eq!(report.faulted, 0);
    assert_eq!(field(&runtime, "Guard", 0), Some(1), "the handler ran once");
    assert!(
        scaled(&host, entity(5), 3.0),
        "the other entity arrived: {:?}",
        host.commands.as_slice()
    );
}

// ─── A payload that does not fit, dropped and reported ──────────────────────

/// A guard that counts its frames, and whose health two events move.
const TICKING_GUARD: &str = r#"
event Damaged(int amount);
event Healed(int amount);

behavior Guard {
    int health = 100;
    int ticks = 0;

    void Update(float dt) { ticks += 1; }
    on Damaged(int amount) { health -= amount; }
    on Healed(int amount) { health += amount; }
}
"#;

/// `name` raised at entity 0, carrying `args`.
fn raised_with(name: &str, args: &[ScriptValue]) -> ScriptEvent {
    args.iter()
        .cloned()
        .fold(ScriptEvent::new(entity(0), name), ScriptEvent::with)
}

/// A frame's mail holding `events`, in order.
fn mail(events: &[ScriptEvent]) -> EventQueue {
    let mut queue = EventQueue::new();
    for event in events {
        queue.push(event.clone());
    }
    queue
}

/// The running problems the runtime holds about `subject`.
fn problems_about(runtime: &ScriptRuntime, subject: EntityId) -> Vec<Problem> {
    runtime
        .running_problems()
        .into_iter()
        .filter(|problem| problem.entity == Some(subject))
        .collect()
}

/// **A behavior handed a payload it cannot take keeps running**: no fault in
/// the frame it arrived or after, its `Update` still runs every frame, and
/// its fields are untouched by the event that was dropped.
#[test]
fn a_behavior_sent_a_payload_it_cannot_take_keeps_running() {
    let view = one_guard();
    let mut runtime = runtime_of(TICKING_GUARD);
    let mut host = Host::new();
    run_behaviors(&view, &EventQueue::new(), &mut runtime, &mut host, u64::MAX);
    assert_eq!(
        field(&runtime, "Guard", 1),
        Some(1),
        "the premise: one tick"
    );

    let wrong = mail(&[raised_with(
        "Damaged",
        &[ScriptValue::Str("ten".to_owned())],
    )]);
    let report = run_behaviors(&view, &wrong, &mut runtime, &mut host, u64::MAX);
    assert_eq!(report.faulted, 0, "not faulted for the payload: {report:?}");

    for frame in 0..3 {
        let report = run_behaviors(&view, &EventQueue::new(), &mut runtime, &mut host, u64::MAX);
        assert_eq!(report.faulted, 0, "frame {frame} after the drop");
    }

    assert!(!disabled(&runtime, entity(0), "Guard"), "still enabled");
    assert_eq!(
        field(&runtime, "Guard", 1),
        Some(5),
        "its `Update` ran every frame, the drop's included"
    );
    assert_eq!(
        field(&runtime, "Guard", 0),
        Some(100),
        "the dropped event touched nothing"
    );
}

/// **A `null` payload is dropped like any payload that does not fit** — never
/// a fault of the receiver — and reported naming what came.
#[test]
fn a_null_payload_is_dropped_not_faulted() {
    let view = one_guard();
    let mut runtime = runtime_of(TICKING_GUARD);
    let mut host = Host::new();
    run_behaviors(&view, &EventQueue::new(), &mut runtime, &mut host, u64::MAX);

    let null = mail(&[raised_with("Damaged", &[ScriptValue::Null])]);
    let report = run_behaviors(&view, &null, &mut runtime, &mut host, u64::MAX);

    assert_eq!(report.faulted, 0, "{report:?}");
    assert!(!disabled(&runtime, entity(0), "Guard"));
    assert_eq!(field(&runtime, "Guard", 0), Some(100));
    let reported = problems_about(&runtime, entity(0));
    assert_eq!(reported.len(), 1, "the drop is reported: {reported:?}");
    assert!(
        reported[0].message.contains("null"),
        "naming what came: {}",
        reported[0].message
    );
}

/// **A dropped event does not cost the events after it their handler**, and
/// it is done with: not kept to be offered again.
#[test]
fn a_dropped_event_does_not_keep_the_next_from_its_handler() {
    let view = one_guard();
    let mut runtime = runtime_of(TICKING_GUARD);
    let mut host = Host::new();
    run_behaviors(&view, &EventQueue::new(), &mut runtime, &mut host, u64::MAX);

    let frame = mail(&[
        raised_with("Damaged", &[ScriptValue::Bool(true)]),
        raised_with("Damaged", &[ScriptValue::Int(10)]),
    ]);
    let report = run_behaviors(&view, &frame, &mut runtime, &mut host, u64::MAX);

    assert_eq!(report.faulted, 0, "{report:?}");
    assert_eq!(field(&runtime, "Guard", 0), Some(90), "the second landed");
    assert!(
        report.undelivered.is_empty(),
        "the dropped one is not offered again: {:?}",
        report.undelivered
    );
}

/// **An engine-raised event named like a member never runs it** — a plain
/// method, or a lifecycle hook: none is an event of the program, so each is
/// "no handler" — no fault, and nothing to report.
#[test]
fn an_engine_event_named_like_a_member_never_runs_it() {
    let view = one_guard();
    let mut runtime = runtime_of(
        r#"
        event Damaged(int amount);

        behavior Guard {
            int hurt = 0;
            int spawned = 0;
            int ticks = 0;
            int farewells = 0;

            void Hurt(int amount) { hurt += amount; }
            void OnSpawn() { spawned += 1; }
            void Update(float dt) { ticks += 1; }
            void OnDespawn() { farewells += 1; }

            on Damaged(int amount) { }
        }
        "#,
    );
    let mut host = Host::new();
    run_behaviors(&view, &EventQueue::new(), &mut runtime, &mut host, u64::MAX);

    let members = mail(&[
        raised_with("Hurt", &[ScriptValue::Int(5)]),
        raised_with("OnSpawn", &[]),
        raised_with("Update", &[ScriptValue::Float(0.1)]),
        raised_with("OnDespawn", &[]),
    ]);
    let report = run_behaviors(&view, &members, &mut runtime, &mut host, u64::MAX);

    assert_eq!(report.faulted, 0, "{report:?}");
    assert_eq!(field(&runtime, "Guard", 0), Some(0), "`Hurt` did not run");
    assert_eq!(
        field(&runtime, "Guard", 1),
        Some(1),
        "`OnSpawn` ran once, at spawn"
    );
    assert_eq!(
        field(&runtime, "Guard", 2),
        Some(2),
        "`Update` ran once a frame, not for the event"
    );
    assert_eq!(
        field(&runtime, "Guard", 3),
        Some(0),
        "`OnDespawn` did not run"
    );
    assert!(
        problems_about(&runtime, entity(0)).is_empty(),
        "an event nobody handles is not a problem"
    );
}

/// **A dropped payload is a running warning** naming the entity, the
/// behavior, the event, its declared signature and what came — and a
/// payload that fits is no problem at all.
#[test]
fn a_dropped_payload_is_reported_as_a_running_warning() {
    let view = one_guard();
    let mut runtime = runtime_of(TICKING_GUARD);
    let mut host = Host::new();
    run_behaviors(&view, &EventQueue::new(), &mut runtime, &mut host, u64::MAX);

    let fits = mail(&[raised_with("Damaged", &[ScriptValue::Int(1)])]);
    run_behaviors(&view, &fits, &mut runtime, &mut host, u64::MAX);
    assert!(
        problems_about(&runtime, entity(0)).is_empty(),
        "a payload that fits is not reported"
    );

    let wrong = mail(&[raised_with(
        "Damaged",
        &[ScriptValue::Str("ten".to_owned())],
    )]);
    run_behaviors(&view, &wrong, &mut runtime, &mut host, u64::MAX);

    let reported = problems_about(&runtime, entity(0));
    assert_eq!(reported.len(), 1, "{reported:?}");
    let problem = &reported[0];
    assert_eq!(problem.severity, ProblemSeverity::Warning);
    assert_eq!(problem.behavior.as_deref(), Some("Guard"));
    assert_eq!(problem.module.as_deref(), Some(MODULE));
    assert!(
        problem.message.contains("Damaged")
            && problem.message.contains("int")
            && problem.message.contains("string"),
        "the event, its signature and what came: {}",
        problem.message
    );
}

/// **The report does not grow with the drops**: one entry per entity,
/// behavior and event, holding the last mismatch.
#[test]
fn the_report_keeps_the_last_mismatch_once_per_entity_behavior_and_event() {
    let view = one_guard();
    let mut runtime = runtime_of(TICKING_GUARD);
    let mut host = Host::new();
    run_behaviors(&view, &EventQueue::new(), &mut runtime, &mut host, u64::MAX);

    let text = || raised_with("Damaged", &[ScriptValue::Str("ten".to_owned())]);
    for _ in 0..4 {
        run_behaviors(
            &view,
            &mail(&[text(), text()]),
            &mut runtime,
            &mut host,
            u64::MAX,
        );
    }
    let last = mail(&[raised_with("Damaged", &[ScriptValue::Bool(true)])]);
    run_behaviors(&view, &last, &mut runtime, &mut host, u64::MAX);

    let reported = problems_about(&runtime, entity(0));
    assert_eq!(
        reported.len(),
        1,
        "one entry, not one per drop: {reported:?}"
    );
    assert!(
        reported[0].message.contains("bool") && !reported[0].message.contains("string"),
        "the last mismatch is the one held: {}",
        reported[0].message
    );
}

/// Two events dropped are two entries — one per event.
#[test]
fn two_events_dropped_are_reported_apart() {
    let view = one_guard();
    let mut runtime = runtime_of(TICKING_GUARD);
    let mut host = Host::new();
    run_behaviors(&view, &EventQueue::new(), &mut runtime, &mut host, u64::MAX);

    let both = mail(&[
        raised_with("Damaged", &[ScriptValue::Str("ten".to_owned())]),
        raised_with("Healed", &[ScriptValue::Str("ten".to_owned())]),
    ]);
    run_behaviors(&view, &both, &mut runtime, &mut host, u64::MAX);

    let reported = problems_about(&runtime, entity(0));
    assert_eq!(reported.len(), 2, "{reported:?}");
    assert!(
        reported.iter().any(|p| p.message.contains("Damaged"))
            && reported.iter().any(|p| p.message.contains("Healed")),
        "{reported:?}"
    );
}

/// **A gone entity's mismatch leaves the report with it** — or the report
/// would grow with every entity that ever received a bad payload.
#[test]
fn a_mismatch_report_leaves_with_its_entity() {
    let view = one_guard();
    let mut runtime = runtime_of(TICKING_GUARD);
    let mut host = Host::new();
    run_behaviors(&view, &EventQueue::new(), &mut runtime, &mut host, u64::MAX);
    let wrong = mail(&[raised_with(
        "Damaged",
        &[ScriptValue::Str("ten".to_owned())],
    )]);
    run_behaviors(&view, &wrong, &mut runtime, &mut host, u64::MAX);
    let premise = problems_about(&runtime, entity(0));
    assert!(
        premise.len() == 1 && premise[0].severity == ProblemSeverity::Warning,
        "the premise: the drop is reported as a warning: {premise:?}"
    );

    let empty = ScriptView {
        programs: view.programs.clone(),
        delta_seconds: 0.016,
        ..Default::default()
    };
    run_behaviors(
        &empty,
        &EventQueue::new(),
        &mut runtime,
        &mut host,
        u64::MAX,
    );

    assert!(
        runtime.running_problems().is_empty(),
        "{:?}",
        runtime.running_problems()
    );
}

/// **Mail queued before a reload changed its event is dropped and reported,
/// never a fault** — the case the payload check exists for: a script raised
/// `Damaged(25)` under `event Damaged(int amount)`, and the edit that landed
/// before it was delivered made it `event Damaged(string why)`.
#[test]
fn mail_queued_before_a_reload_changed_its_event_is_dropped_and_reported() {
    let view = one_guard();
    let mut runtime = runtime_of(SELF_HITTER);
    let mut host = Host::new();
    run_behaviors(&view, &EventQueue::new(), &mut runtime, &mut host, u64::MAX);
    let queued = host.take_events();
    assert_eq!(queued.len(), 1, "the premise: the hit is queued");

    runtime.reload(
        MODULE,
        compile(
            r#"
            event Damaged(string why);

            behavior Guard {
                int health = 100;
                int hits = 0;

                on Damaged(string why) { hits += 1; }
            }
            "#,
        ),
    );
    let report = run_behaviors(&view, &queued, &mut runtime, &mut host, u64::MAX);

    assert_eq!(report.faulted, 0, "dropped, not faulted: {report:?}");
    assert!(!disabled(&runtime, entity(0), "Guard"));
    assert_eq!(
        field(&runtime, "Guard", 1),
        Some(0),
        "the handler did not run"
    );
    let reported = problems_about(&runtime, entity(0));
    assert!(
        reported.len() == 1
            && reported[0].severity == ProblemSeverity::Warning
            && reported[0].message.contains("Damaged")
            && reported[0].message.contains("string")
            && reported[0].message.contains("int"),
        "the drop is reported, the new signature against what came: {reported:?}"
    );
}
