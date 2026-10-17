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

//! Where a field attribute may be written: before a field the inspector
//! shows. A struct's field and a state's field parse one and are refused;
//! one written before anything else in a behavior is a parse error.

use super::{refused_at, refused_saying};

/// The attributes a field takes, each well-formed on an `int` field.
const WELL_FORMED: [&str; 3] = [
    "[Range(0, 10)]",
    "[Tooltip(\"What it is worth\")]",
    "[Header(\"Value\")]",
];

/// **A struct's field takes no attribute** — not even a well-formed one, nor
/// an unknown one: only a component's, a behavior's or a system's fields
/// show in the inspector. The refusal sits on the attribute and says to put
/// it on the field holding the struct.
#[test]
fn an_attribute_on_a_struct_field_is_refused() {
    for attribute in WELL_FORMED.into_iter().chain(["[Foo]"]) {
        for source in [
            format!("struct Loot {{ {attribute} int value = 1; }}"),
            format!(
                "struct Loot {{ {attribute} int value = 1; }}
                 component Chest {{ Loot loot = Loot {{ value: 1 }}; }}"
            ),
            format!(
                "struct Loot {{ string item = \"\"; {attribute} int value = 1; }}
                 behavior Guard {{ Loot loot; void Update(float dt) {{ }} }}"
            ),
        ] {
            refused_at(
                &source,
                attribute,
                &[
                    "a struct field takes no attribute",
                    "the field holding the struct",
                ],
            );
        }
    }
}

/// **A state's field takes no attribute**: its data is the engine's to keep,
/// shown read-only while playing. The refusal sits on the attribute.
#[test]
fn an_attribute_on_a_state_field_is_refused() {
    for attribute in WELL_FORMED.into_iter().chain(["[Foo]"]) {
        for source in [
            format!(
                "behavior Guard {{
                     int hp = 1;
                     state Alert {{ {attribute} int seen = 0; }}
                 }}"
            ),
            format!(
                "behavior Guard {{
                     int hp = 1;
                     state Patrol {{ int step = 0; }}
                     state Alert {{ int seen = 0; {attribute} int missed = 0; }}
                 }}"
            ),
        ] {
            refused_at(
                &source,
                attribute,
                &[
                    "a state's field takes no attribute",
                    "read-only while playing",
                ],
            );
        }
    }
}

/// **Inside a behavior, attributes apply to its fields**: written before a
/// method, an `async` method, a handler, an `every`, an `after` or a
/// `state`, they are a parse error saying so, the note listing the three a
/// field takes.
#[test]
fn an_attribute_before_a_behavior_member_that_is_not_a_field_is_a_parse_error() {
    for member in [
        "void Update(float dt) { }",
        "async void Patrol() { await 1s; }",
        "on Hit(int amount, Entity source) { }",
        "every 0.5s { hp += 1; }",
        "after 10s { hp = 0; }",
        "state Alert { int seen = 0; }",
    ] {
        for attribute in ["[Tooltip(\"Tick\")]", "[Range(0, 1)]", "[Critical]"] {
            let source = format!(
                "event Hit(int amount, Entity source);
                 behavior Guard {{
                     int hp = 1;
                     {attribute} {member}
                 }}"
            );
            refused_saying(
                &source,
                &[
                    "attributes inside a behavior apply to its fields",
                    "Range",
                    "Tooltip",
                    "Header",
                ],
            );
        }
    }
}

/// **A system's members are held to the same rule**: an attribute before its
/// `Update` is a parse error, as in a behavior.
#[test]
fn an_attribute_before_a_system_method_is_a_parse_error() {
    refused_saying(
        "system Sweeper {
             int count = 0;
             [Header(\"Tick\")] void Update(float dt) { count += 1; }
         }",
        &["attributes inside a behavior apply to its fields"],
    );
}
