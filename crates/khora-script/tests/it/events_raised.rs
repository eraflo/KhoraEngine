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

//! `target.Raise(Damaged(3, this))` — a raise names a declared event.
//!
//! The argument is an event construction, checked against the declaration:
//! its count, its types, an `int` widened where the event says `float`. The
//! receiver is any `Entity` — in a behavior, a free function or a system. A
//! construction anywhere but a raise is refused: an event is raised, not
//! stored. The string form `Raise("Damaged", 3)` is refused with the fix in
//! its message, and the free form `Raise(this, …)` stays refused.

use khora_core::ecs::entity::EntityId;
use khora_core::script::{ScriptEvent, ScriptValue};
use khora_script::vm::{Machine, Program, Run, Value};
use khora_script::{
    compile_module, CompileOutcome, Diagnostic, Host, MemoryLoader, NativeRegistry,
    ENGINE_COMPONENTS_MODULE,
};

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

/// The events most rows raise.
const EVENTS: &str = "event Damaged(int amount, Entity source);
event Hit(float amount);
event Ping;
";

fn compile(source: &str) -> CompileOutcome {
    compile_module(
        &MemoryLoader::new().with("main.erg", format!("{EVENTS}{source}")),
        "main.erg",
    )
}

fn said(diagnostic: &Diagnostic) -> String {
    format!(
        "{} | {}",
        diagnostic.message,
        diagnostic.note.as_deref().unwrap_or_default()
    )
}

fn errors(result: &CompileOutcome) -> Vec<String> {
    result
        .diagnostics
        .iter()
        .filter(|d| d.is_error())
        .map(said)
        .collect()
}

fn build(source: &str) -> Program {
    let result = compile(source);
    let found = errors(&result);
    assert!(found.is_empty(), "should compile:\n{source}\n{found:?}");
    result.program.expect("a program with no errors")
}

/// Asserts `source` is refused; `fragments`, when given, must all be said by
/// one of its errors.
fn refused_saying(source: &str, fragments: &[&str]) {
    let result = compile(source);
    assert!(
        !result.succeeded(),
        "should be refused, and compiled:\n{source}"
    );
    let found = errors(&result);
    assert!(
        found
            .iter()
            .any(|text| fragments.iter().all(|fragment| text.contains(fragment))),
        "expected a refusal of\n{source}\nsaying {fragments:?}; got {found:?}"
    );
}

/// Runs `function` as the subject's behavior; what it raised afterwards.
fn raised_by(program: &Program, function: &str, args: &[Value]) -> Vec<ScriptEvent> {
    let mut host = Host::new().for_entity(SUBJECT);
    let mut machine = Machine::new(program, function, args).expect("the function exists");
    assert_eq!(machine.run(program, &mut host, u64::MAX), Run::Completed);
    host.take_events().as_slice().to_vec()
}

// ─── The raise ──────────────────────────────────────────────────────────────

/// **The event and its payload, as declared.** `this.Raise(Damaged(3, this))`
/// queues one event on the subject, named by its declaration, carrying its
/// arguments in order.
#[test]
fn a_typed_raise_queues_the_event_with_its_payload() {
    let program = build(
        "behavior Guard {
             void Hurt() { this.Raise(Damaged(3, this)); }
         }",
    );

    let raised = raised_by(&program, "Guard.Hurt", &[]);
    assert_eq!(raised.len(), 1, "one event raised: {raised:?}");
    assert_eq!(raised[0].target, SUBJECT);
    assert_eq!(raised[0].name, "Damaged");
    assert_eq!(
        raised[0].args,
        vec![ScriptValue::Int(3), ScriptValue::Entity(SUBJECT)]
    );
}

/// An event with no payload is raised with an empty construction.
#[test]
fn an_event_without_a_payload_is_raised_empty() {
    let program = build("behavior Bell { void Ring() { this.Raise(Ping()); } }");

    let raised = raised_by(&program, "Bell.Ring", &[]);
    assert_eq!(raised.len(), 1, "one event raised: {raised:?}");
    assert_eq!(raised[0].name, "Ping");
    assert!(raised[0].args.is_empty());
}

/// **The receiver is any entity**, in a free function as in a behavior: the
/// event lands on the receiver, not on whoever runs the code.
#[test]
fn a_free_function_raises_on_its_receiver() {
    let program = build("fn void Strike(Entity target) { target.Raise(Damaged(4, target)); }");

    let raised = raised_by(&program, "Strike", &[Value::Entity(OTHER)]);
    assert_eq!(raised.len(), 1, "one event raised: {raised:?}");
    assert_eq!(raised[0].target, OTHER, "raised on the receiver");
    assert_eq!(
        raised[0].args,
        vec![ScriptValue::Int(4), ScriptValue::Entity(OTHER)]
    );
}

