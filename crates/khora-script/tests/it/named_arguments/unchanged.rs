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

//! What a call's names change in the compiled program, and what they do not.
//!
//! A call written without names compiles as it always has: the same
//! fingerprints, the same statement keys — so a save holding a body suspended
//! in code that names nothing resumes as before. A labelled call is keyed by
//! the labels it writes, in the order written; a default it leaves out is not
//! part of its key, but is part of its code.

use khora_script::vm::{SiteKind, Value};
use khora_script::Program;

use super::{build, function, run_fn, site_names, Instance};

// ─── A module without names ─────────────────────────────────────────────────

/// Positional calls of every kind of callee: a script function, nested; an
/// engine function; an engine constructor; a behavior method; a state method;
/// a raise; a `become`; an `await` between them.
const POSITIONAL: &str = "event Damaged(int amount, Entity source);
fn int Pair(int a, int b) { return a * 10 + b; }
fn int Nested() { int got = Pair(Pair(1, 2), 3); return got; }
fn float Mix() { return Lerp(0.0, 8.0, 0.25) + Clamp(15.0, 0.0, 10.0); }
fn float Along() { Vec3 v = Vec3(1.0, 2.0, 3.0); return v.y; }
behavior Guard {
    int health = 100;
    int Score(int laps, int bonus) { return laps * 10 + bonus; }
    async void Hurt(int by) {
        await 0.5s;
        health -= Score(by, 1);
        this.Raise(Damaged(by, this));
    }
    state Patrol {
        int Lap(int laps) { return laps; }
        void Spot() { int l = Lap(2); become Chase(l); }
    }
    state Chase(int prey) {
        int Prey() { return prey; }
    }
}";

/// What [`POSITIONAL`] compiles to, as a program.
///
/// Recorded before calls could carry names. A fingerprint is written into
/// saves, and a site name is how a frozen frame is found again: either moving
/// for code that names nothing would strand every body suspended in it.
const POSITIONAL_PROGRAM: u64 = 0xba44_42f3_cc00_4cc4;

/// Each function of [`POSITIONAL`]: its name, its fingerprint, and its site
/// names in program-counter order.
const POSITIONAL_FUNCTIONS: &[(&str, u64, &[&str])] = &[
    (
        "Along",
        0x4d0c_7de5_2c74_5c7b,
        &["entry", "let.7581ad0d", "return.6cc8e814"],
    ),
    (
        "Guard.Chase.Prey",
        0xd03b_a9f3_34d0_b4c6,
        &["entry", "return.bddfedde"],
    ),
    (
        "Guard.Hurt",
        0x2283_3a67_5a2e_e58f,
        &[
            "entry",
            "expr.27678505",
            "expr.27678505:await",
            "expr.8d4b2242",
            "expr.8d4b2242:call.Guard.Score",
            "expr.b95e8473",
        ],
    ),
    (
        "Guard.Patrol.Lap",
        0x22bb_70ff_6f45_0716,
        &["entry", "return.705a7d42"],
    ),
    (
        "Guard.Patrol.Spot",
        0xb947_1a1e_c04c_e640,
        &[
            "entry",
            "let.aaa17950",
            "let.aaa17950:call.Guard.Patrol.Lap",
            "become.5e2f0d20",
        ],
    ),
    (
        "Guard.Score",
        0x81b9_b6c6_8bd5_2bdd,
        &["entry", "return.40440a72"],
    ),
    ("Mix", 0x0b19_5763_f883_91cf, &["entry", "return.9d7a581f"]),
    (
        "Nested",
        0x3f82_013c_2244_64d9,
        &[
            "entry",
            "let.edc1c1c6",
            "let.edc1c1c6:call.Pair",
            "let.edc1c1c6:call.Pair#1",
            "return.a0c724b6",
        ],
    ),
    ("Pair", 0xd695_805b_8951_1a1b, &["entry", "return.39aab1dc"]),
];

