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

//! Which struct defaults build themselves without end, and which do not.
//!
//! A default is refused when a literal in it leaves out a field whose own
//! default needs it again — through an array literal too, and whether or not
//! the struct is ever built. A field written `null` ends the chain.

use khora_script::vm::{Machine, Run, Value};
use khora_script::{check, compile, lex, parse, Host};

fn checked(source: &str) -> Result<khora_script::vm::Program, String> {
    let lexed = lex(source);
    assert!(lexed.diagnostics.is_empty(), "{:?}", lexed.diagnostics);
    let parsed = parse(lexed.tokens);
    assert!(parsed.diagnostics.is_empty(), "{:?}", parsed.diagnostics);
    let result = check(&parsed.module);
    if result.has_errors() {
        return Err(format!("{:?}", result.diagnostics));
    }
    let compiled = compile(&parsed.module);
    assert!(!compiled.has_errors(), "{:?}", compiled.diagnostics);
    Ok(compiled.program)
}

fn run_g(source: &str) -> Value {
    let program = checked(source).expect("accepted");
    let mut machine = Machine::new(&program, "G", &[]).expect("`G` exists");
    match machine.run(&program, &mut Host::new(), u64::MAX) {
        Run::Completed => machine.result(),
        other => panic!("`G` ran to {other:?}"),
    }
}

#[test]
fn a_default_building_itself_through_an_array_is_refused() {
    let result = checked(
        "struct A { int v; A[] xs = [A { v: 1 }]; }
         fn int G() { A a = A { v: 2 }; return a.v; }",
    );
    assert!(result.is_err(), "accepted");
}

#[test]
fn a_cycle_in_a_struct_never_built_is_refused() {
    let result = checked(
        "struct A { int v; A? next = A { v: 1 }; }
         fn int G() { return 1; }",
    );
    assert!(result.is_err(), "accepted");
}

#[test]
fn a_self_reference_ended_by_null_is_accepted() {
    let value = run_g(
        "struct A { int v; A? next = A { v: 1, next: null }; }
         fn int G() { A a = A { v: 2 }; return a.v * 10 + (a.next?.v ?? 0); }",
    );
    assert_eq!(value, Value::Int(21));
}
