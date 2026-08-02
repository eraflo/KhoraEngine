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

//! Native call tests.
//!
//! The decisive one: a source program calls an engine function by name, and the
//! value comes back. Everything between — the checker resolving the name, the
//! compiler emitting an index, the VM finding it in the registry — has to agree
//! for that to happen at all.

use super::{builtins, Host, NativeContext, NativeError, NativeFn, NativeRegistry, NativeTy};
use crate::bytecode::compile_with;
use crate::lexer::lex;
use crate::parser::parse;
use crate::types::{check_with, Ty};
use crate::vm::{Fault, Machine, Run, Value};

/// Compiles a program against `natives`, insisting it is well-formed first.
fn build(source: &str, natives: &NativeRegistry) -> crate::vm::Program {
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

/// The errors a program produces, whichever stage found them.
fn errors(source: &str, natives: &NativeRegistry) -> Vec<String> {
    let lexed = lex(source);
    let parsed = parse(lexed.tokens.clone());
    check_with(&parsed.module, natives)
        .diagnostics
        .into_iter()
        .map(|d| d.message)
        .collect()
}

/// Runs `Main` to completion and hands back the host, so a test can look at
/// what the program left behind.
fn run(program: &crate::vm::Program, host: &mut Host) -> Run {
    let mut machine = Machine::new(program, "Main", &[]).expect("Main exists");
    machine.run(program, host, u64::MAX)
}

fn float_result(source: &str) -> f32 {
    let natives = NativeRegistry::with_builtins();
    let program = build(&format!("fn float Main() {{ return {source}; }}"), &natives);

    let mut host = Host::new();
    let mut machine = Machine::new(&program, "Main", &[]).expect("Main exists");
    assert_eq!(machine.run(&program, &mut host, u64::MAX), Run::Completed);
    machine
        .result()
        .as_float()
        .unwrap_or_else(|| panic!("expected a float from `{source}`"))
}

// ─── The whole road ─────────────────────────────────────────────────────────

/// **The milestone.** Source in, engine function called, value out.
#[test]
fn a_program_calls_an_engine_function_and_gets_its_result() {
    assert_eq!(float_result("Abs(-3.0)"), 3.0);
}

#[test]
fn a_native_taking_several_arguments_gets_them_in_order() {
    assert_eq!(float_result("Clamp(9.0, 0.0, 5.0)"), 5.0);
    assert_eq!(float_result("Clamp(-9.0, 0.0, 5.0)"), 0.0);
    assert_eq!(float_result("Lerp(0.0, 10.0, 0.25)"), 2.5);
}

/// Arguments are expressions, and compiling one takes temporaries of its own —
/// the call's argument slots must still end up adjacent.
#[test]
fn a_native_call_survives_nested_calls_in_its_arguments() {
    assert_eq!(float_result("Max(Abs(-2.0), Min(7.0, 4.0))"), 4.0);
}

#[test]
fn a_native_result_feeds_back_into_arithmetic() {
    assert_eq!(float_result("Sqrt(16.0) * 2.0 + 1.0"), 9.0);
}

/// The checker widens an integer literal in a float context, and the widening
/// has to survive into the call.
#[test]
fn an_integer_literal_reaches_a_float_native() {
    assert_eq!(float_result("Abs(-3)"), 3.0);
}

// ─── What the checker must refuse ───────────────────────────────────────────

#[test]
fn calling_a_native_with_the_wrong_arity_is_refused() {
    let natives = NativeRegistry::with_builtins();
    let found = errors("fn float Main() { return Abs(1.0, 2.0); }", &natives);
    assert!(
        found.iter().any(|m| m.contains("Abs") && m.contains("1")),
        "got {found:?}"
    );
}

#[test]
fn calling_a_native_with_the_wrong_type_is_refused() {
    let natives = NativeRegistry::with_builtins();
    let found = errors("fn float Main() { return Abs(true); }", &natives);
    assert!(!found.is_empty(), "a bool is not a float");
}

/// A name the host does not expose is not a call, whatever it looks like.
#[test]
fn a_name_the_host_does_not_expose_is_not_callable() {
    let bare = NativeRegistry::new();
    let found = errors("fn float Main() { return Abs(-3.0); }", &bare);
    assert!(!found.is_empty(), "the bare host exposes nothing");
}

/// **The shadowing rule.** A script redefining an engine function would compile
/// — the checker would simply prefer the script's — and every existing call
/// would quietly start meaning something else.
#[test]
fn a_script_cannot_redefine_an_engine_function() {
    let natives = NativeRegistry::with_builtins();
    let found = errors(
        "fn float Abs(float x) { return 0.0; }
         fn float Main() { return Abs(-3.0); }",
        &natives,
    );
    let complaint = found
        .iter()
        .find(|m| m.contains("Abs"))
        .unwrap_or_else(|| panic!("expected a clash, got {found:?}"));
    assert!(complaint.contains("engine function"), "got: {complaint}");
}

/// A name the host does not expose is free for a script to use.
#[test]
fn a_script_may_declare_a_name_the_host_does_not_expose() {
    let natives = NativeRegistry::with_builtins();
    let program = build(
        "fn float Scale(float x) { return x * 2.0; }
         fn float Main() { return Scale(Abs(-3.0)); }",
        &natives,
    );

    let mut host = Host::new();
    let mut machine = Machine::new(&program, "Main", &[]).expect("Main exists");
    assert_eq!(machine.run(&program, &mut host, u64::MAX), Run::Completed);
    assert_eq!(machine.result().as_float(), Some(6.0));
}

// ─── What the VM must refuse ────────────────────────────────────────────────

/// A native faults the behavior rather than returning a `NaN` that propagates
/// silently and surfaces later as an entity that has vanished.
#[test]
fn a_native_that_refuses_faults_the_program() {
    let natives = NativeRegistry::with_builtins();
    let program = build("fn float Main() { return Sqrt(-1.0); }", &natives);

    let mut host = Host::new();
    let mut machine = Machine::new(&program, "Main", &[]).expect("Main exists");
    match machine.run(&program, &mut host, u64::MAX) {
        Run::Faulted(Fault::NativeFailed { name, message }) => {
            assert_eq!(name, "Sqrt");
            assert!(message.contains("root"), "got: {message}");
        }
        other => panic!("expected a fault, got {other:?}"),
    }
}

#[test]
fn an_empty_range_is_refused_rather_than_guessed_at() {
    let natives = NativeRegistry::with_builtins();
    let program = build("fn float Main() { return Clamp(1.0, 5.0, 0.0); }", &natives);

    let mut host = Host::new();
    let mut machine = Machine::new(&program, "Main", &[]).expect("Main exists");
    assert!(matches!(
        machine.run(&program, &mut host, u64::MAX),
        Run::Faulted(Fault::NativeFailed { .. })
    ));
}

/// **The indices are the contract.** A program compiled against one registry and
/// run against a smaller one must fault rather than call whatever now sits at
/// that slot.
#[test]
fn a_program_run_against_the_wrong_registry_faults() {
    let natives = NativeRegistry::with_builtins();
    let program = build("fn float Main() { return Abs(-3.0); }", &natives);

    let mut host = Host::bare();
    let mut machine = Machine::new(&program, "Main", &[]).expect("Main exists");
    assert!(
        matches!(
            machine.run(&program, &mut host, u64::MAX),
            Run::Faulted(Fault::UnknownNative { .. })
        ),
        "a bare host exposes nothing to call"
    );
}

// ─── Fuel ───────────────────────────────────────────────────────────────────

/// A native is billed what it declares, not what an instruction costs. Without
/// that a behavior could spend a frame inside one call while the counter
/// reported it had barely started.
#[test]
fn a_native_is_charged_its_own_cost() {
    static EXPENSIVE: NativeFn = NativeFn {
        name: "Expensive",
        params: &[],
        result: NativeTy::Float,
        cost: 5_000,
        call: |_, _| Ok(Value::Float(1.0)),
    };

    let mut natives = NativeRegistry::new();
    natives.register(&EXPENSIVE);
    let program = build("fn float Main() { return Expensive(); }", &natives);

    let mut host = Host {
        natives,
        ..Host::new()
    };
    let mut machine = Machine::new(&program, "Main", &[]).expect("Main exists");

    // Enough fuel for a handful of instructions, nowhere near the call's price.
    assert_eq!(
        machine.run(&program, &mut host, 100),
        Run::Suspended(crate::vm::Suspension::OutOfFuel),
        "the call must not be affordable"
    );
    assert_eq!(machine.run(&program, &mut host, u64::MAX), Run::Completed);
}

// ─── The registry ───────────────────────────────────────────────────────────

#[test]
fn a_registry_hands_back_what_was_registered() {
    let natives = NativeRegistry::with_builtins();
    assert_eq!(natives.len(), builtins().len());
    assert!(natives.get("Abs").is_some());
    assert!(natives.get("Raycast").is_none());

    let index = natives.index_of("Abs").expect("registered");
    assert_eq!(natives.at(index).map(|f| f.name), Some("Abs"));
}

/// Re-registering a name replaces it **in place**. Appending instead would move
/// every later index, and a program compiled a moment earlier would start
/// calling its neighbour.
#[test]
fn replacing_a_function_leaves_every_other_index_alone() {
    static PATCHED: NativeFn = NativeFn {
        name: "Abs",
        params: &[NativeTy::Float],
        result: NativeTy::Float,
        cost: 1,
        call: |_, _| Ok(Value::Float(99.0)),
    };

    let mut natives = NativeRegistry::with_builtins();
    let before: Vec<_> = natives.iter().map(|f| f.name).collect();
    let lerp_index = natives.index_of("Lerp").expect("registered");

    let index = natives.register(&PATCHED);

    assert_eq!(index, natives.index_of("Abs").expect("still there"));
    assert_eq!(natives.len(), before.len(), "nothing was appended");
    assert_eq!(
        natives.index_of("Lerp"),
        Some(lerp_index),
        "an unrelated index did not move"
    );
}

#[test]
fn a_bare_registry_exposes_nothing() {
    let natives = NativeRegistry::new();
    assert!(natives.is_empty());
    assert!(natives.get("Abs").is_none());
}

// ─── Signatures ─────────────────────────────────────────────────────────────

#[test]
fn a_builtins_signature_reaches_the_checker_intact() {
    let clamp = NativeRegistry::with_builtins()
        .get("Clamp")
        .expect("registered");

    assert_eq!(clamp.params.len(), 3);
    assert!(clamp.params.iter().all(|p| p.to_ty() == Ty::Float));
    assert_eq!(clamp.result.to_ty(), Ty::Float);
}

// ─── The context ────────────────────────────────────────────────────────────

/// A native reaches the command buffer, not the `World` — which is what lets
/// the lane it runs inside stay free of one.
#[test]
fn a_native_can_queue_an_effect() {
    use khora_core::ecs::entity::EntityId;
    use khora_core::math::Vec3;
    use khora_core::script::WorldCommand;

    static NUDGE: NativeFn = NativeFn {
        name: "Nudge",
        params: &[],
        result: NativeTy::Void,
        cost: 1,
        call: |context: &mut NativeContext<'_>, _| {
            let entity = context
                .entity
                .ok_or_else(|| NativeError::new("`Nudge` needs an entity to move"))?;
            context.commands.push(WorldCommand::Translate {
                entity,
                delta: Vec3::new(1.0, 0.0, 0.0),
            });
            Ok(Value::Unit)
        },
    };

    let mut natives = NativeRegistry::new();
    natives.register(&NUDGE);
    let program = build("fn void Main() { Nudge(); }", &natives);

    let entity = EntityId {
        index: 4,
        generation: 1,
    };
    let mut host = Host {
        natives,
        ..Host::new()
    }
    .for_entity(entity);

    assert_eq!(run(&program, &mut host), Run::Completed);
    assert_eq!(host.commands.len(), 1);
    assert_eq!(host.commands.as_slice()[0].entity(), Some(entity));
}

