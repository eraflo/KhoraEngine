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

//! String interpolation: `$"hp: {h.current} / {h.max}"`.
//!
//! `$"` opens a string whose `{ … }` holes each hold one expression, shown as
//! text. `{{` and `}}` write a brace; a lone `}` and an empty `{}` are
//! refused. A hole may hold strings, nested calls and another `$"…"`; a
//! ternary in a hole is written in parentheses.
//!
//! A hole shows a number, a duration, an angle, a flag, a string, an engine
//! value, an entity, or an optional of one of these — `null` when empty. The
//! format is chosen from the hole's type when the script compiles: a
//! `Duration` is a float at run time, and only its type says to show `s`.
//! Arrays, structs and maps are refused.
//!
//! The holes are evaluated left to right, and the text is built in one
//! allocation of the frame arena, whatever the number of holes. A string with
//! no hole is a plain string literal.

mod diagnostics;
mod display;
mod escapes;
mod evaluation;
mod fingerprints;
mod refusals;
mod suspension;

use khora_script::vm::{Machine, Program, Run, Value};
use khora_script::{compile_module, CompileOutcome, Diagnostic, Function, Host, MemoryLoader};

fn compile(source: &str) -> CompileOutcome {
    compile_module(&MemoryLoader::new().with("main.erg", source), "main.erg")
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

/// What a diagnostic underlines in `source`.
fn underlined<'s>(source: &'s str, diagnostic: &Diagnostic) -> &'s str {
    source
        .get(diagnostic.span.start as usize..diagnostic.span.end as usize)
        .unwrap_or("<out of the file>")
}

/// Every error of `result`, with what it underlines in `source`.
fn listed(source: &str, result: &CompileOutcome) -> Vec<String> {
    errors(result)
        .into_iter()
        .map(|d| format!("{} @ {:?}", said(d), underlined(source, d)))
        .collect()
}

/// `source`, compiled; panics naming every error when it does not compile.
fn build(source: &str) -> Program {
    let result = compile(source);
    let found = listed(source, &result);
    assert!(found.is_empty(), "should compile:\n{source}\n{found:?}");
    result.program.expect("a program with no errors")
}

/// Whether `text` holds `fragment`; a fragment may list `|`-separated
/// alternatives, one of which must be there.
fn holds(text: &str, fragment: &str) -> bool {
    fragment
        .split('|')
        .any(|alternative| text.contains(alternative))
}

/// The refusal of `source` saying every one of `fragments` and underlining
/// something inside the first `place` written in it; panics when it compiles
/// or when no error does both.
fn refused_at(source: &str, place: &str, fragments: &[&str]) -> Diagnostic {
    let result = compile(source);
    assert!(
        !result.succeeded(),
        "should be refused, and compiled:\n{source}"
    );
    let start = source
        .find(place)
        .unwrap_or_else(|| panic!("{place:?} is written in\n{source}"));
    let end = start + place.len();
    errors(&result)
        .into_iter()
        .find(|d| {
            let text = said(d);
            fragments.iter().all(|fragment| holds(&text, fragment))
                && d.span.start as usize >= start
                && d.span.end as usize <= end
                && d.span.start < d.span.end
        })
        .cloned()
        .unwrap_or_else(|| {
            panic!(
                "expected a refusal of\n{source}\nsaying {fragments:?} at `{place}`; got {:?}",
                listed(source, &result)
            )
        })
}

/// Runs `machine` to its end in `host`, an `await` treated as elapsed at once.
fn finish(mut machine: Machine, program: &Program, host: &mut Host) -> Value {
    for _ in 0..1000 {
        match machine.run(program, host, u64::MAX) {
            Run::Completed => return machine.result(),
            Run::Suspended(_) => host.awaiting = None,
            Run::Faulted(fault) => panic!("faulted: {fault:?}"),
        }
    }
    panic!("still running after 1000 resumes");
}

/// What the free function `name` returns, called with `args`.
fn run_fn(program: &Program, name: &str, args: &[Value]) -> Value {
    let machine = Machine::new(program, name, args)
        .unwrap_or_else(|| panic!("`{name}` is a function taking {} argument(s)", args.len()));
    finish(machine, program, &mut Host::new())
}

/// The text the function `name` returns, called with `args`, and the number
/// of objects its run left in the frame arena.
fn text_and_arena(program: &Program, name: &str, args: &[Value]) -> (String, usize) {
    let mut machine = Machine::new(program, name, args)
        .unwrap_or_else(|| panic!("`{name}` is a function taking {} argument(s)", args.len()));
    let mut host = Host::new();
    for _ in 0..1000 {
        match machine.run(program, &mut host, u64::MAX) {
            Run::Completed => {
                let text = machine
                    .resolve_str(machine.result(), program, &host)
                    .unwrap_or_else(|fault| panic!("`{name}` returned no text: {fault:?}"))
                    .to_owned();
                return (text, host.arena.len());
            }
            Run::Suspended(_) => host.awaiting = None,
            Run::Faulted(fault) => panic!("`{name}` faulted: {fault:?}"),
        }
    }
    panic!("`{name}` still running after 1000 resumes");
}

/// The text the function `name` returns, called with `args`.
fn text_of(program: &Program, name: &str, args: &[Value]) -> String {
    text_and_arena(program, name, args).0
}

/// The text `F` returns, `F` being `fn string F() { <body> }` alongside
/// `declarations`.
fn shown(declarations: &str, body: &str) -> String {
    let source = format!("{declarations}\nfn string F() {{ {body} }}");
    text_of(&build(&source), "F", &[])
}

fn function<'p>(program: &'p Program, name: &str) -> &'p Function {
    program
        .function(name)
        .unwrap_or_else(|| panic!("`{name}` is a function of the program"))
}
