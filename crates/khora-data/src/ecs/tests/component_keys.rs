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

//! A component has one key and one name.
//!
//! Every component — a Rust type or one declared at run time — is identified to
//! storage by a `ComponentKey` that needs no `World`: a Rust component's is its
//! `TypeId`, a declared one's a stable hash of its name. So a page describes
//! itself, and a component has the same key in every world. The world's
//! registry holds one vtable per component, with the single short name
//! (`Transform`, `HandleComponent<Mesh>`) that scenes, the editor and scripts
//! all use. Two Rust types that would share a short name are refused at
//! start-up, naming both full paths.

use std::any::{type_name, TypeId};
use std::panic::{catch_unwind, AssertUnwindSafe};

use khora_core::asset::Material;
use khora_core::ecs::entity::EntityId;
use khora_core::renderer::api::gpu_scene::Mesh;
use khora_core::script::ScriptValue;

use super::runtime_components::{declare, field, patch, read};
use super::{Position, Velocity};
use crate::ecs::{
    Component, ComponentKey, ComponentPage, ComponentProvenance, FieldKind, GlobalTransform,
    HandleComponent, Parent, RegisterError, RustIdentity, SemanticDomain, Transform, World,
};
use crate::scene::{registration_named, registration_of, ComponentRegistration};

/// Two types with one short name, `Twin`, in two modules.
mod a {
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub struct Twin;
    impl crate::ecs::Component for Twin {}
}

mod b {
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub struct Twin;
    impl crate::ecs::Component for Twin {}
}

/// A component nobody else registers, to try in two domains.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Wanderer;
impl Component for Wanderer {}

/// The message of a caught panic, whichever way it was raised.
fn panic_message(payload: &(dyn std::any::Any + Send)) -> String {
    if let Some(text) = payload.downcast_ref::<String>() {
        text.clone()
    } else if let Some(text) = payload.downcast_ref::<&str>() {
        (*text).to_owned()
    } else {
        String::new()
    }
}

/// Checks that the Rust type `T` is registered under `short`, and that its
/// name leads to its key.
fn assert_named<T: 'static>(world: &World, short: &str, domain: SemanticDomain) {
    let registry = world.components();
    let key = ComponentKey::of::<T>();
    let vtable = registry
        .vtable(key)
        .unwrap_or_else(|| panic!("`{}` is registered", type_name::<T>()));
    assert_eq!(&*vtable.name, short, "the name of `{}`", type_name::<T>());
    assert_eq!(registry.key_named(short), Some(key), "`{short}` by name");
    assert_eq!(vtable.key, key);
    assert_eq!(vtable.domain, domain, "the domain of `{short}`");
    assert_eq!(
        vtable.rust,
        Some(RustIdentity {
            type_id: TypeId::of::<T>(),
            path: type_name::<T>(),
        }),
        "the Rust identity of `{short}`"
    );
    // The full path identifies the type; it is not a name.
    assert_eq!(registry.key_named(type_name::<T>()), None);
}

/// `entity`'s location in `domain`.
fn location(
    world: &World,
    entity: EntityId,
    domain: SemanticDomain,
) -> Option<crate::ecs::PageIndex> {
    let (id, metadata) = world.entities.get(entity.index as usize)?;
    if *id != entity {
        return None;
    }
    metadata.as_ref()?.locations.get(&domain).copied()
}

#[test]
fn a_component_has_one_short_name() {
    let mut world = World::new();
    world.register_component::<Position>(SemanticDomain::Spatial);

    // A derived component: its short name is the derive's.
    assert_named::<Transform>(&world, "Transform", SemanticDomain::Spatial);
    // A generic one: every path segment stripped, its parameters' included.
    assert_named::<HandleComponent<Mesh>>(&world, "HandleComponent<Mesh>", SemanticDomain::Render);
    assert_named::<HandleComponent<Box<dyn Material>>>(
        &world,
        "HandleComponent<Box<dyn Material>>",
        SemanticDomain::Render,
    );
    // A hand-written one registered by the caller.
    assert_named::<Position>(&world, "Position", SemanticDomain::Spatial);
}

