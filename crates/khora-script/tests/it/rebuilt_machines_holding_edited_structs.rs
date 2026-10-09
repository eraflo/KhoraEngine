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

//! A rebuilt machine holding a struct edited since it was frozen.
//!
//! A rebuild keeps an unchanged frame as it was frozen — its registers too —
//! and lays an edited frame out again with the locals it matched. Either can
//! hold a struct whose declaration changed so that the held value no longer
//! goes back into the arena: a field its code never names, retyped; a field
//! added with neither default nor zero. The machine must still resume — the
//! field at its zero, or the body restarted — and never fault on its first
//! run.

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
                          async int Wait() { await 1.0s; return 1; }
                          async int Hold() { Pair p = Make(); int n = Wait(); return p.b * 10 + n; }
                      }";

/// **A caller kept unchanged by a rebuild, holding a struct whose unread
/// field was retyped, resumes.** `Hold` holds `p` and waits inside `Wait`.
/// The edit declares `Pair.a` a `string` and changes `Make` and `Wait` only:
/// `Hold`, which reads `p.b` alone, keeps its fingerprint and is kept as
/// frozen, while `Wait` is rebuilt at its await. The held `Pair` still has
/// `a: 3`. The resumed body finishes — `41` with `p` kept, `42` restarted —
/// and does not fault on the held `int`.
#[test]
fn a_rebuilt_caller_holding_a_struct_whose_unread_field_was_retyped_resumes() {
    let original = build(HOLDER);
    let edited = build(
        &HOLDER
            .replace(
                "struct Pair { int a; int b; }",
                "struct Pair { string a; int b; }",
            )
            .replace("Pair { a: 3, b: 4 }", "Pair { a: \"abc\", b: 4 }")
            .replace("await 1.0s; return 1;", "await 1.0s; return 2;"),
    );

    let mut machine = Machine::new(&original, "Guard.Hold", &[]).expect("`Hold` exists");
    let mut host = Host::new();
    assert!(
        matches!(
            machine.run(&original, &mut host, u64::MAX),
            Run::Suspended(_)
        ),
        "the premise: it stopped at the await in `Wait`"
    );
    assert_eq!(machine.depth(), 2, "the premise: `Hold` waits on `Wait`");
    let frozen = machine
        .freeze(&original, PendingBody::Sequence)
        .expect("the machine freezes");
    let (mut resumed, tier) =
        resume(&frozen, original.fingerprint(), &edited).expect("`Hold` still exists");
    let result = finish(&mut resumed, &edited);
    assert!(
        matches!(result, Ok(Value::Int(41 | 42))),
        "resumed at tier {tier:?}: {result:?}"
    );
}

const ADDER: &str = "struct Inner { int x; }
                     struct Pair { int a; int b; }
                     fn Pair Make() { return Pair { a: 3, b: 4 }; }
                     behavior Guard {
                         async int Hold() { Pair p = Make(); await 1.0s; return p.b; }
                     }";

/// **A rebuilt frame holding a struct that gained a field with no zero
/// resumes.** The edit adds `Inner i;` to `Pair` — no default, and a struct
/// has no zero — and edits `Hold`, so its frame is rebuilt at its await
/// holding the old `Pair`, which cannot take the new field. The frame must not
/// be rebuilt with it: the body restarts (or is abandoned), and never faults
/// on its first run.
#[test]
fn a_rebuilt_frame_holding_a_struct_that_gained_a_field_with_no_zero_resumes() {
    let original = build(ADDER);
    let edited = build(
        &ADDER
            .replace(
                "struct Pair { int a; int b; }",
                "struct Pair { int a; int b; Inner i; }",
            )
            .replace(
                "Pair { a: 3, b: 4 }",
                "Pair { a: 3, b: 4, i: Inner { x: 1 } }",
            )
            .replace("return p.b; }", "return p.b + 1; }"),
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
    if let Ok((mut resumed, tier)) = resume(&frozen, original.fingerprint(), &edited) {
        let result = finish(&mut resumed, &edited);
        assert_eq!(result, Ok(Value::Int(5)), "resumed at tier {tier:?}");
    }
}
