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

//! A component's fields, typed, by slot.
//!
//! `#[derive(Component)]` says which fields a script reaches and how — from the
//! same list the serializable mirror and the field schema are built from — and
//! a component's [`ColumnOps`] reads, writes and copies them in a page column
//! without serde: a Rust component through its type, a declared one through
//! its layout.

use std::any::TypeId;
use std::sync::Arc;

use khora_core::ecs::entity::EntityId;
use khora_core::math::{LinearRgba, Vec3};
use khora_core::script::{ErgonType, ScriptValue};
use khora_macros::Component;

use crate::ecs::{
    AnyVec, ColumnOps, Component as _, ComponentKey, ComponentProvenance, FieldKind,
    FieldWriteError, PackedField, PackedLayout, RuntimeComponentDecl, SemanticDomain, Transform,
    World,
};
use crate::scene::{registration_for, ComponentShape};

// ─── Test components ────────────────────────────────────────────────────────

/// A field a script reaches beside one tagged `skip`.
#[derive(Debug, Clone, Default, PartialEq, Component)]
struct Lantern {
    brightness: f32,
    #[component(skip)]
    flicker_cache: u32,
    lit: bool,
}

/// A field whose type no script can hold (`u64`) beside one it can.
#[derive(Debug, Clone, Default, PartialEq, Component)]
struct Odometer {
    km: f32,
    ticks: u64,
}

/// What an alias hides from a spelling.
type Meters = f32;
/// Likewise.
type Stops = Vec<EntityId>;

/// Fields typed by aliases.
#[derive(Debug, Clone, Default, PartialEq, Component)]
struct Ruler {
    length: Meters,
    stops: Stops,
}

/// An array-of-structs component with one field of most kinds.
#[derive(Debug, Clone, Default, PartialEq, Component)]
struct Lighthouse {
    range: f32,
    channel: u8,
    call_sign: String,
    target: Option<EntityId>,
}

/// A field-split (structure-of-arrays) component.
#[derive(Debug, Clone, Copy, Default, PartialEq, Component)]
#[component(layout = "soa")]
struct Sway {
    dx: f32,
    dy: f32,
}

/// A hand-written component: no registration, no script fields.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Pin;
impl crate::ecs::Component for Pin {}

// ─── Helpers ────────────────────────────────────────────────────────────────

/// A world that stores every test component.
fn world() -> World {
    let mut world = World::new();
    world.register_component::<Pin>(SemanticDomain::Ui);
    world.register_component::<Lantern>(SemanticDomain::Spatial);
    world.register_component::<Odometer>(SemanticDomain::Spatial);
    world.register_component::<Ruler>(SemanticDomain::Spatial);
    world.register_component::<Lighthouse>(SemanticDomain::Spatial);
    world.register_component::<Sway>(SemanticDomain::Physics);
    world
}

fn key<T: 'static>() -> ComponentKey {
    ComponentKey::Rust(TypeId::of::<T>())
}

/// The column operations of component `key`.
fn ops(world: &World, key: ComponentKey) -> Arc<dyn ColumnOps> {
    world
        .components()
        .vtable(key)
        .expect("a registered component")
        .columns
        .clone()
}

/// The slot of field `name` of component `key`.
fn slot(world: &World, key: ComponentKey, name: &str) -> usize {
    ops(world, key)
        .field_slot(name)
        .unwrap_or_else(|| panic!("`{name}` has a slot"))
}

/// The page column holding `entity`'s row of `key`, and the row.
fn cell(world: &World, entity: EntityId, key: ComponentKey) -> (&dyn AnyVec, usize) {
    let domain = world.components().domain_of(key).expect("registered");
    let (_, metadata) = world
        .entities
        .get(entity.index as usize)
        .expect("a live entity");
    let location = metadata.as_ref().expect("a placed entity").locations[&domain];
    let column = world.storage.pages[location.page_id as usize]
        .columns
        .get(&key)
        .expect("the entity holds the component");
    (column.as_ref(), location.row_index as usize)
}

