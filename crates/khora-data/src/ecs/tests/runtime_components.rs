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

//! Components with no Rust type.
//!
//! A run-time component is declared by its fields while the engine runs (by a
//! script), registered under a name no Rust component holds, and stored in the
//! page of its declared domain beside the Rust components of the same entity:
//! it migrates with its entity, survives the swap-remove of a neighbour, and is
//! read and written by id, one field at a time.

use khora_core::ecs::entity::EntityId;
use khora_core::math::{LinearRgba, Vec3};
use khora_core::script::ScriptValue;

use super::Position;
use crate::ecs::{
    Component, ComponentKey, ComponentProvenance, FieldError, FieldKind, LayoutError, PackedField,
    PackedLayout, RegisterError, RowError, RuntimeComponentDecl, SemanticDomain, Transform, World,
};

/// A Physics-domain Rust component, to migrate run-time rows beside.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Mass(f32);
impl Component for Mass {}

/// A Script-domain Rust component, to migrate run-time rows beside.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Tick(u32);
impl Component for Tick {}

// --- helpers, shared with the relayout tests ---

/// A field declaration.
pub(super) fn field(name: &str, kind: FieldKind, default: ScriptValue) -> PackedField {
    PackedField {
        name: name.to_owned(),
        kind,
        default,
    }
}

/// A layout the test means to be valid.
pub(super) fn layout(fields: Vec<PackedField>) -> PackedLayout {
    PackedLayout::new(fields).expect("a valid layout")
}

/// A declaration of an authored run-time component.
pub(super) fn decl(
    name: &str,
    domain: SemanticDomain,
    fields: Vec<PackedField>,
) -> RuntimeComponentDecl {
    RuntimeComponentDecl {
        name: name.to_owned(),
        domain,
        provenance: ComponentProvenance::Authored,
        layout: layout(fields),
    }
}

/// Registers a run-time component the test means to succeed.
pub(super) fn declare(
    world: &mut World,
    name: &str,
    domain: SemanticDomain,
    fields: Vec<PackedField>,
) -> ComponentKey {
    world
        .register_runtime_component(decl(name, domain, fields))
        .unwrap_or_else(|error| panic!("`{name}` registers: {error:?}"))
}

/// A patch naming some of a component's fields.
pub(super) fn patch(fields: &[(&str, ScriptValue)]) -> ScriptValue {
    ScriptValue::Struct(
        fields
            .iter()
            .map(|(name, value)| ((*name).to_owned(), value.clone()))
            .collect(),
    )
}

/// The slot of field `name` of run-time component `id`, in its current layout.
pub(super) fn slot(world: &World, id: ComponentKey, name: &str) -> Option<usize> {
    world
        .components()
        .vtable(id)
        .expect("a registered component")
        .columns
        .packed()
        .expect("a run-time component has a packed layout")
        .slot_of(name)
}

/// `entity`'s value of field `name` of component `id`.
pub(super) fn read(
    world: &World,
    entity: EntityId,
    id: ComponentKey,
    name: &str,
) -> Option<ScriptValue> {
    let slot = slot(world, id, name)?;
    world.row(entity, id)?.field(slot)
}

/// Writes `entity`'s field `name` of component `id`.
pub(super) fn write(
    world: &mut World,
    entity: EntityId,
    id: ComponentKey,
    name: &str,
    value: ScriptValue,
) -> Result<(), FieldError> {
    let slot = slot(world, id, name).expect("the field exists");
    world
        .row_mut(entity, id)
        .expect("the entity holds the component")
        .set_field(slot, &value)
}

/// `entity`'s value of every field in `names`.
fn read_all(
    world: &World,
    entity: EntityId,
    id: ComponentKey,
    names: &[&str],
) -> Vec<Option<ScriptValue>> {
    names
        .iter()
        .map(|name| read(world, entity, id, name))
        .collect()
}

fn str_value(text: &str) -> ScriptValue {
    ScriptValue::Str(text.to_owned())
}

fn int_array(values: &[i64]) -> ScriptValue {
    ScriptValue::Array(values.iter().map(|v| ScriptValue::Int(*v)).collect())
}

