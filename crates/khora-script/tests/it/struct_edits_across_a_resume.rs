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

//! A suspended frame resumed after the struct it uses was edited.
//!
//! A frame stopped at a site is rebuilt in the edited code only when what it
//! held still means what the edited code expects there. A struct is carried by
//! name, its fields matched by name — so an edit that reorders a struct's
//! fields, or retypes one, must leave a resumed frame either rebuilt correctly
//! or not rebuilt at all (restarted, or abandoned). It must never finish by
//! faulting on a value the old code put where the new code expects another.

use khora_core::script::PendingBody;
use khora_script::vm::{resume, Machine, Program, Run, Value};
use khora_script::{check, compile, lex, parse, Host};

fn build(source: &str) -> Program {
    let lexed = lex(source);
    assert!(lexed.diagnostics.is_empty(), "{:?}", lexed.diagnostics);
    let parsed = parse(lexed.tokens);
    assert!(parsed.diagnostics.is_empty(), "{:?}", parsed.diagnostics);
    let checked = check(&parsed.module);
    assert!(!checked.has_errors(), "{:?}", checked.diagnostics);
    let compiled = compile(&parsed.module);
    assert!(!compiled.has_errors(), "{:?}", compiled.diagnostics);
    compiled.program
}

/// Runs to the end, a fresh frame per suspension.
fn finish(machine: &mut Machine, program: &Program) -> Result<Value, String> {
    let mut host = Host::new();
    for _ in 0..10_000 {
        match machine.run(program, &mut host, u64::MAX) {
            Run::Suspended(_) => {
                host.awaiting = None;
                host.end_frame();
            }
            Run::Completed => return Ok(machine.result()),
            Run::Faulted(fault) => return Err(format!("{fault:?}")),
        }
    }
    Err("never finished".to_owned())
}

/// What a frame frozen in `original` does once resumed into `edited`:
/// `None` when the resume abandoned it, else how it finished.
fn resumed(
    machine: &Machine,
    original: &Program,
    edited: &Program,
) -> Option<Result<Value, String>> {
    let frozen = machine
        .freeze(original, PendingBody::Sequence)
        .expect("the machine freezes");
    let (mut machine, _) = resume(&frozen, original.fingerprint(), edited).ok()?;
    Some(finish(&mut machine, edited))
}

const PAIR: &str = "struct Pair { int a; int b; }
                    fn int Seven() { return 7; }
                    fn int Two() { return 2; }
                    behavior Guard {
                        int Build() { Pair p = Pair { a: Seven(), b: Two() }; return p.a * 10 + p.b; }
                    }";

/// **A frame cut inside a struct literal survives a reorder of the struct's
/// fields.** `Pair { a: Seven(), b: Two() }` is cut by fuel at every place it
/// can be — inside a call, after a call returns — and resumed into code
/// where `Pair` declares `b` before `a`. Whatever the resume decides, the
/// literal finishes as `Pair { a: 7, b: 2 }`: never a field left unwritten,
/// never a fault.
#[test]
fn a_frame_cut_inside_a_struct_literal_survives_a_reorder_of_its_fields() {
    let original = build(PAIR);
    let edited = build(&PAIR.replace(
        "struct Pair { int a; int b; }",
        "struct Pair { int b; int a; }",
    ));

    let mut wrong = Vec::new();
    for cut in 1..40 {
        let mut machine = Machine::new(&original, "Guard.Build", &[]).expect("`Build` exists");
        let mut host = Host::new();
        let mut cuts = 0;
        let stopped = loop {
            match machine.run(&original, &mut host, 1) {
                Run::Suspended(_) => {
                    cuts += 1;
                    if cuts == cut {
                        break true;
                    }
                }
                Run::Completed => break false,
                Run::Faulted(fault) => panic!("the original faulted: {fault:?}"),
            }
        };
        if !stopped {
            break;
        }
        match resumed(&machine, &original, &edited) {
            None | Some(Ok(Value::Int(72))) => {}
            Some(other) => wrong.push(format!("cut {cut}: {other:?}")),
        }
    }
    assert!(wrong.is_empty(), "{}", wrong.join("\n"));
}

const HOLDER: &str = "struct Pair { int a; int b; }
                      fn Pair Make() { return Pair { a: 3, b: 4 }; }
                      behavior Guard {
                          async int Hold() { Pair p = Make(); await 1.0s; return p.b; }
                      }";

/// **A held struct whose field was retyped is not rebuilt with the old
/// kind.** A frame holds `Pair { a: 3, b: 4 }` at its `await`; the edit
/// declares `a` a `string` and reads `p.a.Length` after the `await`. The
/// held `a` is an `int`, which no `string` can be: the frame may restart or
/// be abandoned, but it must not be rebuilt holding an `int` where the code
/// reads text, and fault there.
#[test]
fn a_held_struct_whose_field_was_retyped_is_not_rebuilt_with_the_old_kind() {
    let original = build(HOLDER);
    let edited = build(
        &HOLDER
            .replace(
                "struct Pair { int a; int b; }",
                "struct Pair { string a; int b; }",
            )
            .replace("Pair { a: 3, b: 4 }", "Pair { a: \"abc\", b: 4 }")
            .replace("return p.b;", "return p.a.Length * 10 + p.b;"),
    );

    let mut machine = Machine::new(&original, "Guard.Hold", &[]).expect("`Hold` exists");
    let mut host = Host::new();
    assert!(
        matches!(
            machine.run(&original, &mut host, u64::MAX),
            Run::Suspended(_)
        ),
        "the premise: it stopped at its await"
    );
    let result = resumed(&machine, &original, &edited);
    assert!(
        matches!(result, None | Some(Ok(Value::Int(34)))),
        "abandoned, or finished reading `a` as text: {result:?}"
    );
}