/// The same, writable.
fn cell_mut(world: &mut World, entity: EntityId, key: ComponentKey) -> (&mut dyn AnyVec, usize) {
    let domain = world.components().domain_of(key).expect("registered");
    let (_, metadata) = world
        .entities
        .get(entity.index as usize)
        .expect("a live entity");
    let location = metadata.as_ref().expect("a placed entity").locations[&domain];
    let column = world.storage.pages[location.page_id as usize]
        .columns
        .get_mut(&key)
        .expect("the entity holds the component");
    (column.as_mut(), location.row_index as usize)
}

/// `ops.write_fields` on `entity`'s row of `key`.
fn write(
    world: &mut World,
    entity: EntityId,
    key: ComponentKey,
    patch: &[(&str, ScriptValue)],
) -> Result<(), FieldWriteError> {
    let ops = ops(world, key);
    let slotted: Vec<(usize, ScriptValue)> = patch
        .iter()
        .map(|(name, value)| {
            (
                ops.field_slot(name)
                    .unwrap_or_else(|| panic!("`{name}` has a slot")),
                value.clone(),
            )
        })
        .collect();
    let by_ref: Vec<(usize, &ScriptValue)> =
        slotted.iter().map(|(slot, value)| (*slot, value)).collect();
    let (column, row) = cell_mut(world, entity, key);
    ops.write_fields(column, row, &by_ref)
}

fn entity_at(index: u32) -> EntityId {
    EntityId {
        index,
        generation: 1,
    }
}

fn lighthouse() -> Lighthouse {
    Lighthouse {
        range: 40.0,
        channel: 16,
        call_sign: "North".to_owned(),
        target: Some(entity_at(3)),
    }
}

/// A declared component, `Signal { strength: int, tint: Color, note: Value }`.
fn declare_signal(world: &mut World) -> ComponentKey {
    let field = |name: &str, kind, default| PackedField {
        name: name.to_owned(),
        kind,
        default,
    };
    world
        .register_runtime_component(RuntimeComponentDecl {
            name: "Signal".to_owned(),
            domain: SemanticDomain::Spatial,
            provenance: ComponentProvenance::Authored,
            layout: PackedLayout::new(vec![
                field("strength", FieldKind::Int, ScriptValue::Int(0)),
                field(
                    "tint",
                    FieldKind::Color,
                    ScriptValue::Color(LinearRgba::WHITE),
                ),
                field("note", FieldKind::Value, ScriptValue::Str(String::new())),
            ])
            .expect("a valid layout"),
        })
        .expect("Signal registers")
}

fn fields(pairs: &[(&str, ScriptValue)]) -> ScriptValue {
    ScriptValue::Struct(
        pairs
            .iter()
            .map(|(name, value)| ((*name).to_owned(), value.clone()))
            .collect(),
    )
}

// ─── What the derive says ───────────────────────────────────────────────────

/// **`skip` holds in the script view too.** A skipped field is absent from the
/// mirror and the schema; it has no slot either, and the slots of the fields
/// around it are their positions in that same list.
#[test]
fn skip_fields_are_not_accessible() {
    assert_eq!(Lantern::script_fields(), ["brightness", "lit"]);
    assert_eq!(Lantern::script_type(0), Some(ErgonType::Float));
    assert_eq!(Lantern::script_type(1), Some(ErgonType::Bool));
    assert_eq!(Lantern::script_type(2), None, "no third slot");

    let world = world();
    let ops = ops(&world, key::<Lantern>());
    assert_eq!(ops.field_slot("flicker_cache"), None);
    assert_eq!(ops.field_slot("brightness"), Some(0));
    assert_eq!(ops.field_slot("lit"), Some(1));
    assert_eq!(ops.field_slot("nonexistent"), None);

    // The schema is built from the same list: one name per slot, in order.
    let reg = registration_for(TypeId::of::<Lantern>()).expect("Lantern registers");
    let ComponentShape::Fields(schema) = reg.shape else {
        panic!("a struct has fields");
    };
    let names: Vec<&str> = schema.iter().map(|field| field.name).collect();
    assert_eq!(names, Lantern::script_fields());
}

