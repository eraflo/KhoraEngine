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

//! `xs?.Push(v)` and `xs?.RemoveAt(i)` on an optional array.
//!
//! The checker types a method called through `?.` on `T[]?` as it types one
//! called through `.` on `T[]`. Whatever it accepts the compiler must
//! compile: the call changes the array when there is one, and does nothing
//! when there is none.

use khora_script::vm::{Machine, Program, Run, Value};
use khora_script::{check, compile, lex, parse, Diagnostic, Host};

/// The checker's and the compiler's diagnostics for `source`.
fn diagnostics(source: &str) -> (Vec<String>, Vec<String>, Program) {
    let lexed = lex(source);
    assert!(lexed.diagnostics.is_empty(), "{:?}", lexed.diagnostics);
    let parsed = parse(lexed.tokens);
    assert!(parsed.diagnostics.is_empty(), "{:?}", parsed.diagnostics);
    let checked = check(&parsed.module);
    let compiled = compile(&parsed.module);
    let messages = |list: &[Diagnostic]| {
        list.iter()
            .map(|diagnostic| diagnostic.message.clone())
            .collect::<Vec<_>>()
    };
    (
        messages(&checked.diagnostics),
        messages(&compiled.diagnostics),
        compiled.program,
    )
}

fn run(program: &Program, function: &str) -> Value {
    let mut machine = Machine::new(program, function, &[]).expect("the function exists");
    match machine.run(program, &mut Host::new(), u64::MAX) {
        Run::Completed => machine.result(),
        other => panic!("`{function}`: {other:?}"),
    }
}

/// **A method called through `?.` that the checker accepts compiles, and
/// runs on the array when there is one.** `ys?.Push(2)` on `[1]` makes two
/// elements and `ys?.RemoveAt(0)` takes one back; on `null`, both do nothing.
#[test]
fn push_and_remove_at_through_a_question_dot_compile_when_the_checker_accepts_them() {
    let source = "fn int Present() {
                      int[]? ys = [1];
                      ys?.Push(2);
                      ys?.Push(3);
                      ys?.RemoveAt(0);
                      int[] zs = ys ?? [];
                      return zs.Length * 10 + zs[0];
                  }
                  fn int Absent() {
                      int[]? xs = null;
                      xs?.Push(1);
                      xs?.RemoveAt(0);
                      int[] zs = xs ?? [7, 7, 7];
                      return zs.Length;
                  }";
    let (checker, compiler, program) = diagnostics(source);
    assert!(
        checker.is_empty(),
        "the premise: the checker accepts `?.Push` and `?.RemoveAt` on `int[]?`: {checker:?}"
    );
    assert!(
        compiler.is_empty(),
        "the checker accepted the program, the compiler refused it: {compiler:?}"
    );
    assert_eq!(run(&program, "Present"), Value::Int(22));
    assert_eq!(run(&program, "Absent"), Value::Int(3));
}
