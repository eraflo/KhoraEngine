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

//! Adding a value kind moves no fingerprint of a program that holds none.
//!
//! A fingerprint is what tells a suspended frame its function is unchanged
//! after a reload. If adding `Asset` shifted the encoding of a value, a type
//! or a shown hole that existed before it, every frame suspended in every
//! project would be rebuilt — or refused — on the first reload after the
//! upgrade. So the fingerprints of programs touching what an asset sits
//! beside — entities, optionals, `null`, equality, interpolation, behavior
//! fields — are pinned at the values they had before assets existed.

use khora_script::vm::Program;

use super::build;

/// Each program, and the fingerprint of each of its functions, as computed
/// before the asset kind existed.
const PINNED: &[(&str, &[(&str, u64)])] = &[
    (
        "fn bool Near(Entity? a, Entity? b) { return a == b || a == null; }",
        &[("Near", 0x2b76_c459_d87a_b36a)],
    ),
    (
        r#"fn string Show(Entity e, int? n, Duration d, Vec3 v, bool b, string s) {
               return $"{e} {n} {d} {v} {b} {s}";
           }"#,
        &[("Show", 0x5021_14bc_4567_bb0f)],
    ),
    (
        "fn int? Pick(int? a, int b) { int? c = a; if (c == null) { c = b; } return c; }",
        &[("Pick", 0xc7a7_0ea8_4dbe_3794)],
    ),
    (
        "struct Loot { int value = 3; Entity? owner; }
         behavior Guard {
             Entity? target;
             Vec3 heading;
             Loot loot;
             bool Hunting() { return target != null; }
             int Worth() { return loot.value; }
         }",
        &[
            ("Guard.Hunting", 0x7531_004e_60dc_5538),
            ("Guard.Worth", 0x6539_0248_c9a6_8d72),
        ],
    ),
];

/// The program fingerprint of each program in [`PINNED`], in order, as
/// computed before the asset kind existed.
const PINNED_PROGRAMS: [u64; 4] = [
    0xb60d_d3a4_e0d6_3571,
    0x0b40_7b09_914f_f1cc,
    0x25e5_5b84_e159_144d,
    0x80ef_4c74_24df_2ccb,
];

fn fingerprint_of(program: &Program, name: &str) -> u64 {
    program
        .function(name)
        .unwrap_or_else(|| panic!("`{name}` is a function of the program"))
        .fingerprint
}

/// **The functions of a program holding no asset fingerprint as they did
/// before assets existed.**
#[test]
fn a_program_without_assets_keeps_its_function_fingerprints() {
    let mut moved = Vec::new();
    for (source, functions) in PINNED {
        let program = build(source);
        for (name, pinned) in *functions {
            let now = fingerprint_of(&program, name);
            if now != *pinned {
                moved.push(format!("`{name}`: pinned {pinned:#018x}, now {now:#018x}"));
            }
        }
    }
    assert!(moved.is_empty(), "{}", moved.join("\n"));
}

/// **The program fingerprint of a program holding no asset is the one it
/// had before assets existed.**
#[test]
fn a_program_without_assets_keeps_its_program_fingerprint() {
    let moved: Vec<String> = PINNED
        .iter()
        .zip(PINNED_PROGRAMS)
        .enumerate()
        .filter_map(|(at, ((source, _), pinned))| {
            let now = build(source).fingerprint();
            (now != pinned)
                .then(|| format!("program #{at}: pinned {pinned:#018x}, now {now:#018x}"))
        })
        .collect();
    assert!(moved.is_empty(), "{}", moved.join("\n"));
}