/// A system has no entity of its own, but it raises on the entities it
/// queries — checked like any other raise.
#[test]
fn a_system_raises_on_an_entity_it_queried() {
    let compile_system = |body: &str| {
        let main = format!(
            "import \"{ENGINE_COMPONENTS_MODULE}\";\n{EVENTS}
             system Pulse {{
                 void Update(float dt) {{
                     foreach (Entity e, Health h) {{ {body} }}
                 }}
             }}"
        );
        compile_module(
            &MemoryLoader::new()
                .with(
                    ENGINE_COMPONENTS_MODULE,
                    "component Health { int current; }",
                )
                .with("main.erg", main),
            "main.erg",
        )
    };

    let accepted = compile_system("e.Raise(Damaged(h.current, e));");
    assert!(accepted.succeeded(), "{:?}", errors(&accepted));

    let refused = compile_system("e.Raise(Damaged(true, e));");
    assert!(!refused.succeeded(), "a `bool` is not the `int` it carries");
}

// ─── The arguments, against the declaration ─────────────────────────────────

/// **An `int` widens where the event says `float`**, at compile time: the
/// payload carries the float.
#[test]
fn an_int_argument_widens_to_a_float_parameter() {
    let program = build("behavior Guard { void Hurt() { this.Raise(Hit(3)); } }");

    let raised = raised_by(&program, "Guard.Hurt", &[]);
    assert_eq!(raised.len(), 1, "one event raised: {raised:?}");
    assert_eq!(raised[0].args, vec![ScriptValue::Float(3.0)]);
}

/// **A `Duration` or an `Angle` parameter takes only its unit.** The one
/// implicit conversion is `int` → `float`: a bare number, whole or not, is
/// not a duration nor an angle.
#[test]
fn a_duration_or_an_angle_parameter_takes_only_its_unit() {
    let units = "event Wait(Duration lasting);
                 event Turn(Angle by);";
    let raising = |raise: &str| {
        format!(
            "{units}
             behavior Guard {{ void Go() {{ this.Raise({raise}); }} }}"
        )
    };

    for unit in ["Wait(2s)", "Wait(500ms)", "Turn(90deg)"] {
        build(&raising(unit));
    }
    for bare in ["Wait(2)", "Turn(1)", "Wait(2.0)"] {
        refused_saying(&raising(bare), &[]);
    }
}

/// A `float` does not narrow to an `int`.
#[test]
fn a_float_argument_does_not_narrow_to_an_int_parameter() {
    build("behavior Guard { void Hurt() { this.Raise(Damaged(1, this)); } }");
    refused_saying(
        "behavior Guard { void Hurt() { this.Raise(Damaged(1.5, this)); } }",
        &[],
    );
}

/// **Each argument is the type its parameter declares.**
#[test]
fn an_argument_of_another_type_is_refused() {
    build("behavior Guard { void Hurt() { this.Raise(Damaged(1, this)); } }");
    for arguments in [
        "true, this",
        "\"three\", this",
        "3, 4",
        "3, Vec3(0.0, 0.0, 0.0)",
    ] {
        refused_saying(
            &format!("behavior Guard {{ void Hurt() {{ this.Raise(Damaged({arguments})); }} }}"),
            &[],
        );
    }
}

/// **As many arguments as the event declares**, naming the event when not.
#[test]
fn a_raise_with_the_wrong_number_of_arguments_is_refused() {
    build("behavior Guard { void Hurt() { this.Raise(Damaged(1, this)); } }");
    for arguments in ["", "1", "1, this, 2"] {
        refused_saying(
            &format!("behavior Guard {{ void Hurt() {{ this.Raise(Damaged({arguments})); }} }}"),
            &["`Damaged`"],
        );
    }
    refused_saying(
        "behavior Bell { void Ring() { this.Raise(Ping(1)); } }",
        &["`Ping`"],
    );
}

/// **A raise names an event in scope**; a misspelled one is a compile error
/// rather than an event nobody hears.
#[test]
fn a_raise_of_an_unknown_event_is_refused() {
    refused_saying(
        "behavior Guard { void Hurt() { this.Raise(Damagd(1, this)); } }",
        &["no event named `Damagd`"],
    );
}

/// What a raise takes is an event construction — not a value that happens to
/// be in hand, nor what another call returned.
#[test]
fn a_raise_of_something_other_than_an_event_construction_is_refused() {
    build("behavior Guard { void Hurt() { this.Raise(Ping()); } }");
    refused_saying(
        "behavior Guard { void Hurt() { int x = 3; this.Raise(x); } }",
        &[],
    );
    refused_saying(
        "fn int Three() { return 3; }
         behavior Guard { void Hurt() { this.Raise(Three()); } }",
        &[],
    );
    refused_saying("behavior Guard { void Hurt() { this.Raise(); } }", &[]);
}