/// **A field type no script can hold does not stop the derive.** `ticks` is a
/// `u64`: it keeps its slot — it is a field of the component — but has no
/// script type, reads as nothing, and refuses a write as not accessible
/// rather than as unknown. `km`, beside it, works.
#[test]
fn an_unsupported_field_type_does_not_break_the_derive() {
    assert_eq!(Odometer::script_fields(), ["km", "ticks"]);
    assert_eq!(Odometer::script_type(0), Some(ErgonType::Float));
    assert_eq!(Odometer::script_type(1), None);
    let reg = registration_for(TypeId::of::<Odometer>()).expect("Odometer registers");
    assert_eq!((reg.script_type)(0), Some(ErgonType::Float));
    assert_eq!((reg.script_type)(1), None);

    let mut odometer = Odometer {
        km: 12.5,
        ticks: 99,
    };
    assert_eq!(odometer.read_field(0), Some(ScriptValue::Float(12.5)));
    assert_eq!(odometer.read_field(1), None);
    assert!(odometer.write_field(1, &ScriptValue::Int(5)).is_err());
    odometer
        .write_field(0, &ScriptValue::Float(3.0))
        .expect("km takes a float");
    assert_eq!(odometer, Odometer { km: 3.0, ticks: 99 });

    let mut world = world();
    let entity = world.spawn(Odometer { km: 1.0, ticks: 7 });
    let key = key::<Odometer>();
    assert_eq!(ops(&world, key).field_slot("ticks"), Some(1));
    assert_eq!(
        write(&mut world, entity, key, &[("ticks", ScriptValue::Int(5))]),
        Err(FieldWriteError::NotAccessible("ticks".to_owned()))
    );
    let (column, row) = cell(&world, entity, key);
    assert_eq!(ops(&world, key).read_field(column, row, 1), None);
    assert_eq!(
        world.get::<Odometer>(entity),
        Some(&Odometer { km: 1.0, ticks: 7 })
    );
}

/// **The type, not its spelling.** `length` is spelled `Meters` and `stops`
/// `Stops`; what a script sees is what they are — a `float` and an
/// `Entity[]` — because the answer comes from the type's `ScriptField` impl,
/// where the type is known, and the registration carries it to the mirror.
#[test]
fn the_registration_types_a_field_by_its_trait_not_its_spelling() {
    let reg = registration_for(TypeId::of::<Ruler>()).expect("Ruler registers");
    let ComponentShape::Fields(schema) = reg.shape else {
        panic!("a struct has fields");
    };
    assert_eq!(schema[0].ty, "Meters", "the spelling hides the type");
    assert_eq!(schema[1].ty, "Stops");

    assert_eq!((reg.script_type)(0), Some(ErgonType::Float));
    assert_eq!(
        (reg.script_type)(1),
        Some(ErgonType::Array(Box::new(ErgonType::Entity)))
    );
    assert_eq!(Ruler::script_type(0), Some(ErgonType::Float));
}

// ─── Rows of Rust components ────────────────────────────────────────────────

/// **`RowRef::field` reads a Rust component** — array-of-structs and
/// field-split alike — not only a declared one.
#[test]
fn rust_rows_read_their_fields() {
    let mut world = world();
    let entity = world.spawn((lighthouse(), Transform::identity()));
    world
        .add_component(entity, Sway { dx: 0.5, dy: -1.5 })
        .expect("Sway attaches");

    let beacon = key::<Lighthouse>();
    let row = world.row(entity, beacon).expect("the entity holds it");
    assert_eq!(
        row.field(slot(&world, beacon, "range")),
        Some(ScriptValue::Float(40.0))
    );
    assert_eq!(
        row.field(slot(&world, beacon, "channel")),
        Some(ScriptValue::Int(16))
    );
    assert_eq!(
        row.field(slot(&world, beacon, "call_sign")),
        Some(ScriptValue::Str("North".to_owned()))
    );
    assert_eq!(
        row.field(slot(&world, beacon, "target")),
        Some(ScriptValue::Entity(entity_at(3)))
    );

    let sway = key::<Sway>();
    let row = world.row(entity, sway).expect("the entity holds it");
    assert_eq!(
        row.field(slot(&world, sway, "dy")),
        Some(ScriptValue::Float(-1.5))
    );
}

