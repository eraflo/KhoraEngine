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

//! A system has no subject, and no free function lends it one.
//!
//! A free function called from a behavior reads the behavior's entity
//! through `Position()`; called from a system it would read nothing. The
//! checker refuses the call — where the system makes it — when the function
//! reads the subject itself or through any chain of free functions it calls.
//! The same function stays callable from a behavior, and a free function that
//! never reads the subject stays callable from a system, recursion included.

use khora_script::{
    compile_module, CompileOutcome, Diagnostic, MemoryLoader, ENGINE_COMPONENTS_MODULE,
};

/// The engine module, as the mirror would serve it.
const ENGINE: &str = "component Health { int current; int max; }";

fn source_of(main: &str) -> String {
    format!("import \"{ENGINE_COMPONENTS_MODULE}\";\n{main}")
}

fn compile(main: &str) -> CompileOutcome {
    let loader = MemoryLoader::new()
        .with(ENGINE_COMPONENTS_MODULE, ENGINE)
        .with("main.erg", source_of(main));
    compile_module(&loader, "main.erg")
}

fn said(diagnostic: &Diagnostic) -> String {
    format!(
        "{} | {}",
        diagnostic.message,
        diagnostic.note.as_deref().unwrap_or_default()
    )
}

fn errors_of(result: &CompileOutcome) -> Vec<&Diagnostic> {
    result.diagnostics.iter().filter(|d| d.is_error()).collect()
}

fn underlined(main: &str, diagnostic: &Diagnostic) -> String {
    source_of(main)
        .get(diagnostic.span.start as usize..diagnostic.span.end as usize)
        .unwrap_or("<out of the file>")
        .to_owned()
}

fn compiles(main: &str) {
    let result = compile(main);
    let errors: Vec<String> = errors_of(&result).into_iter().map(said).collect();
    assert!(errors.is_empty(), "should compile:\n{main}\n{errors:?}");
}

/// The refusal of `main` that says "system" and underlines `call`.
fn refused_at(main: &str, call: &str) -> Diagnostic {
    let result = compile(main);
    assert!(
        !result.succeeded(),
        "should be refused, and compiled:\n{main}"
    );
    errors_of(&result)
        .into_iter()
        .find(|d| said(d).contains("system") && underlined(main, d).contains(call))
        .cloned()
        .unwrap_or_else(|| {
            panic!(
                "expected a refusal saying `system` at `{call}`; got {:?}",
                errors_of(&result)
                    .into_iter()
                    .map(|d| format!("{} @ {:?}", said(d), underlined(main, d)))
                    .collect::<Vec<_>>()
            )
        })
}

/// A free function reading the subject, and one reading it through two
/// others.
const READERS: &str = "
fn Vec3 Where() { return Position(); }
fn Vec3 Inner() { return Position(); }
fn Vec3 Middle() { return Inner(); }
fn Vec3 Outer() { return Middle(); }
fn int Twice(int n) { return n * 2; }
";

/// `READERS`, then `system Sweeper { int count = 0; <members> }`.
fn sweeper(members: &str) -> String {
    format!("{READERS}\nsystem Sweeper {{ int count = 0; {members} }}")
}

/// **A system calling a free function that reads `Position()` is refused at
/// the call**, in `Update`, in a method and in a timer. The same functions
/// are a behavior's to call — directly or through the chain, each reads the
/// behavior's entity — and a system calling a function that reads no
/// subject compiles.
#[test]
fn a_free_function_reading_the_subject_is_refused_where_a_system_calls_it() {
    compiles(&format!(
        "{READERS}
         behavior Walker {{
             void Update(float dt) {{ var a = Where(); var b = Outer(); }}
         }}"
    ));
    compiles(&sweeper("void Update(float dt) { count = Twice(count); }"));
    for member in [
        "void Update(float dt) { var p = Where(); }",
        "int Probe() { var p = Where(); return 1; }",
        "every 1s { var p = Where(); }",
    ] {
        refused_at(&sweeper(member), "Where()");
    }
}

/// **Through any chain of free functions**: `Outer` calls `Middle` calls
/// `Inner`, which reads `Position()` — the system's call to `Outer` is
/// refused, and so is its call to `Middle`.
#[test]
fn a_chain_of_free_functions_reading_the_subject_is_refused_at_the_systems_call() {
    refused_at(
        &sweeper("void Update(float dt) { var p = Outer(); }"),
        "Outer()",
    );
    refused_at(
        &sweeper("void Update(float dt) { var p = Middle(); }"),
        "Middle()",
    );
}

/// **Recursion that never reads the subject is accepted** — the chain is
/// followed to its end without going round a cycle forever — while a cycle
/// that does read it somewhere is refused.
#[test]
fn recursive_free_functions_are_followed_without_looping() {
    let cycle = "
fn int Ping(int n) { if (n > 0) { return Pong(n - 1); } return 0; }
fn int Pong(int n) { return Ping(n); }
";
    compiles(&format!(
        "{cycle}
         system Bouncer {{ int last = 0; void Update(float dt) {{ last = Ping(3); }} }}"
    ));

    let reading = "
fn float Tick(int n) { if (n > 0) { return Tock(n - 1); } return Position().x; }
fn float Tock(int n) { return Tick(n); }
";
    refused_at(
        &format!(
            "{reading}
             system Bouncer {{ float last = 0.0; void Update(float dt) {{ last = Tick(3); }} }}"
        ),
        "Tick(3)",
    );
}