// ─── An event is raised, not stored ─────────────────────────────────────────

/// **A construction exists only as a raise's argument** — in a local, as a
/// statement of its own, or handed to another call, it is refused with the
/// form to write.
#[test]
fn an_event_constructed_outside_a_raise_is_refused() {
    let elsewhere = [
        (
            "behavior Guard { void Hurt() { var d = Damaged(3, this); } }",
            "Raise(Damaged(",
        ),
        (
            "behavior Guard { void Hurt() { Damaged(3, this); } }",
            "Raise(Damaged(",
        ),
        (
            "fn void Keep(int x) { }
             behavior Guard { void Hurt() { Keep(Damaged(3, this)); } }",
            "Raise(Damaged(",
        ),
        (
            "behavior Bell { void Ring() { var p = Ping(); } }",
            "Raise(Ping(",
        ),
    ];
    for (source, fix) in elsewhere {
        refused_saying(source, &["raised, not stored", fix]);
    }
}

// ─── The old forms ──────────────────────────────────────────────────────────

/// **The string form is refused, and the refusal is the fix**: the
/// declaration to write and the raise that names it.
#[test]
fn the_string_form_is_refused_with_the_declaration_to_write() {
    build("behavior Guard { void Hurt() { this.Raise(Damaged(3, this)); } }");
    refused_saying(
        r#"behavior Guard { void Hurt() { this.Raise("Struck", 3); } }"#,
        &["event Struck", "Raise(Struck(3))"],
    );
    // Where the event is declared, the string form is refused all the same.
    refused_saying(
        r#"behavior Bell { void Ring() { this.Raise("Ping"); } }"#,
        &["Raise(Ping())"],
    );
    // A computed name cannot be checked at all.
    refused_saying(
        r#"behavior Guard { void Hurt() { string name = "Struck"; this.Raise(name, 3); } }"#,
        &[],
    );
}

/// **The free form stays refused**, with its method rewrite.
#[test]
fn the_free_form_of_raise_is_refused_with_its_rewrite() {
    build("behavior Guard { void Hurt() { this.Raise(Damaged(3, this)); } }");
    refused_saying(
        "behavior Guard { void Hurt() { Raise(this, Damaged(3, this)); } }",
        &["Write `this.Raise(Damaged(3, this))`"],
    );
}

/// **`Raise` is the language's, not an engine function.** The variadic native
/// is gone: what a raise carries is decided by the declaration, not by the
/// registry.
#[test]
fn raise_is_no_longer_an_engine_function() {
    let registry = NativeRegistry::discovered();
    assert!(
        registry.get("Raise").is_none(),
        "`Raise` is still a registered engine function"
    );
    assert!(
        registry.iter().all(|native| !native.variadic),
        "a variadic engine function is left: {:?}",
        registry
            .iter()
            .filter(|native| native.variadic)
            .map(|native| native.name)
            .collect::<Vec<_>>()
    );
}

// ─── What a raise's fingerprint names ───────────────────────────────────────

fn fingerprint_of(program: &Program, name: &str) -> u64 {
    program
        .function(name)
        .unwrap_or_else(|| panic!("`{name}` is a function of the program"))
        .fingerprint
}

/// **A raise is hashed by the event it names.** Two bodies identical but for
/// the event raised are different code.
#[test]
fn a_raise_fingerprint_names_the_event() {
    let hit = build(
        "event Bump(float amount);
         behavior Guard { void Go() { this.Raise(Hit(1.0)); } }",
    );
    let bump = build(
        "event Bump(float amount);
         behavior Guard { void Go() { this.Raise(Bump(1.0)); } }",
    );

    assert_ne!(
        fingerprint_of(&hit, "Guard.Go"),
        fingerprint_of(&bump, "Guard.Go"),
        "the event raised is part of the code"
    );
}

/// **Where an event is declared is not code.** Declared in another order,
/// with others around it, a raise of it keeps its fingerprint.
#[test]
fn a_raise_fingerprint_ignores_where_the_event_is_declared() {
    let body = "behavior Guard { void Go() { this.Raise(Hit(1.0)); } }";
    let original = build(body);
    let moved = compile_module(
        &MemoryLoader::new().with(
            "main.erg",
            format!(
                "event Alpha(int a);
                 event Ping;
                 event Hit(float amount);
                 event Damaged(int amount, Entity source);
                 event Omega;
                 {body}"
            ),
        ),
        "main.erg",
    );
    assert!(moved.succeeded(), "{:?}", errors(&moved));
    let moved = moved.program.expect("compiled");

    assert_eq!(
        fingerprint_of(&original, "Guard.Go"),
        fingerprint_of(&moved, "Guard.Go")
    );
}