/// And `RowMut::set_field` writes one, the other fields untouched.
#[test]
fn rust_rows_write_their_fields() {
    let mut world = world();
    let entity = world.spawn(lighthouse());
    world
        .add_component(entity, Sway { dx: 0.5, dy: -1.5 })
        .expect("Sway attaches");

    let beacon = key::<Lighthouse>();
    let call_sign = slot(&world, beacon, "call_sign");
    world
        .row_mut(entity, beacon)
        .expect("the entity holds it")
        .set_field(call_sign, &ScriptValue::Str("South".to_owned()))
        .expect("a string field takes a string");
    assert_eq!(
        world.get::<Lighthouse>(entity),
        Some(&Lighthouse {
            call_sign: "South".to_owned(),
            ..lighthouse()
        })
    );

    let sway = key::<Sway>();
    let dx = slot(&world, sway, "dx");
    world
        .row_mut(entity, sway)
        .expect("the entity holds it")
        .set_field(dx, &ScriptValue::Float(4.0))
        .expect("a float field takes a float");
    assert_eq!(
        world.clone_component::<Sway>(entity),
        Some(Sway { dx: 4.0, dy: -1.5 })
    );
}

// ─── Typed writes through the column operations ─────────────────────────────

/// Only the fields named are written — in an array-of-structs column and in a
/// field-split one.
#[test]
fn a_typed_write_touches_only_the_named_fields() {
    let mut world = world();
    let entity = world.spawn((lighthouse(), Sway { dx: 0.5, dy: -1.5 }));

    write(
        &mut world,
        entity,
        key::<Lighthouse>(),
        &[
            ("channel", ScriptValue::Int(9)),
            ("target", ScriptValue::Null),
        ],
    )
    .expect("both values fit");
    assert_eq!(
        world.get::<Lighthouse>(entity),
        Some(&Lighthouse {
            channel: 9,
            target: None,
            ..lighthouse()
        })
    );

    write(
        &mut world,
        entity,
        key::<Sway>(),
        &[("dy", ScriptValue::Float(2.0))],
    )
    .expect("a float fits");
    assert_eq!(
        world.clone_component::<Sway>(entity),
        Some(Sway { dx: 0.5, dy: 2.0 })
    );
}

/// **All or nothing.** A patch with one good value and one the field refuses
/// writes neither, and names the field that refused.
#[test]
fn a_refused_value_writes_nothing() {
    let mut world = world();
    let entity = world.spawn((lighthouse(), Sway { dx: 0.5, dy: -1.5 }));

    let refused = write(
        &mut world,
        entity,
        key::<Lighthouse>(),
        &[
            ("range", ScriptValue::Float(99.0)),
            ("channel", ScriptValue::Int(300)),
        ],
    );
    assert!(
        matches!(&refused, Err(FieldWriteError::Refused { field, .. }) if field == "channel"),
        "got {refused:?}"
    );
    assert_eq!(world.get::<Lighthouse>(entity), Some(&lighthouse()));

    // The refused value first, the good one after: still nothing.
    let refused = write(
        &mut world,
        entity,
        key::<Sway>(),
        &[
            ("dx", ScriptValue::Float(f32::NAN)),
            ("dy", ScriptValue::Float(7.0)),
        ],
    );
    assert!(
        matches!(&refused, Err(FieldWriteError::Refused { field, .. }) if field == "dx"),
        "got {refused:?}"
    );
    assert_eq!(
        world.clone_component::<Sway>(entity),
        Some(Sway { dx: 0.5, dy: -1.5 })
    );

    // A value of the wrong kind is a refusal too.
    let refused = write(
        &mut world,
        entity,
        key::<Lighthouse>(),
        &[("call_sign", ScriptValue::Int(1))],
    );
    assert!(
        matches!(&refused, Err(FieldWriteError::Refused { field, .. }) if field == "call_sign"),
        "got {refused:?}"
    );
    assert_eq!(world.get::<Lighthouse>(entity), Some(&lighthouse()));
}