#[test]
fn a_rust_component_takes_its_registered_provenance() {
    let world = World::new();
    let registry = world.components();
    let provenance = |key: ComponentKey| registry.vtable(key).expect("registered").provenance;

    assert_eq!(
        provenance(ComponentKey::of::<GlobalTransform>()),
        ComponentProvenance::Derived
    );
    assert_eq!(
        provenance(ComponentKey::of::<Parent>()),
        ComponentProvenance::ToolAuthored
    );
    assert_eq!(
        provenance(ComponentKey::of::<Transform>()),
        ComponentProvenance::Authored
    );
    // No `ComponentRegistration`: the default.
    assert_eq!(
        provenance(ComponentKey::of::<HandleComponent<Mesh>>()),
        ComponentProvenance::Authored
    );
}

#[test]
fn a_rust_key_is_its_type() {
    assert_eq!(
        ComponentKey::of::<Transform>(),
        ComponentKey::Rust(TypeId::of::<Transform>())
    );
    assert_ne!(
        ComponentKey::of::<Position>(),
        ComponentKey::of::<Velocity>()
    );

    let mut world = World::new();
    let registry = world.components();
    assert!(
        !registry.is_empty(),
        "`World::new` registers the engine's components"
    );
    assert_eq!(registry.iter().count(), registry.len());
    for vtable in registry.iter() {
        assert_eq!(
            registry.vtable(vtable.key).map(|found| found.key),
            Some(vtable.key),
            "`{}` is found under its key",
            vtable.name
        );
        if let Some(rust) = vtable.rust {
            assert_eq!(
                vtable.key,
                ComponentKey::Rust(rust.type_id),
                "`{}`",
                vtable.name
            );
        }
    }
    let before = registry.len();

    assert_eq!(
        world.try_register_component::<Position>(SemanticDomain::Spatial),
        Ok(ComponentKey::of::<Position>())
    );
    assert_eq!(
        world.try_register_component::<Velocity>(SemanticDomain::Spatial),
        Ok(ComponentKey::of::<Velocity>())
    );
    assert_eq!(world.components().len(), before + 2);
    for key in [
        ComponentKey::of::<Position>(),
        ComponentKey::of::<Velocity>(),
    ] {
        assert_eq!(
            world.components().vtable(key).map(|vtable| vtable.key),
            Some(key)
        );
    }

    // Registering a type again, in its domain, changes nothing.
    assert_eq!(
        world.try_register_component::<Position>(SemanticDomain::Spatial),
        Ok(ComponentKey::of::<Position>())
    );
    assert_eq!(world.components().len(), before + 2);
}

#[test]
fn a_declared_key_is_the_same_in_every_world() {
    let fields = || vec![field("n", FieldKind::Int, ScriptValue::Int(0))];

    let mut first = World::new();
    let alpha = declare(&mut first, "Alpha", SemanticDomain::Script, fields());
    let shared_in_first = declare(&mut first, "Shared", SemanticDomain::Script, fields());
    let beta = declare(&mut first, "Beta", SemanticDomain::Physics, fields());

    // Another world, other declarations, another order.
    let mut second = World::new();
    let gamma = declare(&mut second, "Gamma", SemanticDomain::Physics, fields());
    let delta = declare(&mut second, "Delta", SemanticDomain::Script, fields());
    let epsilon = declare(&mut second, "Epsilon", SemanticDomain::Script, fields());
    let shared_in_second = declare(&mut second, "Shared", SemanticDomain::Script, fields());

    assert_eq!(shared_in_first, shared_in_second);
    assert_eq!(shared_in_first, ComponentKey::named("Shared"));
    assert!(matches!(shared_in_first, ComponentKey::Declared(_)));
    assert_eq!(alpha, ComponentKey::named("Alpha"));
    assert_eq!(beta, ComponentKey::named("Beta"));
    assert_eq!(gamma, ComponentKey::named("Gamma"));
    assert_eq!(delta, ComponentKey::named("Delta"));
    assert_eq!(epsilon, ComponentKey::named("Epsilon"));
    let keys = [alpha, beta, gamma, delta, epsilon, shared_in_first];
    for (i, a) in keys.iter().enumerate() {
        for b in &keys[i + 1..] {
            assert_ne!(a, b, "two names, two keys");
        }
    }

    for world in [&first, &second] {
        let vtable = world
            .components()
            .vtable(shared_in_first)
            .expect("Shared is registered");
        assert_eq!(vtable.key, shared_in_first);
        assert_eq!(&*vtable.name, "Shared");
        assert_eq!(
            world.components().key_named("Shared"),
            Some(shared_in_first)
        );
    }
}