/// A component with a field of most kinds, `Value` ones included.
fn stats_fields() -> Vec<PackedField> {
    vec![
        field("hp", FieldKind::Int, ScriptValue::Int(100)),
        field("speed", FieldKind::Float, ScriptValue::Float(1.5)),
        field("heading", FieldKind::Vec3, ScriptValue::Vec3(Vec3::ZERO)),
        field("alive", FieldKind::Bool, ScriptValue::Bool(true)),
        field(
            "tint",
            FieldKind::Color,
            ScriptValue::Color(LinearRgba::new(1.0, 1.0, 1.0, 1.0)),
        ),
        field("label", FieldKind::Value, str_value("none")),
        field(
            "inventory",
            FieldKind::Value,
            ScriptValue::Array(Vec::new()),
        ),
    ]
}

const STATS: [&str; 7] = [
    "hp",
    "speed",
    "heading",
    "alive",
    "tint",
    "label",
    "inventory",
];

/// Values for every field of `stats_fields`, none of them the default.
fn stats_values(seed: i64) -> ScriptValue {
    patch(&[
        ("hp", ScriptValue::Int(seed)),
        ("speed", ScriptValue::Float(seed as f32 * 0.5)),
        (
            "heading",
            ScriptValue::Vec3(Vec3::new(seed as f32, 2.0, -3.0)),
        ),
        ("alive", ScriptValue::Bool(false)),
        (
            "tint",
            ScriptValue::Color(LinearRgba::new(0.25, 0.5, 0.75, 1.0)),
        ),
        ("label", str_value(&format!("unit-{seed}"))),
        ("inventory", int_array(&[seed, seed + 1, seed + 2])),
    ])
}