/// A declared component is read and written through the same operations,
/// from its layout, with the same all-or-nothing rule.
#[test]
fn a_declared_component_is_read_and_written_through_its_column_ops() {
    let mut world = world();
    let signal = declare_signal(&mut world);
    let entity = world.spawn(Transform::identity());
    world
        .add_runtime_component(
            entity,
            signal,
            &fields(&[("strength", ScriptValue::Int(3))]),
        )
        .expect("Signal attaches");

    let ops = ops(&world, signal);
    assert_eq!(ops.field_slot("strength"), Some(0));
    assert_eq!(ops.field_slot("note"), Some(2));
    assert_eq!(ops.field_slot("nonexistent"), None);

    write(
        &mut world,
        entity,
        signal,
        &[("note", ScriptValue::Str("ready".to_owned()))],
    )
    .expect("a Value field takes any value");
    let refused = write(
        &mut world,
        entity,
        signal,
        &[
            ("strength", ScriptValue::Int(8)),
            ("tint", ScriptValue::Float(1.0)),
        ],
    );
    assert!(
        matches!(&refused, Err(FieldWriteError::Refused { field, .. }) if field == "tint"),
        "got {refused:?}"
    );

    let (column, row) = cell(&world, entity, signal);
    assert_eq!(ops.read_field(column, row, 0), Some(ScriptValue::Int(3)));
    assert_eq!(
        ops.read_field(column, row, 1),
        Some(ScriptValue::Color(LinearRgba::WHITE))
    );
    assert_eq!(
        ops.read_field(column, row, 2),
        Some(ScriptValue::Str("ready".to_owned()))
    );
}

// ─── Snapshots ──────────────────────────────────────────────────────────────

/// The names a script reaches component `key` by, in slot order: a Rust
/// component's from its registration's schema, a declared one's from its
/// layout.
fn names_of(world: &World, key: ComponentKey) -> Vec<String> {
    let vtable = world.components().vtable(key).expect("registered");
    if let Some(layout) = vtable.columns.packed() {
        return layout
            .fields()
            .iter()
            .map(|field| field.name.clone())
            .collect();
    }
    let Some(rust) = vtable.rust else {
        return Vec::new();
    };
    match registration_for(rust.type_id).map(|reg| reg.shape) {
        Some(ComponentShape::Fields(schema)) => {
            schema.iter().map(|field| field.name.to_owned()).collect()
        }
        _ => Vec::new(),
    }
}

/// Row `row` of `column`, built field by field from `read_field`: every field
/// a script reaches, in slot order, the others left out.
fn struct_from_fields(
    ops: &dyn ColumnOps,
    names: &[String],
    column: &dyn AnyVec,
    row: usize,
) -> ScriptValue {
    ScriptValue::Struct(
        names
            .iter()
            .filter_map(|name| {
                let slot = ops.field_slot(name)?;
                Some((name.clone(), ops.read_field(column, row, slot)?))
            })
            .collect(),
    )
}

