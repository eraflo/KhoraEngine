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

//! An event is a declaration: `event Damaged(int amount, Entity source);`.
//!
//! Its name is its identity and its parameters are its signature — what a
//! raise is checked against and what a handler must take. It is visible like a
//! struct or a component: in its module and in every module importing it. The
//! engine declares `Touched(Entity other)` and `Separated(Entity other)`,
//! served as `engine/events.erg` and in scope everywhere without an import.
//!
//! What a declaration may not be: a payload the bridge cannot carry, a name
//! the engine calls on its own (`Update`, `OnSpawn`, …), a name starting with
//! `__`, a name another declaration already has, or two parameters with one
//! name.

use khora_core::script::ErgonType;
use khora_script::vm::{EventSignature, Program};
use khora_script::{compile_module, CompileOutcome, Diagnostic, MemoryLoader};

fn compile_with(loader: &MemoryLoader) -> CompileOutcome {
    compile_module(loader, "main.erg")
}

fn compile(source: &str) -> CompileOutcome {
    compile_with(&MemoryLoader::new().with("main.erg", source))
}

fn errors(result: &CompileOutcome) -> Vec<&Diagnostic> {
    result.diagnostics.iter().filter(|d| d.is_error()).collect()
}

/// What a diagnostic tells the author: its line and its note.
fn said(diagnostic: &Diagnostic) -> String {
    format!(
        "{} | {}",
        diagnostic.message,
        diagnostic.note.as_deref().unwrap_or_default()
    )
}

fn build_with(loader: &MemoryLoader) -> Program {
    let result = compile_with(loader);
    let found: Vec<String> = errors(&result).into_iter().map(said).collect();
    assert!(found.is_empty(), "should compile: {found:?}");
    result.program.expect("a program with no errors")
}

fn build(source: &str) -> Program {
    build_with(&MemoryLoader::new().with("main.erg", source))
}

/// Asserts `result` was refused with an error saying every one of
/// `fragments`, compared without case.
fn assert_refused_saying(result: &CompileOutcome, source: &str, fragments: &[&str]) {
    assert!(
        !result.succeeded(),
        "should be refused, and compiled:\n{source}"
    );
    let found: Vec<String> = errors(result).into_iter().map(said).collect();
    assert!(
        found.iter().any(|text| {
            let text = text.to_lowercase();
            fragments
                .iter()
                .all(|fragment| text.contains(&fragment.to_lowercase()))
        }),
        "expected a refusal of\n{source}\nsaying {fragments:?}; got {found:?}"
    );
}

fn refused_saying(source: &str, fragments: &[&str]) {
    assert_refused_saying(&compile(source), source, fragments);
}

/// The signature of the event named `name` the program lists.
fn signature<'p>(program: &'p Program, name: &str) -> &'p EventSignature {
    program
        .events
        .iter()
        .find(|event| event.name == name)
        .unwrap_or_else(|| panic!("`{name}` is listed among {:?}", program.events))
}

fn engine(name: &'static str) -> ErgonType {
    ErgonType::Engine(name)
}

// ─── Declared ───────────────────────────────────────────────────────────────

/// **An event names its payload.** The declaration compiles, and the program
/// lists it with its parameter types in order.
#[test]
fn an_event_with_a_payload_is_declared_with_its_signature() {
    let program = build("event Damaged(int amount, Entity source);");

    assert_eq!(
        signature(&program, "Damaged"),
        &EventSignature {
            name: "Damaged".to_owned(),
            params: vec![ErgonType::Int, ErgonType::Entity],
        }
    );
}

/// **An event may carry nothing**, written with or without the parentheses.
#[test]
fn an_event_without_a_payload_is_declared_with_or_without_parentheses() {
    let program = build(
        "event Opened;
         event Closed();",
    );

    assert!(signature(&program, "Opened").params.is_empty());
    assert!(signature(&program, "Closed").params.is_empty());
}

/// Every type the value bridge carries is a payload type: the scalars, text,
/// the engine's value types, an entity, and the two units.
#[test]
fn every_type_the_bridge_carries_is_a_payload_type() {
    let program = build(
        "event Everything(bool a, int b, float c, string d, Vec2 e, Vec3 f, Vec4 g,
                          Quat h, Color i, Entity j, Duration k, Angle l);",
    );

    assert_eq!(
        signature(&program, "Everything").params,
        vec![
            ErgonType::Bool,
            ErgonType::Int,
            ErgonType::Float,
            ErgonType::Str,
            engine("Vec2"),
            engine("Vec3"),
            engine("Vec4"),
            engine("Quat"),
            engine("Color"),
            ErgonType::Entity,
            ErgonType::Duration,
            ErgonType::Angle,
        ]
    );
}

