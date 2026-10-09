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

//! A suspended frame whose own code is unchanged, holding a struct the edit
//! changed.
//!
//! A function's fingerprint names a struct by its name and the fields it
//! reads — not by the types of the fields it never touches. So an edit that
//! retypes a field elsewhere leaves a holding frame *unchanged*, and it is
//! thawed as it was frozen, its held struct still the old kind. Going back
//! into the arena, that struct no longer fits its declaration: the frame must
//! still resume — the retyped field at its default, or the frame restarted —
//! and never fault as though the arena were full.

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

const HOLDER: &str = "struct Pair { int a; int b; }
                      fn Pair Make() { return Pair { a: 3, b: 4 }; }
                      behavior Guard {
                          async int Hold() { Pair p = Make(); await 1.0s; return p.b; }
                      }";

/// **A frame holding a struct whose unread field was retyped resumes.** The
/// edit declares `Pair.a` a `string` and changes only `Make`, so `Hold` —
/// which reads `p.b` alone — keeps its fingerprint and is thawed unchanged,
/// holding `a: 3`. It finishes with `4`, whether `a` took its zero or the
/// frame restarted; it does not fault on the held `int`.
#[test]
fn a_frame_holding_a_struct_whose_unread_field_was_retyped_resumes() {
    let original = build(HOLDER);
    let edited = build(
        &HOLDER
            .replace(
                "struct Pair { int a; int b; }",
                "struct Pair { string a; int b; }",
            )
            .replace("Pair { a: 3, b: 4 }", "Pair { a: \"abc\", b: 4 }"),
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
    let frozen = machine
        .freeze(&original, PendingBody::Sequence)
        .expect("the machine freezes");
    let (mut resumed, tier) =
        resume(&frozen, original.fingerprint(), &edited).expect("`Hold` still exists");
    let result = finish(&mut resumed, &edited);
    assert_eq!(result, Ok(Value::Int(4)), "resumed at tier {tier:?}");
}