/// **A snapshot reads what the world holds.** Every component a world stores
/// — the engine's, each with its default on an entity of its own, and the
/// test components and a declared one with values — is snapshotted column by
/// column, and every row of the copy equals the struct built from the
/// column's own `read_field`.
#[test]
fn snapshot_reads_match_the_world() {
    let mut world = world();
    let signal = declare_signal(&mut world);

    let storable: Vec<_> = inventory::iter::<crate::scene::ComponentRegistration>
        .into_iter()
        .filter(|reg| world.component_domain(reg.type_id).is_some())
        .collect();
    for reg in &storable {
        let entity = world.spawn(Pin);
        (reg.create_default)(&mut world, entity)
            .unwrap_or_else(|error| panic!("{}: create_default failed: {error}", reg.type_name));
    }

    let valued = world.spawn((
        lighthouse(),
        Sway { dx: 0.25, dy: 8.0 },
        Transform::from_translation(Vec3::new(1.0, 2.0, 3.0)),
    ));
    world
        .add_component(
            valued,
            Ruler {
                length: 2.5,
                stops: vec![entity_at(1), entity_at(2)],
            },
        )
        .expect("Ruler attaches");
    world
        .add_component(
            valued,
            Lantern {
                brightness: 0.75,
                flicker_cache: 4,
                lit: true,
            },
        )
        .expect("Lantern attaches");
    world
        .add_component(valued, Odometer { km: 6.0, ticks: 12 })
        .expect("Odometer attaches");
    world
        .add_runtime_component(
            valued,
            signal,
            &fields(&[
                ("strength", ScriptValue::Int(5)),
                ("note", ScriptValue::Str("on".to_owned())),
            ]),
        )
        .expect("Signal attaches");

    let keys: Vec<ComponentKey> = world.components().iter().map(|vtable| vtable.key).collect();
    let mut rows_with_fields = 0usize;
    for key in keys {
        let ops = ops(&world, key);
        let names = names_of(&world, key);
        for page in &world.storage.pages {
            let Some(column) = page.columns.get(&key) else {
                continue;
            };
            let column = column.as_ref();
            let snapshot = ops.snapshot(column);
            assert_eq!(snapshot.len(), column.len(), "{key:?}: one row per row");
            for row in 0..column.len() {
                let expected = struct_from_fields(ops.as_ref(), &names, column, row);
                if expected
                    .as_fields()
                    .is_some_and(|fields| !fields.is_empty())
                {
                    rows_with_fields += 1;
                }
                assert_eq!(snapshot.read(row), expected, "{key:?}, row {row}");
            }
        }
    }
    // Not vacuous: the defaults of the test components alone are six such
    // rows, and the valued entity six more.
    assert!(
        rows_with_fields >= 12,
        "the snapshots covered only {rows_with_fields} rows with fields"
    );

    // And the values are the ones written, not merely self-consistent.
    let (column, row) = cell(&world, valued, key::<Lighthouse>());
    assert_eq!(
        ops(&world, key::<Lighthouse>()).snapshot(column).read(row),
        fields(&[
            ("range", ScriptValue::Float(40.0)),
            ("channel", ScriptValue::Int(16)),
            ("call_sign", ScriptValue::Str("North".to_owned())),
            ("target", ScriptValue::Entity(entity_at(3))),
        ])
    );
    let (column, row) = cell(&world, valued, key::<Odometer>());
    assert_eq!(
        ops(&world, key::<Odometer>()).snapshot(column).read(row),
        fields(&[("km", ScriptValue::Float(6.0))]),
        "a field no script reaches is left out"
    );
    let (column, row) = cell(&world, valued, signal);
    assert_eq!(
        ops(&world, signal).snapshot(column).read(row),
        fields(&[
            ("strength", ScriptValue::Int(5)),
            ("tint", ScriptValue::Color(LinearRgba::WHITE)),
            ("note", ScriptValue::Str("on".to_owned())),
        ])
    );
}

/// A snapshot is a copy: the world written after it was taken does not
/// change what it reads.
#[test]
fn a_snapshot_is_a_copy_the_world_does_not_change() {
    let mut world = world();
    let entity = world.spawn((lighthouse(), Sway { dx: 0.5, dy: -1.5 }));

    let (column, row) = cell(&world, entity, key::<Sway>());
    let snapshot = ops(&world, key::<Sway>()).snapshot(column);
    write(
        &mut world,
        entity,
        key::<Sway>(),
        &[("dx", ScriptValue::Float(9.0))],
    )
    .expect("a float fits");

    assert_eq!(
        snapshot.read(row),
        fields(&[
            ("dx", ScriptValue::Float(0.5)),
            ("dy", ScriptValue::Float(-1.5)),
        ])
    );
}