/// What a page alone says about row `row`: its `Position`, and whether it holds
/// a column of the declared `Ballast`.
fn read_page(page: &ComponentPage, row: usize) -> (Option<Position>, bool) {
    let position = page
        .columns
        .get(&ComponentKey::of::<Position>())
        .and_then(|column| column.as_any().downcast_ref::<Vec<Position>>())
        .map(|column| column[row]);
    let ballast = page
        .columns
        .get(&ComponentKey::named("Ballast"))
        .is_some_and(|column| column.len() == page.entities.len());
    (position, ballast)
}

#[test]
fn a_page_is_read_without_a_world() {
    let mut world = World::new();
    world.register_component::<Position>(SemanticDomain::Spatial);
    let ballast = declare(
        &mut world,
        "Ballast",
        SemanticDomain::Spatial,
        vec![field("mass", FieldKind::Float, ScriptValue::Float(1.0))],
    );
    let entity = world.spawn(Position(7));
    world
        .add_runtime_component(
            entity,
            ballast,
            &patch(&[("mass", ScriptValue::Float(3.0))]),
        )
        .expect("Ballast attaches");

    let at = location(&world, entity, SemanticDomain::Spatial).expect("a Spatial row");
    let page = &world.storage.pages[at.page_id as usize];
    assert_eq!(page.entities[at.row_index as usize], entity);
    assert_eq!(
        read_page(page, at.row_index as usize),
        (Some(Position(7)), true),
        "the page's columns are found by key alone"
    );
    assert!(
        page.keys.contains(&ComponentKey::of::<Position>()) && page.keys.contains(&ballast),
        "the page's signature names both: {:?}",
        page.keys
    );
    assert!(
        page.keys.windows(2).all(|pair| pair[0] < pair[1]),
        "the signature is sorted"
    );
    assert_eq!(page.keys.len(), page.columns.len());
    assert_eq!(
        read(&world, entity, ballast, "mass"),
        Some(ScriptValue::Float(3.0))
    );
}

#[test]
fn a_rust_component_registered_in_two_domains_is_refused() {
    let mut world = World::new();
    world
        .try_register_component::<Wanderer>(SemanticDomain::Spatial)
        .expect("Wanderer registers");

    match world.try_register_component::<Wanderer>(SemanticDomain::Physics) {
        Err(RegisterError::DomainConflict {
            existing,
            requested,
            ..
        }) => {
            assert_eq!(existing, SemanticDomain::Spatial);
            assert_eq!(requested, SemanticDomain::Physics);
        }
        other => panic!("expected DomainConflict, got {other:?}"),
    }
    assert_eq!(
        world.component_domain(TypeId::of::<Wanderer>()),
        Some(SemanticDomain::Spatial),
        "the refused registration moved nothing"
    );
}

#[test]
fn duplicate_short_names_are_refused_at_startup() {
    let mut world = World::new();
    world
        .try_register_component::<a::Twin>(SemanticDomain::Spatial)
        .expect("the first Twin registers");

    assert_eq!(
        world.try_register_component::<b::Twin>(SemanticDomain::Spatial),
        Err(RegisterError::DuplicateName {
            name: "Twin".to_owned(),
            first: type_name::<a::Twin>().to_owned(),
            second: type_name::<b::Twin>().to_owned(),
        })
    );
    assert!(
        world
            .components()
            .vtable(ComponentKey::of::<b::Twin>())
            .is_none(),
        "the refused type is not registered"
    );
    assert_eq!(
        world.components().key_named("Twin"),
        Some(ComponentKey::of::<a::Twin>()),
        "the name still leads to the first"
    );
}

