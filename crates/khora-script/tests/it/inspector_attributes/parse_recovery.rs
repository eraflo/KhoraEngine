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

//! A malformed or misplaced field attribute does not hide what follows it:
//! the parser reports it and goes on to the next member.

use super::{compile, errors, refused_saying, said};

/// **A broken attribute is one error among the others**: an unclosed
/// attribute on one field and another on a later field are both reported,
/// in a component and in a behavior.
#[test]
fn a_broken_attribute_does_not_hide_the_next_one() {
    for source in [
        "component Stats {
             [Range(0, 10] int a = 1;
             int b = 2;
             [Tooltip(\"x\") int c = 3;
         }",
        "behavior Guard {
             [Range(0, 10] int a = 1;
             int b = 2;
             [Tooltip(\"x\") int c = 3;
             void Update(float dt) { b += 1; }
         }",
    ] {
        let result = compile(source);
        let found: Vec<String> = errors(&result).into_iter().map(said).collect();
        assert!(
            found.len() >= 2,
            "both broken attributes are reported in\n{source}\ngot {found:?}"
        );
    }
}

/// **Inside a state, attributes before a method are a parse error too**, and
/// the state's other members still parse: the field after it is checked.
#[test]
fn an_attribute_before_a_state_method_is_a_parse_error() {
    refused_saying(
        "behavior Guard {
             int hp = 1;
             state Alert {
                 [Range(0, 10)] void Enter() { }
                 int seen = 0;
             }
         }",
        &["attributes inside a behavior apply to its fields"],
    );
}