/// The same native without a subject says so rather than picking one.
#[test]
fn a_native_needing_an_entity_says_so_when_there_is_none() {
    static NEEDS_SUBJECT: NativeFn = NativeFn {
        name: "Nudge",
        params: &[],
        result: NativeTy::Void,
        cost: 1,
        call: |context: &mut NativeContext<'_>, _| {
            context
                .entity
                .ok_or_else(|| NativeError::new("`Nudge` needs an entity to move"))?;
            Ok(Value::Unit)
        },
    };

    let mut natives = NativeRegistry::new();
    natives.register(&NEEDS_SUBJECT);
    let program = build("fn void Main() { Nudge(); }", &natives);

    let mut host = Host {
        natives,
        ..Host::new()
    };
    assert!(matches!(
        run(&program, &mut host),
        Run::Faulted(Fault::NativeFailed { .. })
    ));
}

/// Ending a frame frees the arena and hands over the commands together —
/// releasing the arena while a command still referred to it is the mistake the
/// generation counter exists to catch.
#[test]
fn ending_a_frame_releases_the_arena_and_yields_the_commands() {
    use khora_core::ecs::entity::EntityId;
    use khora_core::script::WorldCommand;

    let mut host = Host::new();
    host.commands.push(WorldCommand::Despawn {
        entity: EntityId {
            index: 1,
            generation: 1,
        },
    });
    let generation = host.arena.generation();

    let taken = host.end_frame();

    assert_eq!(taken.len(), 1, "the commands came out");
    assert!(host.commands.is_empty(), "and are no longer held");
    assert_ne!(host.arena.generation(), generation, "the arena was freed");
}

