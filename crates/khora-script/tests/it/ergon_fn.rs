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

//! `#[ergon_fn]` expanded in a crate that is not `khora_script`.
//!
//! `khora-macros` is a proc-macro crate and cannot test its own output, and the
//! unit tests inside `khora_script` resolve `::khora_script::…` through
//! `extern crate self`, which hides a broken path. Here the attribute is used
//! the way a game uses it: from another crate, through `khora_script::ergon_fn`.
//! Its expansion names `::khora_script::native::{NativeFn, NativeRegistration,
//! ScriptType}`, `::khora_script::vm::Value::Unit` and
//! `::khora_script::inventory::submit!`; a move that drops one of them stops
//! this module compiling, and a submission that no longer reaches the registry
//! fails the assertions.
//!
//! The natives are named `guard_probe_*`: `inventory` shares one collection
//! across the whole test binary, so a plain name would be reserved for every
//! other test's scripts.

use khora_script::native::{Host, NativeContext, NativeError, NativeRegistry, NativeTy};
use khora_script::vm::{Machine, Run, Value};
use khora_script::{check, compile, ergon_fn, lex, parse};

/// A pure native: one argument, one result, the default name and cost.
#[ergon_fn]
fn guard_probe_triple(x: f32) -> f32 {
    x * 3.0
}

/// A native that can refuse: the macro propagates the `Err` with `?`.
#[ergon_fn]
fn guard_probe_checked(x: f32) -> Result<f32, NativeError> {
    if x < 0.0 {
        Err(NativeError::new("negative"))
    } else {
        Ok(x)
    }
}

/// A native that takes the context, under an explicit name and cost.
#[ergon_fn(name = "GuardProbeNamed", cost = 7)]
fn guard_probe_with_context(context: &mut NativeContext<'_>, flag: bool) -> bool {
    flag && context.entity.is_none()
}

/// Compiles `source` against the registry the engine runs on, insisting it is
/// well-formed.
fn build(source: &str) -> khora_script::Program {
    let lexed = lex(source);
    assert!(lexed.diagnostics.is_empty(), "{:?}", lexed.diagnostics);
    let parsed = parse(lexed.tokens);
    assert!(parsed.diagnostics.is_empty(), "{:?}", parsed.diagnostics);
    let checked = check(&parsed.module);
    assert!(checked.diagnostics.is_empty(), "{:?}", checked.diagnostics);
    let compiled = compile(&parsed.module);
    assert!(
        compiled.diagnostics.is_empty(),
        "{:?}",
        compiled.diagnostics
    );
    compiled.program
}

#[test]
fn the_generated_declaration_describes_the_rust_signature() {
    assert_eq!(ERGON_GUARD_PROBE_TRIPLE.name, "GuardProbeTriple");
    assert_eq!(ERGON_GUARD_PROBE_TRIPLE.params, &[NativeTy::Float]);
    assert_eq!(ERGON_GUARD_PROBE_TRIPLE.result, NativeTy::Float);
    assert_eq!(ERGON_GUARD_PROBE_TRIPLE.cost, 1);
    assert!(!ERGON_GUARD_PROBE_TRIPLE.variadic);

    assert_eq!(ERGON_GUARD_PROBE_WITH_CONTEXT.name, "GuardProbeNamed");
    assert_eq!(
        ERGON_GUARD_PROBE_WITH_CONTEXT.params,
        &[NativeTy::Bool],
        "the context is not an argument"
    );
    assert_eq!(ERGON_GUARD_PROBE_WITH_CONTEXT.result, NativeTy::Bool);
    assert_eq!(ERGON_GUARD_PROBE_WITH_CONTEXT.cost, 7);
}

#[test]
fn a_native_annotated_in_another_crate_is_discovered() {
    let natives = NativeRegistry::discovered();
    for name in ["GuardProbeTriple", "GuardProbeChecked", "GuardProbeNamed"] {
        let found = natives
            .get(name)
            .unwrap_or_else(|| panic!("`{name}` was not collected from its inventory submission"));
        assert_eq!(found.name, name);
    }
    assert!(natives.get("GuardProbeWithContext").is_none());
}

#[test]
fn the_trampoline_unpacks_arguments_and_propagates_errors() {
    let mut host = Host::new();
    let strings: Vec<String> = Vec::new();

    let mut context = host.context(&strings);
    let tripled = (ERGON_GUARD_PROBE_TRIPLE.call)(&mut context, &[Value::Float(2.0)])
        .expect("a float argument is accepted");
    assert_eq!(tripled.as_float(), Some(6.0));

    let refused = (ERGON_GUARD_PROBE_CHECKED.call)(&mut context, &[Value::Float(-1.0)]);
    assert_eq!(
        refused.expect_err("`Err` is propagated").message,
        "negative"
    );

    let accepted = (ERGON_GUARD_PROBE_CHECKED.call)(&mut context, &[Value::Float(4.0)])
        .expect("`Ok` is unwrapped");
    assert_eq!(accepted.as_float(), Some(4.0));

    let answered = (ERGON_GUARD_PROBE_WITH_CONTEXT.call)(&mut context, &[Value::Bool(true)])
        .expect("the context is forwarded");
    assert_eq!(answered.as_bool(), Some(true));
}

#[test]
fn a_native_annotated_in_another_crate_is_callable_from_a_script() {
    let program = build("fn float Main() { return GuardProbeTriple(7.0); }");
    let mut host = Host::new();
    let mut machine = Machine::new(&program, "Main", &[]).expect("Main exists");
    assert_eq!(machine.run(&program, &mut host, u64::MAX), Run::Completed);
    assert_eq!(machine.result().as_float(), Some(21.0));
}
