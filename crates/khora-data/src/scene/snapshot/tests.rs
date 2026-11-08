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

//! Every registration, through its snapshot form: written from a page column,
//! staged from its bytes, consuming them exactly.

use khora_core::ecs::entity::EntityId;
use khora_core::ecs::PersistentId;

use crate::ecs::component::Component;
use crate::ecs::{SemanticDomain, World};
use crate::scene::record::{EntityRef, RecordError, ReferenceReader, ReferenceWriter};
use crate::scene::ComponentRegistration;

/// A component no registration is about, so an entity can exist without
/// already holding the one a test is about to add.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Anchor;
impl Component for Anchor {}

/// An entity carrying only [`Anchor`].
fn anchor(world: &mut World) -> EntityId {
    world.register_component::<Anchor>(SemanticDomain::Ui);
    world.spawn(Anchor)
}

/// References written as the entity's own bits and read back as them, so a
/// value holding one compares equal on both sides.
struct Same;

impl ReferenceWriter for Same {
    fn write_entity(&mut self, entity: EntityId) -> EntityRef {
        EntityRef::Id(PersistentId::from_bits(
            (u64::from(entity.index) << 32) | u64::from(entity.generation),
        ))
    }
}

impl ReferenceReader for Same {
    fn read_entity(&mut self, reference: EntityRef) -> Result<EntityId, RecordError> {
        match reference {
            EntityRef::Id(id) => Ok(EntityId {
                index: (id.to_bits() >> 32) as u32,
                generation: id.to_bits() as u32,
            }),
            EntityRef::Outside => Err(RecordError("an outside reference".to_owned())),
        }
    }
}

/// Every registration's default value, each on an entity of its own — every
/// one a world knows how to store: a component declared by a test module for
/// its own world, with no domain, is not.
fn every_default(world: &mut World) -> Vec<(&'static ComponentRegistration, EntityId)> {
    let storable: Vec<&'static ComponentRegistration> = inventory::iter::<ComponentRegistration>
        .into_iter()
        .filter(|reg| world.component_domain(reg.type_id).is_some())
        .collect();
    let placed: Vec<_> = storable
        .into_iter()
        .map(|reg| {
            let entity = anchor(world);
            (reg.create_default)(world, entity)
                .unwrap_or_else(|e| panic!("{}: create_default failed: {e}", reg.type_name));
            (reg, entity)
        })
        .collect();
    for (reg, entity) in &placed {
        assert!(
            world.component_cell(*entity, reg.type_id).is_some(),
            "{}: create_default added nothing",
            reg.type_name
        );
    }
    placed
}

/// The snapshot bytes of `reg`'s component on `entity`.
fn snapshot_bytes(reg: &ComponentRegistration, world: &World, entity: EntityId) -> Vec<u8> {
    let (column, row) = world
        .component_cell(entity, reg.type_id)
        .unwrap_or_else(|| panic!("{} is not on {entity:?}", reg.type_name));
    let mut bytes = Vec::new();
    (reg.column_to_snapshot)(column, row, &mut bytes, &mut Same)
        .unwrap_or_else(|e| panic!("{}: column_to_snapshot failed: {e}", reg.type_name));
    bytes
}

/// Every registration — derived, hand-written, saved or not — writes its
/// component from a page column and stages it back from those bytes, equal
/// as the inspector sees it.
#[test]
fn every_registration_round_trips_through_its_snapshot_form() {
    let mut src = World::new();
    let placed = every_default(&mut src);
    assert!(placed.len() >= 30, "only {} registrations", placed.len());

    for (reg, entity) in placed {
        let bytes = snapshot_bytes(reg, &src, entity);
        let staged = (reg.stage_snapshot)(&bytes, &mut Same)
            .unwrap_or_else(|e| panic!("{}: stage_snapshot failed: {e}", reg.type_name));
        assert!(
            staged.report.is_empty(),
            "{}: a snapshot adapts nothing: {:?}",
            reg.type_name,
            staged.report
        );
        let mut dst = World::new();
        let target = anchor(&mut dst);
        staged
            .component
            .commit(&mut dst, target)
            .unwrap_or_else(|e| panic!("{}: commit failed: {e}", reg.type_name));
        assert_eq!(
            (reg.to_json)(&dst, target),
            (reg.to_json)(&src, entity),
            "{} changed through its snapshot form",
            reg.type_name
        );
    }
}

/// The bytes of one value are exactly that value: one byte more, or one
/// byte less, is refused by every registration.
#[test]
fn a_value_must_consume_its_bytes_exactly() {
    let mut src = World::new();
    for (reg, entity) in every_default(&mut src) {
        let bytes = snapshot_bytes(reg, &src, entity);

        let mut longer = bytes.clone();
        longer.push(0);
        assert!(
            (reg.stage_snapshot)(&longer, &mut Same).is_err(),
            "{}: a trailing byte was accepted",
            reg.type_name
        );

        if let Some((_, shorter)) = bytes.split_last() {
            assert!(
                (reg.stage_snapshot)(shorter, &mut Same).is_err(),
                "{}: a value cut short was accepted",
                reg.type_name
            );
        }
    }
}

/// How many snapshot payloads this thread read while `load` ran.
fn payloads_read_by(load: impl FnOnce()) -> usize {
    let before = super::PAYLOADS_READ.with(std::cell::Cell::get);
    load();
    super::PAYLOADS_READ.with(std::cell::Cell::get) - before
}

/// **A snapshot is parsed once per prepare.** Restoring a snapshot — the
/// editor's Stop — reads its payload once, whether it is prepared with the
/// world or off the frame against its catalogue: the count of entities it
/// reserves for comes from that one reading, not a reading of its own.
#[test]
fn a_snapshot_is_parsed_once_per_prepare() {
    let mut src = World::new();
    for i in 0..16 {
        let entity = src.spawn(crate::ecs::Transform::from_translation(
            khora_core::math::Vec3::new(i as f32, 0.0, 0.0),
        ));
        src.mark_authored(entity).expect("authored");
    }
    let file = super::write_snapshot(&src).expect("the world snapshots");

    let mut world = World::new();
    let read = payloads_read_by(|| {
        let prepared = super::prepare_snapshot(&mut world, &file).expect("the snapshot prepares");
        prepared.abandon(&mut world);
    });
    assert_eq!(
        read, 1,
        "the synchronous prepare read the payload {read} times"
    );

    let catalog = world.load_catalog();
    let hold = world.hold_reservations();
    let file = std::sync::Arc::new(file);
    let read = payloads_read_by(|| {
        let prepared = super::prepare_snapshot_with(&catalog, hold, file)
            .expect("the snapshot prepares off the frame");
        prepared.abandon(&mut world);
    });
    assert_eq!(
        read, 1,
        "the prepare off the frame read the payload {read} times"
    );
}
