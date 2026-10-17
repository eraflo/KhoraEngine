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

//! What an interpolation contributes to its function's fingerprint and to its
//! statement's key.
//!
//! The text between the holes and the way each hole is shown are part of the
//! code: changing either changes the function's fingerprint, so a frame
//! suspended in it is rebuilt rather than kept. Where the program happens to
//! store that description is not: another function's interpolation leaves
//! this one's fingerprint alone.

use khora_script::Program;

use super::{build, function};

fn fingerprint_of_f(source: &str) -> u64 {
    function(&build(source), "F").fingerprint
}

/// Every site name of `F`, in program-counter order.
fn sites_of_f(program: &Program) -> Vec<String> {
    function(program, "F")
        .sites
        .iter()
        .map(|site| site.name.clone())
        .collect()
}

#[test]
fn two_interpolations_differing_only_by_a_segment_fingerprint_apart() {
    let b = fingerprint_of_f(r#"fn string F(int x) { return $"a{x}b"; }"#);
    let c = fingerprint_of_f(r#"fn string F(int x) { return $"a{x}c"; }"#);
    assert_ne!(b, c);
}

#[test]
fn the_same_interpolation_fingerprints_alike() {
    let source = r#"fn string F(int x) { return $"a{x}b"; }"#;
    assert_eq!(fingerprint_of_f(source), fingerprint_of_f(source));
}

/// A `Duration` and a `float` are the same register at run time; the unit the
/// hole shows is the difference, and it is in the fingerprint.
#[test]
fn two_interpolations_differing_only_by_how_a_hole_is_shown_fingerprint_apart() {
    let float = fingerprint_of_f(r#"fn string F(float x) { return $"t {x}"; }"#);
    let duration = fingerprint_of_f(r#"fn string F(Duration x) { return $"t {x}"; }"#);
    assert_ne!(float, duration);
}

/// Another function's interpolation, declared first, shifts where this one's
/// description is stored — and nothing about this one's code.
#[test]
fn another_functions_interpolation_leaves_the_fingerprint_alone() {
    let f = r#"fn string F(int x) { return $"a{x}b"; }"#;
    let alone = fingerprint_of_f(f);
    let after = fingerprint_of_f(&format!(
        "fn string G(int y) {{ return $\"g{{y}}g{{y}}\"; }}\n{f}"
    ));
    assert_eq!(alone, after);
}

/// A statement holding `$"…"` is keyed by what it writes: its segments and its
/// holes, in order.
#[test]
fn an_interpolations_segments_and_holes_are_part_of_its_statement_key() {
    let keyed = |interpolation: &str| {
        sites_of_f(&build(&format!(
            "fn string F(int a, int b) {{ string s = {interpolation}; return s; }}"
        )))
    };
    let base = keyed(r#"$"a{a}b{b}""#);
    assert_ne!(base, keyed(r#"$"a{a}c{b}""#), "a segment changed");
    assert_ne!(base, keyed(r#"$"a{b}b{a}""#), "the holes swapped");
    assert_eq!(base, keyed(r#"$"a{a}b{b}""#), "the same statement");
}