/// **What the bridge cannot carry is refused at the declaration, saying
/// why** — an optional, an array, a struct, a map, a component — instead of
/// compiling into a raise the delivery would refuse.
#[test]
fn a_payload_type_the_bridge_cannot_carry_is_refused_with_its_reason() {
    let cases = [
        ("", "int?", "optional"),
        ("", "int[]", "array"),
        ("struct Loot { int value; }", "Loot", "struct"),
        ("", "Map<string, int>", "map"),
    ];
    for (prelude, ty, reason) in cases {
        // The premise: the type is one a function may take.
        build(&format!("{prelude}\nfn void Keep({ty} x) {{ }}"));
        refused_saying(&format!("{prelude}\nevent Hit({ty} x);"), &[reason]);
    }

    // A component is a type nowhere, so its premise is only that it is
    // declared.
    let shield = "component Shield { int strength; }";
    build(shield);
    refused_saying(&format!("{shield}\nevent Hit(Shield x);"), &["component"]);
}

/// **A parameter is named once**, in an event as in a function.
#[test]
fn two_parameters_with_one_name_are_refused() {
    build("event Hit(int a, float b);");
    refused_saying("event Hit(int a, float a);", &["`a`"]);
}

// ─── Names ──────────────────────────────────────────────────────────────────

/// **A hook is called by the engine, never raised.** An event named like a
/// lifecycle member would let a raise run that member, so every name the
/// lifecycle table lists is refused as an event name.
#[test]
fn an_event_named_like_a_lifecycle_hook_is_refused() {
    build("event Ping;");
    assert!(
        !khora_script::lifecycle::ALL.is_empty(),
        "the premise: the engine calls some members on its own"
    );
    for hook in khora_script::lifecycle::ALL {
        let quoted = format!("`{}`", hook.name);
        refused_saying(&format!("event {};", hook.name), &[&quoted]);
        refused_saying(&format!("event {}(float dt);", hook.name), &[&quoted]);
    }
}

/// **`__` names belong to the compiler** — a state's entry, a timer's body —
/// so no event may start with them.
#[test]
fn an_event_name_starting_with_two_underscores_is_refused() {
    build("event Enter;");
    refused_saying("event __enter;", &["`__enter`"]);
    refused_saying("event __Hit(int amount);", &["`__Hit`"]);
}

/// **An event's name is global.** It clashes with any other declaration of
/// the program — a struct, a component, a behavior, a system, a function,
/// another event — and with what the engine already names.
#[test]
fn an_event_name_another_declaration_has_is_refused() {
    let others = [
        "struct Ping { int value; }",
        "component Ping { int value; }",
        "behavior Ping { int value = 0; }",
        "system Ping { int value = 0; }",
        "fn void Ping() { }",
        "event Ping(int value);",
    ];
    for other in others {
        // The premise: each declaration compiles on its own.
        build(other);
        refused_saying(&format!("{other}\nevent Ping;"), &["`Ping`"]);
    }

    // What the engine names: one of its functions, one of its value types.
    refused_saying("event Abs(float x);", &["`Abs`"]);
    refused_saying("event Vec3;", &["`Vec3`"]);
}

/// The clash holds across modules: an event imported from one module and a
/// struct of the same name in the importer are one name too many.
#[test]
fn an_event_name_clashes_across_modules() {
    let loader = MemoryLoader::new()
        .with("events.erg", "event Ping(int value);")
        .with(
            "main.erg",
            r#"import "events.erg";
               struct Ping { int value; }"#,
        );
    assert_refused_saying(&compile_with(&loader), "main.erg", &["`Ping`"]);
}

/// **`Touched` and `Separated` are the engine's.** A script declaring either
/// is refused, whatever payload it gives them.
#[test]
fn a_script_cannot_declare_an_engine_event() {
    for name in ["Touched", "Separated"] {
        let quoted = format!("`{name}`");
        refused_saying(&format!("event {name}(Entity other);"), &[&quoted]);
        refused_saying(&format!("event {name};"), &[&quoted]);
    }
}

// ─── Visibility ─────────────────────────────────────────────────────────────