// ─── Strings ────────────────────────────────────────────────────────────────

mod strings {
    use super::{build, Host, NativeRegistry};
    use crate::vm::{Fault, Machine, Program, Run, StrRef, Value};

    /// Runs `Main` and hands back both the machine and the host, so a test can
    /// resolve the string it produced.
    fn run(source: &str) -> (Program, Machine, Host) {
        let natives = NativeRegistry::with_builtins();
        let program = build(source, &natives);

        let mut host = Host::new();
        let mut machine = Machine::new(&program, "Main", &[]).expect("Main exists");
        assert_eq!(machine.run(&program, &mut host, u64::MAX), Run::Completed);
        (program, machine, host)
    }

    fn text(source: &str) -> String {
        let (program, machine, host) = run(source);
        machine
            .resolve_str(machine.result(), &program, &host)
            .expect("a string came back")
            .to_owned()
    }

    fn truth(source: &str) -> bool {
        let (_, machine, _) = run(source);
        machine.result().as_bool().expect("a bool came back")
    }

    /// **The milestone.** A literal travels from source to a value and back.
    #[test]
    fn a_literal_reaches_the_result() {
        assert_eq!(text(r#"fn string Main() { return "hello"; }"#), "hello");
    }

    #[test]
    fn escapes_and_non_ascii_survive() {
        assert_eq!(text(r#"fn string Main() { return "a\nb"; }"#), "a\nb");
        assert_eq!(text(r#"fn string Main() { return "héllo →"; }"#), "héllo →");
    }

    /// A literal is a *reference* into the program, so it costs nothing to name
    /// — which is what keeps one inside a loop from allocating per iteration.
    #[test]
    fn a_literal_stays_in_the_program_rather_than_the_arena() {
        let (_, machine, _) = run(r#"fn string Main() { return "hello"; }"#);
        assert!(matches!(
            machine.result().as_str_ref(),
            Some(StrRef::Const(_))
        ));
    }

    /// The same text written twice is one entry. A literal is immutable, so
    /// sharing one is indistinguishable from two copies — except in size.
    #[test]
    fn the_same_literal_is_stored_once() {
        let (program, _, _) = run(r#"fn string Main() { return "hit"; }
               fn string Again() { return "hit"; }
               fn string Other() { return "miss"; }"#);
        assert_eq!(program.strings.len(), 2, "{:?}", program.strings);
    }

    /// **Joining is not adding.** `+` on two strings allocates, so it compiles
    /// to its own instruction rather than to arithmetic that would fault.
    #[test]
    fn two_strings_join() {
        assert_eq!(text(r#"fn string Main() { return "a" + "b"; }"#), "ab");
        assert_eq!(
            text(r#"fn string Main() { return "pv: " + "100" + "!"; }"#),
            "pv: 100!"
        );
    }

    /// Joined text did not exist when the program was compiled, so unlike a
    /// literal it lives in the arena — and lasts one frame, like everything
    /// else there.
    #[test]
    fn joined_text_lives_in_the_arena() {
        let (_, machine, _) = run(r#"fn string Main() { return "a" + "b"; }"#);
        assert!(matches!(
            machine.result().as_str_ref(),
            Some(StrRef::Arena(_))
        ));
    }

    /// **What makes two representations workable.** A literal and a computed
    /// string spelling the same thing are equal — comparing the references
    /// would answer no, which is the case a script most often means to test.
    #[test]
    fn a_literal_equals_the_same_text_built_at_runtime() {
        assert!(truth(r#"fn bool Main() { return "ab" == "a" + "b"; }"#));
        assert!(truth(r#"fn bool Main() { return "a" + "b" == "ab"; }"#));
    }

    #[test]
    fn different_text_is_not_equal() {
        assert!(!truth(r#"fn bool Main() { return "a" == "b"; }"#));
        assert!(truth(r#"fn bool Main() { return "a" != "b"; }"#));
    }

    #[test]
    fn a_string_passes_through_a_local() {
        assert_eq!(
            text(r#"fn string Main() { var greeting = "hi"; return greeting + "!"; }"#),
            "hi!"
        );
    }

    /// A string reaches an engine function, which is what `Log` needs.
    #[test]
    fn a_string_reaches_a_native() {
        let (_, machine, _) = run(r#"fn int Main() { return Length("héllo"); }"#);
        assert_eq!(machine.result().as_int(), Some(5), "characters, not bytes");
    }

    #[test]
    fn joined_text_reaches_a_native_too() {
        let (_, machine, _) = run(r#"fn int Main() { return Length("ab" + "cde"); }"#);
        assert_eq!(machine.result().as_int(), Some(5));
    }

    #[test]
    fn logging_a_string_runs() {
        let (_, machine, _) = run(r#"fn void Main() { Log("a script said this"); }"#);
        assert!(machine.is_finished());
    }

    /// **The guard the arena exists for.** Text from a previous frame does not
    /// read as whatever landed at its index — the generation catches it.
    #[test]
    fn text_from_a_previous_frame_is_refused() {
        let (program, machine, mut host) = run(r#"fn string Main() { return "a" + "b"; }"#);
        let stale = machine.result();

        host.end_frame();
        assert!(matches!(
            machine.resolve_str(stale, &program, &host),
            Err(Fault::BadString)
        ));
    }

    /// A string is not a number, and the checker says so rather than the VM
    /// finding out.
    #[test]
    fn arithmetic_on_a_string_is_refused_at_compile_time() {
        let natives = NativeRegistry::with_builtins();
        let found = super::errors(r#"fn string Main() { return "a" - "b"; }"#, &natives);
        assert!(!found.is_empty(), "subtraction is not defined on text");
    }

    #[test]
    fn a_string_where_a_number_belongs_is_refused() {
        let natives = NativeRegistry::with_builtins();
        let found = super::errors(r#"fn float Main() { return Abs("a"); }"#, &natives);
        assert!(!found.is_empty());
    }

    /// A machine mid-run is saved with its program, so a string reference in a
    /// register has to survive the trip.
    #[test]
    fn a_string_value_survives_serialization() {
        let value = Value::Str(StrRef::Const(3));
        let json = serde_json::to_string(&value).expect("serialises");
        assert_eq!(
            serde_json::from_str::<Value>(&json).expect("deserialises"),
            value
        );
    }
}

// ─── The macro ──────────────────────────────────────────────────────────────

mod macro_tests {
    use super::{build, Host, NativeRegistry};
    use crate::vm::{Machine, Run, Value};
    use khora_core::ecs::entity::EntityId;
    use khora_core::math::Vec3;
    use khora_core::script::WorldCommand;
    use khora_script::ergon_fn;

    /// A pure function: no context, one argument, one result.
    ///
    /// Named `probe_*` rather than `double`, because `#[ergon_fn]` submits to
    /// `inventory` and the whole test binary shares one collection: a plain name
    /// here reserves it for every other test's scripts, which is the engine
    /// surface doing exactly what it should — to somebody who did not ask.
    #[ergon_fn]
    fn probe_double(x: f32) -> f32 {
        x * 2.0
    }

    /// The Rust name is `snake_case` and the script name is `PascalCase`; the
    /// author writes one and gets the other.
    #[ergon_fn]
    fn half_of(x: f32) -> f32 {
        x / 2.0
    }

    /// An explicit name, for when the mechanical answer is wrong.
    #[ergon_fn(name = "Hypot", cost = 4)]
    fn hypotenuse(a: f32, b: f32) -> f32 {
        (a * a + b * b).sqrt()
    }

    /// An effect: reaches the command buffer through the context, never the
    /// `World`.
    #[ergon_fn]
    fn shove(context: &mut crate::native::NativeContext<'_>, entity: EntityId, by: f32) {
        context.commands.push(WorldCommand::Translate {
            entity,
            delta: Vec3::new(by, 0.0, 0.0),
        });
    }

    /// A function that answers with an entity, so the type travels both ways.
    #[ergon_fn]
    fn subject(context: &mut crate::native::NativeContext<'_>) -> Option<EntityId> {
        context.entity
    }

    fn registry() -> NativeRegistry {
        let mut natives = NativeRegistry::with_builtins();
        for function in [
            &ERGON_PROBE_DOUBLE,
            &ERGON_HALF_OF,
            &ERGON_HYPOTENUSE,
            &ERGON_SHOVE,
            &ERGON_SUBJECT,
        ] {
            natives.register(function);
        }
        natives
    }

    /// **What the macro is for.** An annotated Rust function is callable from a
    /// script — no separate list, no hand-written signature.
    #[test]
    fn an_annotated_function_is_callable_from_a_script() {
        let natives = registry();
        let program = build("fn float Main() { return ProbeDouble(21.0); }", &natives);

        let mut host = Host {
            natives,
            ..Host::new()
        };
        let mut machine = Machine::new(&program, "Main", &[]).expect("Main exists");
        assert_eq!(machine.run(&program, &mut host, u64::MAX), Run::Completed);
        assert_eq!(machine.result().as_float(), Some(42.0));
    }

    #[test]
    fn the_script_name_is_the_rust_name_in_pascal_case() {
        let natives = registry();
        assert!(natives.get("HalfOf").is_some(), "half_of → HalfOf");
        assert!(natives.get("half_of").is_none());
    }

    #[test]
    fn an_explicit_name_and_cost_are_honoured() {
        let natives = registry();
        let hypot = natives.get("Hypot").expect("registered under its own name");
        assert_eq!(hypot.cost, 4);
        assert!(natives.get("Hypotenuse").is_none());
    }

    #[test]
    fn a_two_argument_function_gets_them_in_order() {
        let natives = registry();
        let program = build("fn float Main() { return Hypot(3.0, 4.0); }", &natives);

        let mut host = Host {
            natives,
            ..Host::new()
        };
        let mut machine = Machine::new(&program, "Main", &[]).expect("Main exists");
        assert_eq!(machine.run(&program, &mut host, u64::MAX), Run::Completed);
        assert_eq!(machine.result().as_float(), Some(5.0));
    }

    /// The signature the checker sees comes from the Rust types, resolved
    /// through `ScriptType` rather than read off the written tokens.
    #[test]
    fn the_signature_is_taken_from_the_rust_types() {
        use crate::native::NativeTy;

        let natives = registry();
        let shove = natives.get("Shove").expect("registered");
        assert_eq!(shove.params, &[NativeTy::Entity, NativeTy::Float]);
        assert_eq!(shove.result, NativeTy::Void);

        let subject = natives.get("Subject").expect("registered");
        assert!(subject.params.is_empty(), "the context is not an argument");
        assert_eq!(subject.result, NativeTy::Optional(&NativeTy::Entity));
    }

    /// A leading context parameter is passed through by the engine, not by the
    /// script — which is how a function reaches the command buffer.
    #[test]
    fn a_context_parameter_is_not_an_argument_the_script_supplies() {
        let natives = registry();
        let program = build(
            "fn void Main(Entity e) { Shove(e, 2.0); }
             fn void Unused() { }",
            &natives,
        );

        let entity = EntityId {
            index: 3,
            generation: 1,
        };
        let mut host = Host {
            natives,
            ..Host::new()
        };
        let mut machine =
            Machine::new(&program, "Main", &[Value::Entity(entity)]).expect("Main exists");
        assert_eq!(machine.run(&program, &mut host, u64::MAX), Run::Completed);

        assert_eq!(host.commands.len(), 1);
        assert_eq!(
            host.commands.as_slice()[0],
            WorldCommand::Translate {
                entity,
                delta: Vec3::new(2.0, 0.0, 0.0),
            }
        );
    }

    /// The annotated function is still an ordinary Rust function — the macro
    /// adds to it rather than replacing it.
    #[test]
    fn the_function_is_still_callable_from_rust() {
        assert_eq!(probe_double(4.0), 8.0);
        assert_eq!(hypotenuse(3.0, 4.0), 5.0);
    }

    /// A default cost of one instruction: a function that does real work has to
    /// say so, and the macro will not guess on its behalf.
    #[test]
    fn the_default_cost_is_one_instruction() {
        assert_eq!(registry().get("ProbeDouble").expect("registered").cost, 1);
    }

    /// Annotating is the whole registration — the function reaches the registry
    /// without being named anywhere else.
    #[test]
    fn an_annotated_function_is_discovered_without_being_listed() {
        let discovered = NativeRegistry::discovered();

        for name in ["ProbeDouble", "HalfOf", "Hypot", "Shove", "Subject"] {
            assert!(
                discovered.get(name).is_some(),
                "`{name}` was annotated but not discovered"
            );
        }
        assert!(
            discovered.get("Abs").is_some(),
            "the built-ins are still in"
        );
    }

    /// **The determinism that makes an index safe.** `inventory` yields
    /// submissions in link order, which is not stable across builds — and a
    /// program addresses a native by index, so an unstable order would make a
    /// call mean one function today and its neighbour after a relink.
    #[test]
    fn discovery_produces_the_same_order_every_time() {
        let first: Vec<_> = NativeRegistry::discovered()
            .iter()
            .map(|f| f.name)
            .collect();
        let second: Vec<_> = NativeRegistry::discovered()
            .iter()
            .map(|f| f.name)
            .collect();
        assert_eq!(first, second);

        // The built-ins keep their written order; the discovered ones follow,
        // sorted, so the sequence is a property of the code rather than of the
        // linker.
        let discovered_names = &first[super::builtins().len()..];
        let mut sorted = discovered_names.to_vec();
        sorted.sort();
        assert_eq!(discovered_names, sorted.as_slice());
    }
}

// ─── Built-in behaviour worth pinning ───────────────────────────────────────

/// `f32::signum` answers 1.0 for +0.0 and -1.0 for -0.0, which reads as a
/// direction where there is none.
#[test]
fn the_sign_of_zero_is_zero() {
    assert_eq!(float_result("Sign(0.0)"), 0.0);
    assert_eq!(float_result("Sign(-2.5)"), -1.0);
    assert_eq!(float_result("Sign(2.5)"), 1.0);
}

/// Unclamped on purpose: overshooting is how a spring or an ease-out is
/// written, and clamping here would quietly remove that.
#[test]
fn lerp_is_not_clamped() {
    assert_eq!(float_result("Lerp(0.0, 10.0, 1.5)"), 15.0);
    assert_eq!(float_result("Lerp(0.0, 10.0, -0.5)"), -5.0);
}

#[test]
fn rounding_goes_the_ways_it_says() {
    assert_eq!(float_result("Floor(2.7)"), 2.0);
    assert_eq!(float_result("Ceil(2.1)"), 3.0);
    assert_eq!(float_result("Round(2.5)"), 3.0);
    assert_eq!(float_result("Round(-2.5)"), -3.0);
}
