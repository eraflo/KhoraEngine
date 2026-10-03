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

//! Guard over the paths `#[derive(Component)]` expands to.
//!
//! `khora-macros` spells its output with `crate::` paths, because the derive is
//! only used inside `khora-data`:
//!
//! - `crate::ecs::component::Component`
//! - `crate::ecs::soa::{SoaLayout, FieldSoaColumn}` (field-SoA form only)
//! - `crate::ecs::page::AnyVec` (field-SoA form only)
//! - `crate::ecs::{ComponentDomainRegistration, ComponentProvenance, SemanticDomain, World}`
//! - `crate::scene::{ComponentRegistration, ComponentShape, FieldSchema, Staged, StagedValue}`
//! - `crate::scene::record::{to_record, resolve}`
//!
//! This test crate rebuilds that `crate::ecs` / `crate::scene` shape out of
//! `khora_data`'s public paths, so the derive expands here exactly as it does in
//! `khora-data`. The public half of each macro path is therefore checked at
//! compile time: `ecs::component` and `ecs::soa` are re-exported as whole
//! modules, the way the macro reaches them. The derived components are then
//! driven through a column, through `World`, and through their scene
//! registration, in both the plain and the field-SoA (`layout = "soa"`) form.
//!
//! What this cannot check is the in-crate spelling itself (`ecs::page` is a
//! private module): a move of `component.rs`, `soa.rs` or `page.rs` breaks the
//! build of `khora-data`'s own derived components instead.

use khora_data::ecs::component::Component as _;
use khora_data::ecs::soa::SoaLayout as _;
use khora_data::ecs::{FieldSoaColumn, SemanticDomain, Soa, World};
use khora_data::scene::{ComponentShape, FieldSchema};
use khora_macros::Component;

/// The `crate::ecs` the derive expands against.
mod ecs {
    pub use khora_data::ecs::{component, soa};
    pub use khora_data::ecs::{
        ComponentDomainRegistration, ComponentProvenance, SemanticDomain, World,
    };

    /// `page` is private in `khora-data`; `AnyVec` is public at `ecs::AnyVec`.
    pub mod page {
        pub use khora_data::ecs::AnyVec;
    }
}

/// The `crate::scene` the derive expands against.
mod scene {
    pub use khora_data::scene::{positional, record, schema};
    pub use khora_data::scene::{
        ComponentRegistration, ComponentShape, FieldSchema, Staged, StagedValue,
    };
}

/// References for values that hold no entity.
struct NoEntities;

impl khora_data::scene::record::ReferenceWriter for NoEntities {
    fn write_entity(
        &mut self,
        entity: khora_core::ecs::entity::EntityId,
    ) -> khora_data::scene::record::EntityRef {
        panic!("unexpected entity {entity:?}")
    }
}

impl khora_data::scene::record::ReferenceReader for NoEntities {
    fn read_entity(
        &mut self,
        reference: khora_data::scene::record::EntityRef,
    ) -> Result<khora_core::ecs::entity::EntityId, khora_data::scene::record::RecordError> {
        panic!("unexpected entity {reference:?}")
    }
}

/// Plain (array-of-structs column) form, with a domain and serialization.
#[derive(Debug, Clone, PartialEq, Default, Component)]
#[component(domain = Physics)]
pub struct GuardMass {
    pub kg: f32,
    pub label: String,
}

/// Field-SoA form: one `Vec<f32>` per field.
///
/// `layout` comes first on purpose: the derive's `layout` scan stops at the
/// first key it does not consume, so `domain = Physics, layout = "soa"` would
/// silently yield the plain form.
#[derive(Debug, Clone, Copy, PartialEq, Default, Component)]
#[component(layout = "soa", domain = Physics)]
pub struct GuardSpin {
    pub x: f32,
    pub y: f32,
    pub z: f32,
}

fn mass(kg: f32, label: &str) -> GuardMass {
    GuardMass {
        kg,
        label: label.to_owned(),
    }
}

fn spin(x: f32, y: f32, z: f32) -> GuardSpin {
    GuardSpin { x, y, z }
}

#[test]
fn a_plain_derived_component_round_trips_through_its_column() {
    let mut column = GuardMass::make_column();
    mass(1.5, "a").push_into_column(&mut *column);
    mass(2.5, "b").push_into_column(&mut *column);

    assert_eq!(GuardMass::clone_from_column(&*column, 0), mass(1.5, "a"));
    assert_eq!(GuardMass::clone_from_column(&*column, 1), mass(2.5, "b"));

    mass(9.0, "c").set_in_column(&mut *column, 0);
    assert_eq!(GuardMass::clone_from_column(&*column, 0), mass(9.0, "c"));

    let mut other = GuardMass::make_column();
    GuardMass::copy_row_between(&*column, 1, &mut *other);
    assert_eq!(GuardMass::clone_from_column(&*other, 0), mass(2.5, "b"));
}