/// **Nothing moves for a call without names.** The module's fingerprint, each
/// function's fingerprint and every site name are what they were before named
/// arguments existed.
#[test]
fn a_module_without_names_keeps_its_fingerprints_and_statement_keys() {
    let program = build(POSITIONAL);
    let mut actual: Vec<(String, u64, Vec<String>)> = program
        .functions
        .iter()
        .map(|f| {
            (
                f.name.clone(),
                f.fingerprint,
                f.sites.iter().map(|site| site.name.clone()).collect(),
            )
        })
        .collect();
    actual.sort();
    let mut expected: Vec<(String, u64, Vec<String>)> = POSITIONAL_FUNCTIONS
        .iter()
        .map(|(name, fingerprint, sites)| {
            (
                (*name).to_owned(),
                *fingerprint,
                sites.iter().map(|site| (*site).to_owned()).collect(),
            )
        })
        .collect();
    expected.sort();

    let table: String = actual
        .iter()
        .map(|(name, fingerprint, sites)| {
            format!("    ({name:?}, {fingerprint:#018x}, &{sites:?}),\n")
        })
        .collect();
    assert_eq!(
        (program.fingerprint(), &actual),
        (POSITIONAL_PROGRAM, &expected),
        "a module calling without names moved; it now compiles to {:#018x} and\n{table}",
        program.fingerprint()
    );
}

// ─── A labelled call ────────────────────────────────────────────────────────

/// `Guard` with an `async` `Hit(int a, int b)` that writes `a * 10 + b` to
/// `fired` once it has waited, called from `Go` as `call`.
fn hitting(call: &str) -> Program {
    build(&format!(
        "behavior Guard {{
             int fired = 0;
             async void Hit(int a, int b) {{ await 1.0s; fired = a * 10 + b; }}
             async void Go() {{ {call} }}
         }}"
    ))
}

/// The names of `Go`'s sites where a call returns.
fn call_sites(program: &Program) -> Vec<String> {
    function(program, "Guard.Go")
        .sites
        .iter()
        .filter(|site| matches!(site.kind, SiteKind::Return { .. }))
        .map(|site| site.name.clone())
        .collect()
}

/// **Same result, different key.** `Hit(1, 2)` and `Hit(a: 1, b: 2)` write
/// the same `fired`; the labelled statement is keyed by its labels, so its
/// call site is named apart from the positional one.
#[test]
fn a_positional_call_and_the_same_call_named_return_alike_and_are_keyed_apart() {
    let positional = hitting("Hit(1, 2);");
    let named = hitting("Hit(a: 1, b: 2);");

    for program in [&positional, &named] {
        let mut guard = Instance::of(program, "Guard");
        guard.call("Go");
        assert_eq!(guard.field("fired"), Value::Int(12));
    }
    let (positional, named) = (call_sites(&positional), call_sites(&named));
    assert_eq!(
        positional.len(),
        1,
        "the premise: one call site, {positional:?}"
    );
    assert_eq!(named.len(), 1, "one call site, {named:?}");
    assert_ne!(positional, named, "the labels are part of the key");
}

/// **Labels are keyed in the order written.** `Hit(a: 1, b: 2)` and `Hit(b:
/// 2, a: 1)` do the same, and are two statements.
#[test]
fn the_same_labels_written_in_another_order_are_keyed_apart() {
    let forwards = hitting("Hit(a: 1, b: 2);");
    let backwards = hitting("Hit(b: 2, a: 1);");

    for program in [&forwards, &backwards] {
        let mut guard = Instance::of(program, "Guard");
        guard.call("Go");
        assert_eq!(guard.field("fired"), Value::Int(12));
    }
    assert_ne!(call_sites(&forwards), call_sites(&backwards));
}

/// **A default is code, not key.** `F` calls `G(3)` and leaves out `s`. Whether
/// `G` has no `s`, an `s = 1` or an `s = 2`, `F`'s statement keys are what it
/// writes, and stay; a changed default changes `F`'s code, so its fingerprint
/// moves and a frame in it is rebuilt at its site.
#[test]
fn a_defaulted_parameter_left_out_is_in_the_callers_code_not_its_key() {
    let caller = "fn int F() { int got = G(3); return got; }";
    let without = build(&format!("fn int G(int x) {{ return x + 1; }}\n{caller}"));
    let one = build(&format!(
        "fn int G(int x, int s = 1) {{ return x + s; }}\n{caller}"
    ));
    let two = build(&format!(
        "fn int G(int x, int s = 2) {{ return x + s; }}\n{caller}"
    ));

    assert_eq!(run_fn(&one, "F", &[]), Value::Int(4));
    assert_eq!(run_fn(&two, "F", &[]), Value::Int(5));
    assert_eq!(
        site_names(&without, "F"),
        site_names(&one, "F"),
        "the default is not part of the caller's key"
    );
    assert_eq!(site_names(&one, "F"), site_names(&two, "F"));
    assert_ne!(
        function(&one, "F").fingerprint,
        function(&two, "F").fingerprint,
        "the default the caller loads is part of its code"
    );
}