// ─── A raise across a resume ────────────────────────────────────────────────

/// `Guard`, whose `Attack` raises `Struck` with a payload that waits on an
/// `async` call — so the body stops in the middle of the raise, the entity
/// and the first value already in their registers — then runs `after`.
fn striking_guard(declaration: &str, after: &str) -> Program {
    build(&format!(
        "{declaration}
         behavior Guard {{
             int later = 0;
             async int Wind() {{ await 1.0s; return 41; }}
             async void Attack() {{
                 this.Raise(Struck(7, Wind(), this));
                 {after}
             }}
         }}"
    ))
}

const STRUCK_INT: &str = "event Struck(int first, int second, Entity source);";
const STRUCK_FLOAT: &str = "event Struck(float first, int second, Entity source);";

/// Stops `Attack` of `before` at its `await`, freezes it, resumes it into
/// `after`, and runs it to the end: the tier, and what it raised.
fn struck_across_an_edit(
    before: &Program,
    after: &Program,
) -> (khora_script::vm::ResumeTier, Vec<ScriptEvent>) {
    use khora_core::script::PendingBody;
    use khora_script::vm::{resume, Suspension};

    let layout = before.layout("Guard").expect("a layout");
    let mut host = Host::new()
        .with_fields(layout.fresh_store())
        .for_entity(SUBJECT);
    let mut machine = Machine::new(before, "Guard.Attack", &[]).expect("the member exists");
    assert_eq!(
        machine.run(before, &mut host, u64::MAX),
        Run::Suspended(Suspension::Awaiting),
        "the body stopped inside its raise"
    );
    assert!(host.take_events().as_slice().is_empty(), "not raised yet");
    let frozen = machine
        .freeze(before, PendingBody::Sequence)
        .expect("freezes");

    let (mut machine, tier) = resume(&frozen, before.fingerprint(), after)
        .unwrap_or_else(|abandoned| panic!("abandoned: {abandoned:?}"));
    let mut host = Host::new()
        .with_fields(host.fields.clone())
        .for_entity(SUBJECT);
    for _ in 0..100 {
        match machine.run(after, &mut host, u64::MAX) {
            Run::Completed => return (tier, host.take_events().as_slice().to_vec()),
            Run::Suspended(_) => host.awaiting = None,
            Run::Faulted(fault) => panic!("faulted: {fault:?}"),
        }
    }
    panic!("still running after 100 resumes")
}

/// **A raise stopped inside its payload finishes as the raise it was.** The
/// body waits in the call that computes the second value, after the receiver
/// and the first value are in their registers; resumed — into the same
/// program, or rebuilt after an edit past the raise — it raises the event
/// once, with the whole payload.
#[test]
fn a_raise_stopped_inside_its_payload_resumes_with_the_whole_payload() {
    let payload = vec![
        ScriptValue::Int(7),
        ScriptValue::Int(41),
        ScriptValue::Entity(SUBJECT),
    ];
    let before = striking_guard(STRUCK_INT, "later += 1;");

    let (tier, raised) = struck_across_an_edit(&before, &before);
    assert_eq!(tier, khora_script::vm::ResumeTier::Exact);
    assert_eq!(raised.len(), 1, "raised once: {raised:?}");
    assert_eq!(raised[0].args, payload);

    let edited = striking_guard(STRUCK_INT, "later += 2;");
    let (tier, raised) = struck_across_an_edit(&before, &edited);
    assert_eq!(tier, khora_script::vm::ResumeTier::Rebuilt);
    assert_eq!(raised.len(), 1, "raised once: {raised:?}");
    assert_eq!(raised[0].args, payload);
}

/// **A raise carries what the declaration it runs under says.** The event's
/// first parameter became a `float` while the body waited inside the raise;
/// the frame is rebuilt into the new code, so the `7` it raises is the float
/// the declaration now names — not the `int` the old code had computed.
#[test]
fn a_raise_rebuilt_after_its_parameter_became_a_float_carries_a_float() {
    let before = striking_guard(STRUCK_INT, "later += 1;");
    let edited = striking_guard(STRUCK_FLOAT, "later += 1;");

    let (tier, raised) = struck_across_an_edit(&before, &edited);

    assert_eq!(raised.len(), 1, "raised once ({tier:?}): {raised:?}");
    assert_eq!(
        raised[0].args,
        vec![
            ScriptValue::Float(7.0),
            ScriptValue::Int(41),
            ScriptValue::Entity(SUBJECT),
        ],
        "resumed as {tier:?}: `Struck` is declared `Struck(float, int, Entity)`"
    );
}
