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

//! One name per component, whoever took it first.
//!
//! A name is held by a Rust component or by a declared one. Every Rust
//! component's registration and its vtable agree on it, and a refusal over a
//! name says who holds it.

use khora_core::script::ScriptValue;

use super::runtime_components::{declare, field};
use crate::ecs::{Component, ComponentKey, FieldKind, SemanticDomain, World};

/// A Rust component registered after a declared one took its name.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Spin(f32);
impl Component for Spin {}

/// The registration a scene knows a component by and the vtable the world
/// stores it under carry the same short name and provenance.
#[test]
fn every_registration_is_named_as_its_vtable() {
    let world = World::new();
    for reg in inventory::iter::<crate::scene::ComponentRegistration> {
        let Some(vtable) = world.components().vtable(ComponentKey::Rust(reg.type_id)) else {
            continue;
        };
        assert_eq!(&*vtable.name, reg.type_name);
        assert_eq!(
            world.components().key_named(reg.type_name),
            Some(vtable.key),
            "{}",
            reg.type_name
        );
        assert_eq!(vtable.provenance, reg.provenance, "{}", reg.type_name);
    }
}

/// A Rust type whose short name a declared component already holds is
/// refused — and the refusal says a declared component holds it, not a Rust
/// one: a diagnostic that blames a Rust component that does not exist sends
/// the author looking for the wrong type.
#[test]
fn a_rust_component_named_like_a_declared_one_is_refused_for_that_reason() {
    let mut world = World::new();
    let declared = declare(
        &mut world,
        "Spin",
        SemanticDomain::Script,
        vec![field("n", FieldKind::Int, ScriptValue::Int(0))],
    );

    let error = world
        .try_register_component::<Spin>(SemanticDomain::Spatial)
        .expect_err("the name is taken");
    let message = error.to_string();
    assert!(
        !message.contains("is the name of a Rust component"),
        "refused as {error:?}, which reads: {message}"
    );
    assert_eq!(world.components().key_named("Spin"), Some(declared));
    assert!(world
        .components()
        .vtable(ComponentKey::of::<Spin>())
        .is_none());
}
