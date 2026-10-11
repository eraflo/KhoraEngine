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

//! `World::write_column`: many rows of one component, by key and slot, in
//! one pass — what it reports, the epoch it bumps, and the Rust rows it
//! writes in place.

use std::cell::Cell;

use khora_core::ecs::entity::EntityId;
use khora_core::script::ScriptValue;

use crate::ecs::{ColumnWriteReport, ComponentKey, SemanticDomain, Transform, World};

thread_local! {
    /// How many times a `Gauge` was cloned on this thread.
    static GAUGE_CLONES: Cell<usize> = const { Cell::new(0) };
}

/// A Physics component that counts its clones.
#[derive(Debug, Default, PartialEq, khora_macros::Component)]
#[component(domain = Physics)]
struct Gauge {
    level: i64,
    label: String,
}

impl Clone for Gauge {
    fn clone(&self) -> Self {
        GAUGE_CLONES.with(|clones| clones.set(clones.get() + 1));
        Self {
            level: self.level,
            label: self.label.clone(),
        }
    }
}

fn gauge(level: i64) -> Gauge {
    Gauge {
        level,
        label: format!("gauge {level}"),
    }
}

/// A world of `count` entities, each holding a `Transform` and a `Gauge`
/// whose level is its index.
fn gauges(count: i64) -> (World, ComponentKey, Vec<EntityId>) {
    let mut world = World::new();
    let key = world
        .try_register_component::<Gauge>(SemanticDomain::Physics)
        .expect("Gauge registers");
    let entities = (0..count)
        .map(|level| world.spawn((Transform::identity(), gauge(level))))
        .collect();
    (world, key, entities)
}

fn slot(world: &World, key: ComponentKey, name: &str) -> usize {
    world
        .components()
        .vtable(key)
        .expect("registered")
        .columns
        .field_slot(name)
        .unwrap_or_else(|| panic!("Gauge has a field `{name}`"))
}

/// Rows as `write_column` takes them, borrowed from owned values.
fn borrowed(rows: &[(EntityId, Vec<ScriptValue>)]) -> Vec<(EntityId, &[ScriptValue])> {
    rows.iter()
        .map(|(entity, values)| (*entity, values.as_slice()))
        .collect()
}

/// **The report.** Rows written, rows whose entity is gone or does not
/// hold the component, rows a field refused — each counted where it fell.
#[test]
fn a_column_write_reports_what_it_wrote_and_skipped() {
    let (mut world, key, entities) = gauges(4);
    world.despawn(entities[3]);
    let bare = world.spawn(Transform::identity());
    let level = slot(&world, key, "level");

    let rows = vec![
        (entities[0], vec![ScriptValue::Int(10)]),
        (entities[1], vec![ScriptValue::Str("ten".to_owned())]),
        (entities[2], vec![ScriptValue::Int(12)]),
        (entities[3], vec![ScriptValue::Int(13)]),
        (bare, vec![ScriptValue::Int(14)]),
    ];
    let report = world.write_column(key, &[level], borrowed(&rows));

    assert_eq!(
        report,
        ColumnWriteReport {
            written: 2,
            missing: 2,
            refused: 1,
        }
    );
    assert_eq!(world.get::<Gauge>(entities[0]).map(|g| g.level), Some(10));
    assert_eq!(world.get::<Gauge>(entities[1]).map(|g| g.level), Some(1));
    assert_eq!(world.get::<Gauge>(entities[2]).map(|g| g.level), Some(12));
}

/// **One bump per batch.** A batch that wrote something marks the
/// component's domain changed once — not once per row — and no other domain.
#[test]
fn a_column_write_bumps_the_domain_epoch_once() {
    let (mut world, key, entities) = gauges(100);
    let level = slot(&world, key, "level");
    let physics = world.domain_epoch(SemanticDomain::Physics);
    let spatial = world.domain_epoch(SemanticDomain::Spatial);

    let rows: Vec<_> = entities
        .iter()
        .map(|&e| (e, vec![ScriptValue::Int(-1)]))
        .collect();
    let report = world.write_column(key, &[level], borrowed(&rows));

    assert_eq!(report.written, 100);
    assert_eq!(
        world.domain_epoch(SemanticDomain::Physics),
        physics.wrapping_add(1)
    );
    assert_eq!(world.domain_epoch(SemanticDomain::Spatial), spatial);
}

