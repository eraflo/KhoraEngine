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

//! A field's default written with named arguments: `Vec3 aim = Vec3(z: 3.0,
//! x: 1.0, y: 2.0);`.
//!
//! A field's default is read as data — the store a behavior instance starts
//! with, the row a declared component starts with — not run as code. Read as
//! data, a constructor's arguments are still placed by their names.

use khora_core::math::Vec3;
use khora_core::script::ScriptValue;
use khora_script::vm::Value;

use super::{build, Instance};

/// **A behavior field.** An instance starts with `aim = (1, 2, 3)`, whatever
/// order the names are written in, and with a positional `x` before them.
#[test]
fn a_behavior_field_default_constructed_with_names_starts_at_the_value_named() {
    for default in ["Vec3(z: 3.0, x: 1.0, y: 2.0)", "Vec3(1.0, z: 3.0, y: 2.0)"] {
        let program = build(&format!(
            "behavior Turret {{
                 Vec3 aim = {default};
                 float Digits() {{ return aim.x * 100.0 + aim.y * 10.0 + aim.z; }}
             }}"
        ));

        let mut turret = Instance::of(&program, "Turret");
        assert_eq!(
            turret.call("Digits"),
            Value::Float(123.0),
            "`{default}` is x = 1, y = 2, z = 3"
        );
    }
}

/// **A declared component's field.** The default the program declares — what
/// the engine registers and a new row starts with — is `(1, 2, 3)`.
#[test]
fn a_component_field_default_constructed_with_names_is_declared_as_the_value_named() {
    let program = build("component Aim { Vec3 dir = Vec3(z: 3.0, x: 1.0, y: 2.0); }");

    let aim = program
        .declared
        .iter()
        .find(|declared| declared.name == "Aim")
        .expect("Aim is declared");
    assert_eq!(
        aim.fields[0].default,
        ScriptValue::Vec3(Vec3::new(1.0, 2.0, 3.0)),
        "`dir` is x = 1, y = 2, z = 3"
    );
}
