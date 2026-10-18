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

//! `e.SetMaterial(m)`: an entity given a material, by its asset identity.

use super::*;

/// The material's identity.
fn stone() -> AssetUUID {
    AssetUUID::from_bytes([0x5a; 16])
}

/// **`e.SetMaterial(m)` queues the material for the entity.**
#[test]
fn set_material_queues_the_material_for_the_entity() {
    let program = build("fn void Paint(Entity e, Material m) { e.SetMaterial(m); }");

    let (_, commands) = run(
        &program,
        "Paint",
        &[Value::Entity(SUBJECT), Value::Asset(stone())],
    );

    assert_eq!(
        commands,
        vec![WorldCommand::SetMaterial {
            entity: SUBJECT,
            material: stone(),
        }]
    );
}

/// A `Material?` holding a material is taken as a `Material` is.
#[test]
fn set_material_takes_an_optional_holding_one() {
    let program = build("fn void Paint(Entity e, Material? m) { e.SetMaterial(m); }");

    let (_, commands) = run(
        &program,
        "Paint",
        &[Value::Entity(SUBJECT), Value::Asset(stone())],
    );

    assert_eq!(
        commands,
        vec![WorldCommand::SetMaterial {
            entity: SUBJECT,
            material: stone(),
        }]
    );
}

/// A behavior gives itself the material its field holds.
#[test]
fn a_behavior_sets_its_own_material() {
    build(
        "behavior Painted {
             Material? look;
             void Update(float dt) { this.SetMaterial(look); }
         }",
    );
}

/// **A `null` material faults** — "`m` is `null`" — and queues nothing.
#[test]
fn a_null_material_faults() {
    let program = build("fn void Paint(Entity e, Material? m) { e.SetMaterial(m); }");

    let ran = run_on(
        &program,
        "Paint",
        &[Value::Entity(SUBJECT), Value::Null],
        host(),
    );

    let fault = fault_of(&ran);
    assert!(fault.contains("`m` is `null`"), "{fault}");
    assert!(ran.commands.is_empty(), "{:?}", ran.commands);
}

/// **Another kind of asset is refused** where it is passed: a mesh is not a
/// material.
#[test]
fn another_asset_kind_is_refused() {
    build("fn void Paint(Entity e, Material? m) { e.SetMaterial(m); }");
    refused_at(
        "fn void Paint(Entity e, Mesh? m) { e.SetMaterial(m); }",
        "m)",
        &["`Material", "`Mesh?`"],
    );
}

/// **`SetMaterial` is the language's**, as `Set` is: a behavior's member
/// may not take the name, which `this.SetMaterial(…)` would never reach.
#[test]
fn a_member_named_set_material_is_refused() {
    refused_at(
        "behavior Painted {
             Material? look;
             void SetMaterial(Material? m) { look = m; }
             void Update(float dt) { this.SetMaterial(look); }
         }",
        "SetMaterial(Material? m)",
        &["SetMaterial"],
    );
}