#[test]
fn a_field_soa_derived_component_is_stored_one_array_per_field() {
    assert_eq!(GuardSpin::FIELD_COUNT, 3);
    assert_eq!(GuardSpin::FIELD_NAMES, &["x", "y", "z"]);

    let mut column = GuardSpin::make_column();
    spin(1.0, 2.0, 3.0).push_into_column(&mut *column);
    spin(4.0, 5.0, 6.0).push_into_column(&mut *column);

    let soa = column
        .as_any()
        .downcast_ref::<FieldSoaColumn<GuardSpin>>()
        .expect("a field-SoA component must get a FieldSoaColumn");
    assert_eq!(soa.len(), 2);
    assert_eq!(soa.field(0), &[1.0, 4.0]);
    assert_eq!(soa.field(1), &[2.0, 5.0]);
    assert_eq!(soa.field(2), &[3.0, 6.0]);

    assert_eq!(
        GuardSpin::clone_from_column(&*column, 1),
        spin(4.0, 5.0, 6.0)
    );
    spin(7.0, 8.0, 9.0).set_in_column(&mut *column, 0);
    assert_eq!(
        GuardSpin::clone_from_column(&*column, 0),
        spin(7.0, 8.0, 9.0)
    );
}

#[test]
fn a_plain_derived_component_round_trips_through_the_world() {
    let mut world = World::new();
    // `#[component(domain = Physics)]` self-registers through `inventory`.
    assert_eq!(
        world.component_domain(std::any::TypeId::of::<GuardMass>()),
        Some(SemanticDomain::Physics)
    );

    let a = world.spawn(mass(1.0, "a"));
    let b = world.spawn(mass(2.0, "b"));

    assert_eq!(world.get::<GuardMass>(a), Some(&mass(1.0, "a")));
    let mut all: Vec<GuardMass> = world.query::<&GuardMass>().cloned().collect();
    all.sort_by(|l, r| l.kg.total_cmp(&r.kg));
    assert_eq!(all, vec![mass(1.0, "a"), mass(2.0, "b")]);

    assert!(world.set_component(b, mass(5.0, "b2")));
    assert_eq!(world.clone_component::<GuardMass>(b), Some(mass(5.0, "b2")));
}

#[test]
fn a_field_soa_derived_component_round_trips_through_the_world() {
    let mut world = World::new();
    assert_eq!(
        world.component_domain(std::any::TypeId::of::<GuardSpin>()),
        Some(SemanticDomain::Physics)
    );

    let a = world.spawn(spin(1.0, 2.0, 3.0));
    let b = world.spawn(spin(4.0, 5.0, 6.0));

    let mut all: Vec<GuardSpin> = world.query::<Soa<GuardSpin>>().collect();
    all.sort_by(|l, r| l.x.total_cmp(&r.x));
    assert_eq!(all, vec![spin(1.0, 2.0, 3.0), spin(4.0, 5.0, 6.0)]);

    world.for_each_soa_column_mut::<GuardSpin>(|column| {
        for x in column.field_mut(0) {
            *x *= 10.0;
        }
    });
    assert_eq!(
        world.clone_component::<GuardSpin>(a),
        Some(spin(10.0, 2.0, 3.0))
    );

    assert!(world.set_component(b, spin(0.5, 0.5, 0.5)));
    assert_eq!(
        world.clone_component::<GuardSpin>(b),
        Some(spin(0.5, 0.5, 0.5))
    );
}

#[test]
fn a_derived_registration_describes_and_restores_the_component() {
    let registration = khora_data::scene::registration_of("GuardMass")
        .expect("#[derive(Component)] registers through khora_data::scene::ComponentRegistration");
    assert_eq!(registration.type_id, std::any::TypeId::of::<GuardMass>());
    assert_eq!(
        registration.provenance,
        khora_data::ecs::ComponentProvenance::Authored
    );
    assert_eq!(
        registration.shape,
        ComponentShape::Fields(&[
            FieldSchema {
                name: "kg",
                ty: "f32",
            },
            FieldSchema {
                name: "label",
                ty: "String",
            },
        ])
    );

    let mut world = World::new();
    let source = world.spawn(mass(3.25, "saved"));
    let record =
        khora_data::scene::component_to_record(registration, &world, source, &mut NoEntities)
            .expect("a present component is written")
            .expect("it writes as a record");

    let target = world.spawn(());
    let staged =
        (registration.stage)(&record, &mut NoEntities).expect("the record it wrote reads back");
    staged
        .component
        .commit(&mut world, target)
        .expect("the read component attaches");
    assert_eq!(
        world.clone_component::<GuardMass>(target),
        Some(mass(3.25, "saved"))
    );
}

#[test]
fn a_field_soa_derived_registration_restores_the_component() {
    let registration =
        khora_data::scene::registration_of("GuardSpin").expect("the field-SoA form registers too");
    assert_eq!(
        registration.shape,
        ComponentShape::Fields(&[
            FieldSchema {
                name: "x",
                ty: "f32"
            },
            FieldSchema {
                name: "y",
                ty: "f32"
            },
            FieldSchema {
                name: "z",
                ty: "f32"
            },
        ])
    );

    // Through the JSON pair, which writes in place (`set_component`) when the
    // component is present. A staged record attaches with
    // `World::add_component`, which does not handle a field-SoA column today, so
    // only the writing half is guarded here.
    let mut world = World::new();
    let entity = world.spawn(spin(1.0, 2.0, 3.0));
    assert!(
        khora_data::scene::component_to_record(registration, &world, entity, &mut NoEntities)
            .is_some_and(|r| r.is_ok())
    );
    let json = (registration.to_json)(&world, entity).expect("a present component has JSON");

    assert!(world.set_component(entity, spin(0.0, 0.0, 0.0)));
    (registration.from_json)(&mut world, entity, &json).expect("the JSON it wrote reads back");
    assert_eq!(
        world.clone_component::<GuardSpin>(entity),
        Some(spin(1.0, 2.0, 3.0))
    );
}