/// **Nothing written, nothing changed.** A batch whose every row was
/// skipped — missing or refused — or that held no row leaves the epoch as
/// it was, so no cached view is thrown away for it.
#[test]
fn a_column_write_that_wrote_nothing_leaves_the_epoch_alone() {
    let (mut world, key, entities) = gauges(3);
    let level = slot(&world, key, "level");
    world.despawn(entities[2]);
    let epoch = world.domain_epoch(SemanticDomain::Physics);

    let missing = vec![(entities[2], vec![ScriptValue::Int(5)])];
    let refused = vec![
        (entities[0], vec![ScriptValue::Bool(true)]),
        (entities[1], vec![ScriptValue::Float(f32::NAN)]),
    ];

    let report = world.write_column(key, &[level], borrowed(&missing));
    assert_eq!(report.missing, 1);
    let report = world.write_column(key, &[level], borrowed(&refused));
    assert_eq!(report.refused, 2);
    let report = world.write_column(key, &[level], Vec::new());
    assert_eq!(report, ColumnWriteReport::default());

    assert_eq!(world.domain_epoch(SemanticDomain::Physics), epoch);
}

/// **In place.** A Rust component's rows are written where they are: no
/// copy of a row is made to write it, and the fields not named — the
/// label — keep their values.
#[test]
fn a_rust_column_is_written_in_place() {
    let (mut world, key, entities) = gauges(500);
    let level = slot(&world, key, "level");
    let rows: Vec<_> = entities
        .iter()
        .enumerate()
        .map(|(i, &e)| (e, vec![ScriptValue::Int(i as i64 * 2)]))
        .collect();
    GAUGE_CLONES.with(|clones| clones.set(0));

    let report = world.write_column(key, &[level], borrowed(&rows));

    assert_eq!(report.written, 500);
    assert_eq!(GAUGE_CLONES.with(Cell::get), 0, "a Gauge was cloned");
    for (i, &entity) in entities.iter().enumerate() {
        assert_eq!(
            world.get::<Gauge>(entity),
            Some(&Gauge {
                level: i as i64 * 2,
                label: format!("gauge {i}"),
            }),
            "row {i}"
        );
    }
}

/// **Checked whole before any field of it is written.** A row whose second
/// value is refused does not keep its first; the rows around it land.
#[test]
fn a_refused_rust_row_writes_none_of_its_fields() {
    let (mut world, key, entities) = gauges(3);
    let slots = [slot(&world, key, "level"), slot(&world, key, "label")];

    let rows = vec![
        (
            entities[0],
            vec![ScriptValue::Int(40), ScriptValue::Str("a".to_owned())],
        ),
        (entities[1], vec![ScriptValue::Int(41), ScriptValue::Int(0)]),
        (
            entities[2],
            vec![ScriptValue::Int(42), ScriptValue::Str("c".to_owned())],
        ),
    ];
    let report = world.write_column(key, &slots, borrowed(&rows));

    assert_eq!(report.written, 2);
    assert_eq!(report.refused, 1);
    assert_eq!(world.get::<Gauge>(entities[1]), Some(&gauge(1)));
    assert_eq!(
        world.get::<Gauge>(entities[0]),
        Some(&Gauge {
            level: 40,
            label: "a".to_owned(),
        })
    );
    assert_eq!(
        world.get::<Gauge>(entities[2]),
        Some(&Gauge {
            level: 42,
            label: "c".to_owned(),
        })
    );
}

/// A Physics component whose `reading` a Rust system may leave unbounded.
#[derive(Debug, Clone, Default, PartialEq, khora_macros::Component)]
#[component(domain = Physics)]
struct Dial {
    reading: f32,
    label: String,
}

/// **A refused row is left as it was, whatever it held.** A row may hold a
/// value a script could not write — an infinite `f32` a Rust system put
/// there — and a refused later field must still put the earlier one back:
/// the row reads as before, not half written.
#[test]
fn a_refused_rust_row_keeps_a_value_no_script_could_write() {
    let mut world = World::new();
    let key = world
        .try_register_component::<Dial>(SemanticDomain::Physics)
        .expect("Dial registers");
    let held = Dial {
        reading: f32::INFINITY,
        label: "open".to_owned(),
    };
    let entity = world.spawn((Transform::identity(), held.clone()));
    let columns = &world.components().vtable(key).expect("registered").columns;
    let slots = [
        columns.field_slot("reading").expect("a reading"),
        columns.field_slot("label").expect("a label"),
    ];

    let rows = vec![(entity, vec![ScriptValue::Float(1.0), ScriptValue::Int(0)])];
    let report = world.write_column(key, &slots, borrowed(&rows));

    assert_eq!(report.refused, 1);
    assert_eq!(report.written, 0);
    assert_eq!(
        world.get::<Dial>(entity),
        Some(&held),
        "the row was half written"
    );
}