/// What `read_all` reads back from a row written with `stats_values(seed)`.
fn stats_read_back(seed: i64) -> Vec<Option<ScriptValue>> {
    let ScriptValue::Struct(fields) = stats_values(seed) else {
        unreachable!()
    };
    fields.into_iter().map(|(_, value)| Some(value)).collect()
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

// --- registration ---

#[test]
fn a_runtime_component_cannot_take_a_rust_name() {
    let mut world = World::new();
    world.register_component::<Position>(SemanticDomain::Spatial);
    let before = world.components().len();

    for name in ["Transform", "Position"] {
        assert_eq!(
            world.register_runtime_component(decl(
                name,
                SemanticDomain::Script,
                vec![field("n", FieldKind::Int, ScriptValue::Int(0))],
            )),
            Err(RegisterError::NameTaken {
                name: name.to_owned()
            }),
        );
    }
    assert_eq!(world.components().len(), before, "nothing was registered");
    assert_eq!(
        world.components().key_named("Transform"),
        Some(ComponentKey::of::<Transform>()),
        "the name still leads to the Rust component"
    );
}

#[test]
fn a_runtime_name_registered_twice_is_reported() {
    let mut world = World::new();
    let first = declare(
        &mut world,
        "Health",
        SemanticDomain::Script,
        vec![field("hp", FieldKind::Int, ScriptValue::Int(10))],
    );
    let before = world.components().len();

    assert_eq!(
        world.register_runtime_component(decl(
            "Health",
            SemanticDomain::Physics,
            vec![field("hp", FieldKind::Float, ScriptValue::Float(1.0))],
        )),
        Err(RegisterError::AlreadyRegistered {
            name: "Health".to_owned(),
            key: first,
        }),
    );
    assert_eq!(world.components().len(), before);
    let vtable = world.components().vtable(first).expect("the first stays");
    assert_eq!(&*vtable.name, "Health");
    assert_eq!(vtable.domain, SemanticDomain::Script);
}

#[test]
fn a_runtime_component_is_described_by_the_registry() {
    let mut world = World::new();
    let id = world
        .register_runtime_component(RuntimeComponentDecl {
            name: "Wake".to_owned(),
            domain: SemanticDomain::Physics,
            provenance: ComponentProvenance::Runtime,
            layout: layout(vec![field(
                "strength",
                FieldKind::Float,
                ScriptValue::Float(0.0),
            )]),
        })
        .expect("Wake registers");

    let vtable = world.components().vtable(id).expect("its vtable");
    assert_eq!(vtable.key, id);
    assert_eq!(&*vtable.name, "Wake");
    assert!(vtable.rust.is_none(), "no Rust type");
    assert_eq!(vtable.domain, SemanticDomain::Physics);
    assert_eq!(vtable.provenance, ComponentProvenance::Runtime);
    let packed = vtable
        .columns
        .packed()
        .expect("a run-time component has fields");
    assert_eq!(packed.fields().len(), 1);
    assert_eq!(packed.fields()[0].name, "strength");
    assert_eq!(world.components().key_named("Wake"), Some(id));
}

#[test]
fn a_packed_layout_refuses_duplicate_fields() {
    assert_eq!(
        PackedLayout::new(vec![
            field("hp", FieldKind::Int, ScriptValue::Int(1)),
            field("speed", FieldKind::Float, ScriptValue::Float(0.0)),
            field("hp", FieldKind::Float, ScriptValue::Float(0.0)),
        ])
        .err(),
        Some(LayoutError::DuplicateField {
            field: "hp".to_owned()
        }),
    );
}

#[test]
fn a_packed_layout_refuses_a_default_of_another_kind() {
    assert_eq!(
        PackedLayout::new(vec![
            field("hp", FieldKind::Int, ScriptValue::Int(1)),
            field("speed", FieldKind::Float, str_value("fast")),
        ])
        .err(),
        Some(LayoutError::DefaultOfAnotherKind {
            field: "speed".to_owned(),
            kind: FieldKind::Float,
            found: "string".to_owned(),
        }),
    );
}

#[test]
fn a_packed_layout_lists_its_fields_in_order() {
    let packed = layout(stats_fields());
    assert_eq!(packed.fields(), stats_fields().as_slice());
    for (index, name) in STATS.iter().enumerate() {
        let slot = packed.slot_of(name).expect("every field has a slot");
        assert_eq!(packed.fields()[slot].name, *name, "field {index}");
    }
    assert_eq!(packed.slot_of("missing"), None);
}

// --- storage ---

#[test]
fn a_runtime_component_lives_in_its_domain_page() {
    let mut world = World::new();
    let buoyancy = declare(
        &mut world,
        "Buoyancy",
        SemanticDomain::Physics,
        vec![field("lift", FieldKind::Float, ScriptValue::Float(1.0))],
    );
    let entity = world.spawn(Transform::identity());
    let physics_epoch = world.domain_epoch(SemanticDomain::Physics);

    world
        .add_runtime_component(
            entity,
            buoyancy,
            &patch(&[("lift", ScriptValue::Float(4.0))]),
        )
        .expect("Buoyancy attaches");

    let physics =
        location(&world, entity, SemanticDomain::Physics).expect("the entity has a Physics row");
    let spatial = location(&world, entity, SemanticDomain::Spatial)
        .expect("the entity keeps its Spatial row");
    assert_ne!(physics.page_id, spatial.page_id, "two domains, two pages");

    let physics_page = &world.storage.pages[physics.page_id as usize];
    assert_eq!(physics_page.entities[physics.row_index as usize], entity);
    assert_eq!(
        physics_page.columns.len(),
        1,
        "the Physics page holds the run-time column alone"
    );
    let spatial_page = &world.storage.pages[spatial.page_id as usize];
    assert_eq!(spatial_page.entities[spatial.row_index as usize], entity);
    assert_eq!(
        spatial_page.columns.len(),
        1,
        "the Spatial page still holds Transform alone"
    );

    assert!(world.domain_epoch(SemanticDomain::Physics) > physics_epoch);
    assert_eq!(world.get::<Transform>(entity), Some(&Transform::identity()));
    let row = world.row(entity, buoyancy).expect("the row is there");
    assert_eq!(row.key(), buoyancy);
    assert_eq!(&*row.vtable().name, "Buoyancy");
    assert_eq!(
        read(&world, entity, buoyancy, "lift"),
        Some(ScriptValue::Float(4.0))
    );
}

#[test]
fn runtime_components_have_no_cap() {
    const COUNT: usize = 10_000;
    let mut world = World::new();
    let before = world.components().len();

    let ids: Vec<ComponentKey> = (0..COUNT)
        .map(|i| {
            declare(
                &mut world,
                &format!("Generated{i}"),
                SemanticDomain::Script,
                vec![field("n", FieldKind::Int, ScriptValue::Int(-1))],
            )
        })
        .collect();
    assert_eq!(world.components().len(), before + COUNT);

    let entities: Vec<EntityId> = ids
        .iter()
        .enumerate()
        .map(|(i, id)| {
            let entity = world.spawn(());
            world
                .add_runtime_component(entity, *id, &patch(&[("n", ScriptValue::Int(i as i64))]))
                .unwrap_or_else(|error| panic!("Generated{i} attaches: {error:?}"));
            entity
        })
        .collect();

    for (i, (entity, id)) in entities.iter().zip(&ids).enumerate() {
        assert_eq!(
            read(&world, *entity, *id, "n"),
            Some(ScriptValue::Int(i as i64)),
            "Generated{i}"
        );
    }
}

#[test]
fn one_entity_holds_many_runtime_components() {
    const COUNT: usize = 100;
    let mut world = World::new();
    let ids: Vec<ComponentKey> = (0..COUNT)
        .map(|i| {
            declare(
                &mut world,
                &format!("Stacked{i}"),
                SemanticDomain::Script,
                vec![field("n", FieldKind::Int, ScriptValue::Int(-1))],
            )
        })
        .collect();
    let entity = world.spawn(());
    for (i, id) in ids.iter().enumerate() {
        world
            .add_runtime_component(entity, *id, &patch(&[("n", ScriptValue::Int(i as i64))]))
            .unwrap_or_else(|error| panic!("Stacked{i} attaches: {error:?}"));
    }

    let script = location(&world, entity, SemanticDomain::Script).expect("a Script row");
    assert_eq!(
        world.storage.pages[script.page_id as usize].columns.len(),
        COUNT,
        "one page holds every one of them"
    );
    for (i, id) in ids.iter().enumerate() {
        assert_eq!(
            read(&world, entity, *id, "n"),
            Some(ScriptValue::Int(i as i64)),
            "Stacked{i}"
        );
    }
}

#[test]
fn a_runtime_component_migrates_with_its_entity() {
    let mut world = World::new();
    world.register_component::<Mass>(SemanticDomain::Physics);
    let stats = declare(&mut world, "Stats", SemanticDomain::Physics, stats_fields());

    // A neighbour in the same page, so the migration leaves a row behind it.
    let neighbour = world.spawn(Transform::identity());
    world
        .add_runtime_component(neighbour, stats, &stats_values(7))
        .expect("the neighbour's Stats attach");
    let entity = world.spawn(Transform::from_translation(Vec3::new(1.0, 2.0, 3.0)));
    world
        .add_runtime_component(entity, stats, &stats_values(40))
        .expect("Stats attach");
    assert_eq!(read_all(&world, entity, stats, &STATS), stats_read_back(40));
    let before = location(&world, entity, SemanticDomain::Physics).expect("a Physics row");

    world
        .add_component(entity, Mass(2.5))
        .expect("Mass attaches");
    let after = location(&world, entity, SemanticDomain::Physics).expect("a Physics row");
    assert_ne!(
        before.page_id, after.page_id,
        "the row moved to Mass's page"
    );
    assert_eq!(
        read_all(&world, entity, stats, &STATS),
        stats_read_back(40),
        "every field moved with the row"
    );
    assert_eq!(world.get::<Mass>(entity), Some(&Mass(2.5)));
    assert_eq!(
        read_all(&world, neighbour, stats, &STATS),
        stats_read_back(7),
        "the row left behind is untouched"
    );

    world
        .remove_component::<Mass>(entity)
        .expect("Mass detaches");
    assert_eq!(world.get::<Mass>(entity), None);
    assert_eq!(
        read_all(&world, entity, stats, &STATS),
        stats_read_back(40),
        "and back"
    );
    assert_eq!(
        read_all(&world, neighbour, stats, &STATS),
        stats_read_back(7)
    );
    assert_eq!(
        world.get::<Transform>(entity),
        Some(&Transform::from_translation(Vec3::new(1.0, 2.0, 3.0)))
    );
}

#[test]
fn a_runtime_row_reads_its_defaults() {
    let mut world = World::new();
    let stats = declare(&mut world, "Stats", SemanticDomain::Script, stats_fields());
    let defaults: Vec<Option<ScriptValue>> = stats_fields()
        .into_iter()
        .map(|field| Some(field.default))
        .collect();

    let plain = world.spawn(());
    world
        .add_runtime_component(plain, stats, &ScriptValue::Unit)
        .expect("Stats attach with their defaults");
    assert_eq!(read_all(&world, plain, stats, &STATS), defaults);

    let patched = world.spawn(());
    world
        .add_runtime_component(
            patched,
            stats,
            &patch(&[
                ("speed", ScriptValue::Float(9.0)),
                ("label", str_value("boss")),
            ]),
        )
        .expect("Stats attach patched");
    let mut expected = defaults.clone();
    expected[1] = Some(ScriptValue::Float(9.0));
    expected[5] = Some(str_value("boss"));
    assert_eq!(
        read_all(&world, patched, stats, &STATS),
        expected,
        "the patch overrides only the fields it names"
    );
    assert_eq!(
        read_all(&world, plain, stats, &STATS),
        defaults,
        "the first row kept its defaults"
    );
}

#[test]
fn adding_a_runtime_row_refuses_what_it_cannot_hold() {
    let mut world = World::new();
    let stats = declare(&mut world, "Stats", SemanticDomain::Script, stats_fields());

    // A patch of the wrong kind attaches nothing.
    let entity = world.spawn(Transform::identity());
    assert_eq!(
        world.add_runtime_component(entity, stats, &patch(&[("speed", str_value("fast"))])),
        Err(RowError::Field(FieldError {
            field: "speed".to_owned(),
            expected: FieldKind::Float,
            found: "string".to_owned(),
        })),
    );
    assert!(world.row(entity, stats).is_none(), "nothing was attached");

    // Once attached, a second one is refused.
    world
        .add_runtime_component(entity, stats, &ScriptValue::Unit)
        .expect("Stats attach");
    assert_eq!(
        world.add_runtime_component(entity, stats, &stats_values(3)),
        Err(RowError::AlreadyAttached),
    );
    assert_eq!(
        read(&world, entity, stats, "hp"),
        Some(ScriptValue::Int(100)),
        "the refused add wrote nothing"
    );

    // A dead entity has nothing to attach to.
    let dead = world.spawn(());
    assert!(world.despawn(dead));
    assert_eq!(
        world.add_runtime_component(dead, stats, &ScriptValue::Unit),
        Err(RowError::NoSuchEntity(dead)),
    );
    assert!(world.row(dead, stats).is_none());
}

#[test]
fn a_runtime_field_refuses_the_wrong_kind() {
    let mut world = World::new();
    let stats = declare(&mut world, "Stats", SemanticDomain::Physics, stats_fields());
    let entity = world.spawn(());
    world
        .add_runtime_component(entity, stats, &stats_values(4))
        .expect("Stats attach");

    assert_eq!(
        write(&mut world, entity, stats, "speed", str_value("fast")),
        Err(FieldError {
            field: "speed".to_owned(),
            expected: FieldKind::Float,
            found: "string".to_owned(),
        }),
    );
    assert_eq!(
        read(&world, entity, stats, "speed"),
        Some(ScriptValue::Float(2.0)),
        "the value is unchanged"
    );
    assert_eq!(read_all(&world, entity, stats, &STATS), stats_read_back(4));
}

#[test]
fn setting_a_runtime_field_bumps_its_domain_epoch() {
    let mut world = World::new();
    let stats = declare(&mut world, "Stats", SemanticDomain::Physics, stats_fields());
    let entity = world.spawn(());
    world
        .add_runtime_component(entity, stats, &ScriptValue::Unit)
        .expect("Stats attach");
    let before = world.domain_epoch(SemanticDomain::Physics);

    write(&mut world, entity, stats, "speed", ScriptValue::Float(3.0)).expect("a float fits");
    write(&mut world, entity, stats, "inventory", int_array(&[1, 2])).expect("a value fits");

    assert!(world.domain_epoch(SemanticDomain::Physics) > before);
    assert_eq!(
        read(&world, entity, stats, "speed"),
        Some(ScriptValue::Float(3.0))
    );
    assert_eq!(
        read(&world, entity, stats, "inventory"),
        Some(int_array(&[1, 2]))
    );
}

#[test]
fn removing_a_runtime_component_detaches_it() {
    let mut world = World::new();
    let marker = declare(
        &mut world,
        "Marker",
        SemanticDomain::Spatial,
        vec![field("weight", FieldKind::Float, ScriptValue::Float(0.0))],
    );
    let score = declare(
        &mut world,
        "Score",
        SemanticDomain::Script,
        vec![field("points", FieldKind::Int, ScriptValue::Int(0))],
    );

    let spawn = |world: &mut World, x: f32| {
        let entity = world.spawn(Transform::from_translation(Vec3::new(x, 0.0, 0.0)));
        world
            .add_runtime_component(entity, marker, &patch(&[("weight", ScriptValue::Float(x))]))
            .expect("Marker attaches");
        world
            .add_runtime_component(
                entity,
                score,
                &patch(&[("points", ScriptValue::Int(x as i64))]),
            )
            .expect("Score attaches");
        entity
    };
    let entity = spawn(&mut world, 1.0);
    let other = spawn(&mut world, 2.0);
    let spatial_epoch = world.domain_epoch(SemanticDomain::Spatial);

    assert!(world.remove_component_by_key(entity, marker));
    assert!(world.row(entity, marker).is_none());
    assert!(world.domain_epoch(SemanticDomain::Spatial) > spatial_epoch);
    assert_eq!(
        world.get::<Transform>(entity),
        Some(&Transform::from_translation(Vec3::new(1.0, 0.0, 0.0))),
        "the Rust component of the same domain stays"
    );
    assert_eq!(
        read(&world, entity, score, "points"),
        Some(ScriptValue::Int(1)),
        "the other run-time component stays"
    );
    assert_eq!(
        read(&world, other, marker, "weight"),
        Some(ScriptValue::Float(2.0)),
        "another entity's row stays"
    );
    assert!(
        !world.remove_component_by_key(entity, marker),
        "a second removal finds nothing"
    );
}

#[test]
fn removing_by_key_also_detaches_a_rust_component() {
    let mut world = World::new();
    let marker = declare(
        &mut world,
        "Marker",
        SemanticDomain::Spatial,
        vec![field("weight", FieldKind::Float, ScriptValue::Float(0.0))],
    );
    let entity = world.spawn(Transform::identity());
    world
        .add_runtime_component(
            entity,
            marker,
            &patch(&[("weight", ScriptValue::Float(5.0))]),
        )
        .expect("Marker attaches");
    let transform = ComponentKey::of::<Transform>();
    assert!(
        world.components().vtable(transform).is_some(),
        "Transform is registered"
    );

    assert!(world.remove_component_by_key(entity, transform));
    assert_eq!(world.get::<Transform>(entity), None);
    assert_eq!(
        read(&world, entity, marker, "weight"),
        Some(ScriptValue::Float(5.0))
    );
}

#[test]
fn a_runtime_component_without_fields_is_a_tag() {
    let mut world = World::new();
    let frozen = declare(&mut world, "Frozen", SemanticDomain::Script, Vec::new());
    let entity = world.spawn(());
    let other = world.spawn(());

    world
        .add_runtime_component(entity, frozen, &ScriptValue::Unit)
        .expect("Frozen attaches");
    world
        .add_runtime_component(other, frozen, &ScriptValue::Unit)
        .expect("Frozen attaches");
    let row = world.row(entity, frozen).expect("the tag is there");
    assert_eq!(row.field(0), None, "a tag has no field");

    assert!(world.despawn(other));
    assert!(
        world.row(entity, frozen).is_some(),
        "it survives a neighbour's despawn"
    );
    assert!(world.remove_component_by_key(entity, frozen));
    assert!(world.row(entity, frozen).is_none());
}

#[test]
fn a_value_field_is_deep_copied_with_its_row() {
    let mut world = World::new();
    world.register_component::<Tick>(SemanticDomain::Script);
    let bag = declare(
        &mut world,
        "Bag",
        SemanticDomain::Script,
        vec![
            field("items", FieldKind::Value, ScriptValue::Array(Vec::new())),
            field("owner", FieldKind::Value, str_value("")),
        ],
    );
    let fill = |world: &mut World, seed: i64| {
        let entity = world.spawn(());
        world
            .add_runtime_component(
                entity,
                bag,
                &patch(&[
                    ("items", int_array(&[seed, seed * 10])),
                    ("owner", str_value(&format!("owner-{seed}"))),
                ]),
            )
            .expect("Bag attaches");
        entity
    };
    let bag_of =
        |world: &World, entity: EntityId| read_all(world, entity, bag, &["items", "owner"]);
    let expected = |seed: i64| {
        vec![
            Some(int_array(&[seed, seed * 10])),
            Some(str_value(&format!("owner-{seed}"))),
        ]
    };

    let first = fill(&mut world, 1);
    let second = fill(&mut world, 2);
    let third = fill(&mut world, 3);

    // The last row is swapped into the first's place.
    assert!(world.despawn(first));
    assert_eq!(bag_of(&world, second), expected(2));
    assert_eq!(
        bag_of(&world, third),
        expected(3),
        "the swapped row kept its values"
    );

    // A migration copies the row to another page.
    world.add_component(second, Tick(1)).expect("Tick attaches");
    assert_eq!(
        bag_of(&world, second),
        expected(2),
        "the migrated row kept its values"
    );
    assert_eq!(bag_of(&world, third), expected(3));

    // The copy owns its values: writing it changes no other row.
    write(&mut world, second, bag, "items", int_array(&[99])).expect("a value fits");
    write(&mut world, second, bag, "owner", str_value("changed")).expect("a value fits");
    assert_eq!(
        bag_of(&world, second),
        vec![Some(int_array(&[99])), Some(str_value("changed"))]
    );
    assert_eq!(bag_of(&world, third), expected(3));

    // And the row left in the old page by the migration is gone with it.
    assert!(world.despawn(third));
    assert_eq!(
        bag_of(&world, second),
        vec![Some(int_array(&[99])), Some(str_value("changed"))]
    );
}

#[test]
fn queries_still_find_rust_components_next_to_runtime_ones() {
    let mut world = World::new();
    world.register_component::<Mass>(SemanticDomain::Physics);
    let drag = declare(
        &mut world,
        "Drag",
        SemanticDomain::Spatial,
        vec![field("k", FieldKind::Float, ScriptValue::Float(0.1))],
    );
    let lift = declare(
        &mut world,
        "Lift",
        SemanticDomain::Physics,
        vec![field("k", FieldKind::Float, ScriptValue::Float(0.1))],
    );

    let at = |x: f32| Transform::from_translation(Vec3::new(x, 0.0, 0.0));
    // A run-time component in Transform's own page.
    let beside = world.spawn(at(1.0));
    world
        .add_runtime_component(beside, drag, &ScriptValue::Unit)
        .expect("Drag attaches");
    // None at all.
    let plain = world.spawn(at(2.0));
    // One in another domain, beside a Rust component of that domain.
    let physical = world.spawn((at(3.0), Mass(4.0)));
    world
        .add_runtime_component(physical, lift, &ScriptValue::Unit)
        .expect("Lift attaches");

    let mut xs: Vec<f32> = world
        .query::<&Transform>()
        .map(|transform| transform.translation.x)
        .collect();
    xs.sort_by(f32::total_cmp);
    assert_eq!(xs, vec![1.0, 2.0, 3.0], "each entity once");

    let mut entities: Vec<EntityId> = world
        .query::<(EntityId, &Transform)>()
        .map(|(entity, _)| entity)
        .collect();
    entities.sort_by_key(|entity| entity.index);
    let mut expected = vec![beside, plain, physical];
    expected.sort_by_key(|entity| entity.index);
    assert_eq!(entities, expected);

    let joined: Vec<(f32, f32)> = world
        .query::<(&Transform, &Mass)>()
        .map(|(transform, mass)| (transform.translation.x, mass.0))
        .collect();
    assert_eq!(joined, vec![(3.0, 4.0)]);

    for transform in world.query_mut::<&mut Transform>() {
        transform.translation.y = 7.0;
    }
    assert_eq!(
        world.get::<Transform>(beside).map(|t| t.translation.y),
        Some(7.0)
    );
    assert_eq!(
        read(&world, beside, drag, "k"),
        Some(ScriptValue::Float(0.1)),
        "the run-time row beside it is untouched"
    );
}
