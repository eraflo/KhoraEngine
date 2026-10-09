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

//! A struct field's default that builds the struct it belongs to.
//!
//! `struct A { A? next = A { v: 1 }; }` is a constant default the checker
//! accepts: an `A` literal of constants. But every `A` built without `next`
//! takes that default, which is an `A` built without `next` — a default that
//! names its own struct, directly or through another one, describes a value
//! with no end. The checker must refuse it, or the build must give it a
//! finite meaning; neither may recurse until the stack runs out.

use khora_script::vm::{Machine, Run, Value};
use khora_script::{check, compile, lex, parse, Host};

/// Checks and compiles `source`: `Err` with the checker's errors when it
/// refuses it, else what `G()` returns.
fn outcome(source: &str) -> Result<Value, String> {
    let lexed = lex(source);
    assert!(lexed.diagnostics.is_empty(), "{:?}", lexed.diagnostics);
    let parsed = parse(lexed.tokens);
    assert!(parsed.diagnostics.is_empty(), "{:?}", parsed.diagnostics);
    let checked = check(&parsed.module);
    if checked.has_errors() {
        return Err(format!("{:?}", checked.diagnostics));
    }
    let compiled = compile(&parsed.module);
    if compiled.has_errors() {
        return Err(format!("{:?}", compiled.diagnostics));
    }
    let program = compiled.program;
    let mut machine = Machine::new(&program, "G", &[]).expect("`G` exists");
    match machine.run(&program, &mut Host::new(), u64::MAX) {
        Run::Completed => Ok(machine.result()),
        other => panic!("`G` ran to {other:?}"),
    }
}

/// **A default naming its own struct does not recurse forever** — directly,
/// or through a second struct whose default names the first. Refused by the
/// checker, or built: either is an answer; a stack overflow is not.
#[test]
fn a_struct_default_naming_its_own_struct_does_not_recurse_forever() {
    for source in [
        "struct A { int v; A? next = A { v: 1 }; }
         fn int G() { A a = A { v: 2 }; return a.v; }",
        "struct A { int v; B? b = B { }; }
         struct B { A? a = A { v: 1 }; }
         fn int G() { A a = A { v: 2 }; return a.v; }",
    ] {
        let result = outcome(source);
        assert!(
            matches!(result, Err(_) | Ok(Value::Int(2))),
            "`{source}`: {result:?}"
        );
    }
}