/// A Physics component stored field by field.
#[derive(Debug, Clone, Copy, Default, PartialEq, khora_macros::Component)]
#[component(no_serializable, domain = Physics, layout = "soa")]
struct SplitGauge {
    x: f32,
    y: f32,
}

/// **A field-split column takes the copy road.** Its rows are not an array
/// of `SplitGauge`, so they are written one by one as a `Set` writes them: every
/// row lands, the field not named keeps its value, and a refused row writes
/// none of its fields.
#[test]
fn a_field_split_column_is_written_row_by_row() {
    let mut world = World::new();
    let key = world
        .try_register_component::<SplitGauge>(SemanticDomain::Physics)
        .expect("SplitGauge registers");
    let entities: Vec<_> = (0..4)
        .map(|i| {
            world.spawn((
                Transform::identity(),
                SplitGauge {
                    x: i as f32,
                    y: -(i as f32),
                },
            ))
        })
        .collect();
    let columns = &world.components().vtable(key).expect("registered").columns;
    let slots = [
        columns.field_slot("x").expect("an x"),
        columns.field_slot("y").expect("a y"),
    ];

    let rows = vec![
        (
            entities[0],
            vec![ScriptValue::Float(10.0), ScriptValue::Float(20.0)],
        ),
        (
            entities[1],
            vec![ScriptValue::Float(11.0), ScriptValue::Bool(true)],
        ),
        (
            entities[3],
            vec![ScriptValue::Float(13.0), ScriptValue::Float(23.0)],
        ),
    ];
    let report = world.write_column(key, &slots, borrowed(&rows));

    assert_eq!(
        report,
        ColumnWriteReport {
            written: 2,
            missing: 0,
            refused: 1,
        }
    );
    // Read through the column operations: a field-split row is not a `&SplitGauge`.
    let drift = |entity: EntityId| {
        let row = world.row(entity, key).expect("holds a SplitGauge");
        (row.field(slots[0]), row.field(slots[1]))
    };
    let float = |v: f32| Some(ScriptValue::Float(v));
    assert_eq!(drift(entities[0]), (float(10.0), float(20.0)));
    assert_eq!(drift(entities[1]), (float(1.0), float(-1.0)));
    assert_eq!(drift(entities[2]), (float(2.0), float(-2.0)));
    assert_eq!(drift(entities[3]), (float(13.0), float(23.0)));
}

/// **One value a slot.** A row holding fewer or more values than the batch
/// writes is refused and counted, whether or not its entity is still there,
/// and writes none of them; the rows of the right width land.
#[test]
fn a_row_of_another_width_than_the_slots_is_refused() {
    let (mut world, key, entities) = gauges(4);
    let slots = [slot(&world, key, "level"), slot(&world, key, "label")];
    world.despawn(entities[3]);

    let rows = vec![
        (entities[0], vec![ScriptValue::Int(50)]),
        (
            entities[1],
            vec![
                ScriptValue::Int(51),
                ScriptValue::Str("b".to_owned()),
                ScriptValue::Int(0),
            ],
        ),
        (
            entities[2],
            vec![ScriptValue::Int(52), ScriptValue::Str("c".to_owned())],
        ),
        (entities[3], vec![]),
    ];
    let report = world.write_column(key, &slots, borrowed(&rows));

    assert_eq!(
        report,
        ColumnWriteReport {
            written: 1,
            missing: 0,
            refused: 3,
        }
    );
    assert_eq!(world.get::<Gauge>(entities[0]), Some(&gauge(0)));
    assert_eq!(world.get::<Gauge>(entities[1]), Some(&gauge(1)));
    assert_eq!(
        world.get::<Gauge>(entities[2]),
        Some(&Gauge {
            level: 52,
            label: "c".to_owned(),
        })
    );
}

/// **No field, no write.** A batch of no field changes no row, reports no
/// row and leaves the epoch as it was.
#[test]
fn a_column_write_of_no_field_writes_reports_and_bumps_nothing() {
    let (mut world, key, entities) = gauges(3);
    let epoch = world.domain_epoch(SemanticDomain::Physics);

    let rows: Vec<_> = entities.iter().map(|&e| (e, Vec::new())).collect();
    let report = world.write_column(key, &[], borrowed(&rows));

    assert_eq!(report, ColumnWriteReport::default());
    assert_eq!(world.domain_epoch(SemanticDomain::Physics), epoch);
    for (i, &entity) in entities.iter().enumerate() {
        assert_eq!(world.get::<Gauge>(entity), Some(&gauge(i as i64)));
    }
}