/// **An event is visible like a struct**: declared in one module, it is
/// raised and handled in a module importing it, and listed by the program.
#[test]
fn an_event_declared_in_an_imported_module_is_in_scope() {
    let loader = MemoryLoader::new()
        .with("combat.erg", "event Damaged(int amount);")
        .with(
            "main.erg",
            r#"import "combat.erg";
               behavior Guard {
                   int health = 100;
                   void Hurt() { this.Raise(Damaged(3)); }
                   on Damaged(int amount) { health -= amount; }
               }"#,
        );
    let program = build_with(&loader);

    assert_eq!(signature(&program, "Damaged").params, vec![ErgonType::Int]);
}

/// A module the entry point does not import declares nothing the entry point
/// can name: handling or raising its event is refused as an unknown event.
#[test]
fn an_event_declared_in_a_module_not_imported_is_not_in_scope() {
    let handled = MemoryLoader::new()
        .with("combat.erg", "event Damaged(int amount);")
        .with("main.erg", "behavior Guard { on Damaged(int amount) { } }");
    assert_refused_saying(
        &compile_with(&handled),
        "main.erg (handling)",
        &["no event named `Damaged`"],
    );

    let raised = MemoryLoader::new()
        .with("combat.erg", "event Damaged(int amount);")
        .with(
            "main.erg",
            "behavior Guard { void Hurt() { this.Raise(Damaged(3)); } }",
        );
    assert_refused_saying(
        &compile_with(&raised),
        "main.erg (raising)",
        &["no event named `Damaged`"],
    );
}

// ─── What the program knows ─────────────────────────────────────────────────

/// **`Program.events` is every event the program can raise or handle**: its
/// own, the ones it imports and the engine's — sorted by name, each once.
#[test]
fn the_program_lists_every_event_it_knows_sorted_by_name() {
    let loader = MemoryLoader::new()
        .with("signals.erg", "event Alarm(Vec3 at, Duration lasting);")
        .with(
            "main.erg",
            r#"import "signals.erg";
               event Zeal(int level, Entity source);
               event Ping;"#,
        );
    let program = build_with(&loader);

    let event = |name: &str, params: Vec<ErgonType>| EventSignature {
        name: name.to_owned(),
        params,
    };
    assert_eq!(
        program.events,
        vec![
            event("Alarm", vec![engine("Vec3"), ErgonType::Duration]),
            event("Ping", Vec::new()),
            event("Separated", vec![ErgonType::Entity]),
            event("Touched", vec![ErgonType::Entity]),
            event("Zeal", vec![ErgonType::Int, ErgonType::Entity]),
        ]
    );
}

// ─── The engine's events ────────────────────────────────────────────────────

/// **A script handling a contact needs no import.** `Touched` and `Separated`
/// are declared by the engine, each carrying the other entity, and are in
/// scope in a module that imports nothing — from a loader that serves nothing
/// but that module.
#[test]
fn the_engine_events_are_in_scope_without_an_import() {
    let program = build(
        "behavior Guard {
             on Touched(Entity other) { other.Despawn(); }
             on Separated(Entity other) { }
         }",
    );

    assert_eq!(
        signature(&program, "Touched").params,
        vec![ErgonType::Entity]
    );
    assert_eq!(
        signature(&program, "Separated").params,
        vec![ErgonType::Entity]
    );
}

/// The engine's events are served as `engine/events.erg`, so naming the
/// module explicitly resolves too — whatever loader the project uses — and
/// does not declare them a second time.
#[test]
fn the_engine_events_are_served_as_a_module() {
    let program = build(
        r#"import "engine/events.erg";
           behavior Guard { on Touched(Entity other) { } }"#,
    );

    let touched = program
        .events
        .iter()
        .filter(|event| event.name == "Touched")
        .count();
    assert_eq!(touched, 1, "listed once: {:?}", program.events);
}

/// The engine events are checked like any other: a handler of `Touched`
/// taking something other than the entity is refused, naming the signature.
#[test]
fn a_handler_of_an_engine_event_is_held_to_its_signature() {
    build("behavior Guard { on Touched { } }");
    refused_saying(
        "behavior Guard { on Touched(int other) { } }",
        &["Touched(Entity"],
    );
}

/// **`Spawn` is the language's.** An event named `Spawn` would make
/// `Spawn(…)` mean two things: outside a raise it is the entity spawn, never
/// the construction a raise's argument is — so a construction of the event
/// outside a raise would silently create an entity instead of being refused.
#[test]
fn an_event_cannot_take_the_name_of_spawn() {
    refused_saying(
        "event Spawn(Vec3 at);
         behavior Guard { void Go() { Spawn(Vec3(0.0, 0.0, 0.0)); } }",
        &["Spawn"],
    );
}