#[test]
fn register_component_panics_on_a_duplicate_short_name() {
    let mut world = World::new();
    world.register_component::<a::Twin>(SemanticDomain::Spatial);

    let payload = catch_unwind(AssertUnwindSafe(|| {
        world.register_component::<b::Twin>(SemanticDomain::Spatial);
    }))
    .expect_err("a second `Twin` is refused at start-up");
    let message = panic_message(payload.as_ref());
    assert!(
        message.contains(type_name::<a::Twin>()) && message.contains(type_name::<b::Twin>()),
        "the message names both paths: {message}"
    );
}

#[test]
#[should_panic(expected = "khora_data::ecs::tests::component_keys::b::Twin")]
fn register_component_names_the_refused_path_in_its_panic() {
    let mut world = World::new();
    world.register_component::<a::Twin>(SemanticDomain::Spatial);
    world.register_component::<b::Twin>(SemanticDomain::Spatial);
}

#[test]
fn lookup_by_name_is_a_map() {
    // Every persistence registration is found by its name, and by each name it
    // had before.
    let mut checked = 0usize;
    for registration in inventory::iter::<ComponentRegistration> {
        let found = registration_of(registration.type_name)
            .unwrap_or_else(|| panic!("`{}` by registration_of", registration.type_name));
        assert!(
            std::ptr::eq(found, registration),
            "{}",
            registration.type_name
        );
        let named = registration_named(registration.type_name)
            .unwrap_or_else(|| panic!("`{}` by registration_named", registration.type_name));
        assert!(
            std::ptr::eq(named, registration),
            "{}",
            registration.type_name
        );
        for former in registration.formerly {
            let named = registration_named(former).unwrap_or_else(|| panic!("`{former}`"));
            assert!(std::ptr::eq(named, registration), "`{former}`");
        }
        checked += 1;
    }
    assert!(checked > 10, "the inventory holds the engine's components");
    assert!(registration_of("NoSuchComponent").is_none());
    assert!(registration_named("NoSuchComponent").is_none());

    // The world names each Rust component it stores by its registration's name.
    let mut world = World::new();
    for registration in inventory::iter::<ComponentRegistration> {
        let key = ComponentKey::Rust(registration.type_id);
        if world.components().vtable(key).is_some() {
            assert_eq!(
                world.components().key_named(registration.type_name),
                Some(key),
                "{}",
                registration.type_name
            );
        }
    }

    // And each of ten thousand run-time names.
    const COUNT: usize = 10_000;
    let keys: Vec<_> = (0..COUNT)
        .map(|i| {
            declare(
                &mut world,
                &format!("Named{i}"),
                SemanticDomain::Script,
                vec![field("n", FieldKind::Int, ScriptValue::Int(0))],
            )
        })
        .collect();
    for (i, key) in keys.iter().enumerate() {
        assert_eq!(
            world.components().key_named(&format!("Named{i}")),
            Some(*key)
        );
    }
    assert_eq!(world.components().key_named("Named10000"), None);
}

#[test]
fn access_snapshot_uses_short_names() {
    let mut world = World::new();
    world.spawn(Transform::identity());
    let _ = world.query::<&Transform>().count();

    let snapshot = world.component_access_snapshot();
    let names: Vec<&str> = snapshot.iter().map(|(name, ..)| name.as_str()).collect();
    assert!(names.contains(&"Transform"), "{names:?}");
    assert!(names.contains(&"HandleComponent<Mesh>"), "{names:?}");
    assert!(
        !names.iter().any(|name| name.contains("::")),
        "no entry is named by its path: {names:?}"
    );
    let (_, _, queries, rows) = snapshot
        .iter()
        .find(|(name, ..)| name == "Transform")
        .expect("Transform is in the snapshot");
    assert_eq!(
        (*queries, *rows),
        (1, 1),
        "Transform's access is recorded under its name"
    );
}
