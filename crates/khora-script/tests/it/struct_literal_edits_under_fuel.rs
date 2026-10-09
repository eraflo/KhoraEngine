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

//! A struct literal cut by fuel, resumed into code that edited its struct.

use khora_core::script::PendingBody;
use khora_script::arena::Owned;
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

const BAG: &str = "struct Bag { int[] xs; int n; }
                   fn int Two() { return 2; }
                   behavior Guard {
                       int Build() {
                           int[] ys = [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20];
                           Bag b = Bag { n: Two(), xs: ys };
                           return b.xs.Length * 10 + b.n;
                       }
                   }";

/// **A literal cut anywhere — at a call, at the copy of a written array —
/// survives an edit that reorders its struct and adds a defaulted field.**
#[test]
fn a_struct_literal_cut_at_every_slice_survives_a_reorder_and_an_added_field() {
    let original = build(BAG);
    let edited = build(&BAG.replace(
        "struct Bag { int[] xs; int n; }",
        "struct Bag { int c = 5; int n; int[] xs; }",
    ));

    let mut wrong = Vec::new();
    for cut in 1..200 {
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
        let frozen = machine
            .freeze(&original, PendingBody::Sequence)
            .expect("the machine freezes");
        let Ok((mut resumed, tier)) = resume(&frozen, original.fingerprint(), &edited) else {
            continue;
        };
        match finish(&mut resumed, &edited) {
            Ok(Value::Int(202)) => {}
            other => wrong.push(format!("cut {cut} ({tier:?}): {other:?}")),
        }
    }
    assert!(wrong.is_empty(), "{}", wrong.join("\n"));
}

/// **A saved value missing a field whose default is a literal of a struct
/// declared later is filled whole** — that literal's own missing fields
/// included.
#[test]
fn a_partial_struct_takes_a_default_whose_literal_names_a_later_struct() {
    let program = build(
        "struct Outer { Inner i = Inner { n: 2 }; int k; }
         struct Inner { int n; int m = 5; }
         fn int G() { return 0; }",
    );
    let mut host = Host::new();
    let saved = Owned::Struct {
        name: "Outer".to_owned(),
        fields: vec![("k".to_owned(), Owned::Scalar(Value::Int(1)))],
    };
    let value = host.arena.import(&saved, &program).expect("it fits");
    let back = host.arena.export(value, &program).expect("it exports");
    assert_eq!(
        back,
        Owned::Struct {
            name: "Outer".to_owned(),
            fields: vec![
                (
                    "i".to_owned(),
                    Owned::Struct {
                        name: "Inner".to_owned(),
                        fields: vec![
                            ("n".to_owned(), Owned::Scalar(Value::Int(2))),
                            ("m".to_owned(), Owned::Scalar(Value::Int(5))),
                        ],
                    }
                ),
                ("k".to_owned(), Owned::Scalar(Value::Int(1))),
            ],
        }
    );
}

const NESTED: &str = "struct Inner { int a; int b; }
                      struct Outer { Inner i; int k; }
                      fn int Seven() { return 7; }
                      fn int Two() { return 2; }
                      fn int Three() { return 3; }
                      behavior Guard {
                          int Build() {
                              Outer o = Outer { k: Three(), i: Inner { b: Two(), a: Seven() } };
                              return o.i.a * 100 + o.i.b * 10 + o.k;
                          }
                      }";

/// **A literal written inside another, both cut at every slice, survives a
/// reorder of both structs.**
#[test]
fn a_nested_struct_literal_cut_at_every_slice_survives_a_reorder_of_both() {
    let original = build(NESTED);
    let edited = build(
        &NESTED
            .replace(
                "struct Inner { int a; int b; }",
                "struct Inner { int b; int a; }",
            )
            .replace(
                "struct Outer { Inner i; int k; }",
                "struct Outer { int k; Inner i; }",
            ),
    );

    let mut wrong = Vec::new();
    for cut in 1..200 {
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
        let frozen = machine
            .freeze(&original, PendingBody::Sequence)
            .expect("the machine freezes");
        let Ok((mut resumed, tier)) = resume(&frozen, original.fingerprint(), &edited) else {
            continue;
        };
        match finish(&mut resumed, &edited) {
            Ok(Value::Int(723)) => {}
            other => wrong.push(format!("cut {cut} ({tier:?}): {other:?}")),
        }
    }
    assert!(wrong.is_empty(), "{}", wrong.join("\n"));
}
